//! `nodal reclaim --check` says what a reclaim would do, and does none of it.
//!
//! `tests/reclaim_refusal.rs` holds that a reclaim refuses work that exists nowhere
//! else, and `tests/reclaim_fresh_uniqueness.rs` holds that a stale ref never makes one
//! safe. Both are about the operation. This file is about the answer a person reads
//! *before* they type it, and it has two halves that are equally load-bearing.
//!
//! **It must agree with the operation.** A preflight that said safe where the reclaim
//! refuses is worse than no preflight at all, because a person stops checking. There is
//! one evaluator under both ([`nodal_core::lifecycle::assess`]), and every refusing case
//! below puts the same unit to `nodal reclaim` and asserts that it refuses too — with
//! two stated exceptions, and no others:
//!
//! * `the_check_changes_nothing_and_runs_no_hook` does not, because a second command is
//!   the one thing it is asserting did not happen.
//! * `check_refuses_force_and_yes` is about the flags and reaches no verdict at all.
//!
//! A reclaim refused over work is free to ask: the uniqueness check runs before the
//! first hook and before any plan, so nothing has happened by the time it raises. A
//! reclaim refused over a **bystander** is not free — the teardown has already stopped
//! what carries the unit's identifier — so that one comparison is made last, after every
//! reading it could disturb.
//!
//! The safe cases are not put to the operation, because the only way to compare them is
//! to let the reclaim go through — and a unit that has been reclaimed is no longer there
//! to assert anything about. What can be shown without ending it is shown.
//!
//! **It must do nothing.** No hook runs, no signal is sent, no container is touched, no
//! port comes back, no snapshot is taken, nothing moves to the trash, no registry row is
//! written and no remote is reached. The last property asserts the checkout, the home,
//! the whole state directory and the registry's rows are what they were, and that a unit
//! whose declared hook this machine has *not* approved is still answered — because a
//! command that ran the hook would be refused there.
//!
//! Every property runs on a machine whose checkout has a bare `origin` beside it, and
//! every `git` Nodal starts on it may use the local file transport and nothing else. A
//! property that reached a network would fail rather than quietly succeed on whichever
//! host happened to be online.
//!
//! | property | test |
//! |---|---|
//! | work in the tree refuses, and so does the reclaim | `work_in_the_tree_refuses_and_the_reclaim_refuses_the_same_way` |
//! | an only copy is named as one | `a_commit_nothing_else_has_is_reported_as_only_here` |
//! | a second copy here is not a remote proof | `a_commit_on_a_sibling_branch_is_a_second_local_copy` |
//! | an object no ref reaches is no second copy | `a_commit_a_sibling_has_under_no_ref_is_only_here` |
//! | a ref reaches through its ancestry too | `a_sibling_branch_reaches_the_commits_behind_its_tip` |
//! | collecting garbage moves no verdict | `garbage_collection_in_a_sibling_moves_no_verdict` |
//! | a current reading is a remote proof | `a_commit_a_current_reading_reaches_is_proved_on_the_remote` |
//! | an older, partial or unread witness proves nothing | `a_witness_that_cannot_answer_leaves_the_commits_not_checked` |
//! | a name is not an object store | `a_ref_with_no_object_behind_it_proves_nothing` |
//! | a reading that failed proves nothing | `a_checkout_whose_object_store_will_not_answer_proves_nothing` |
//! | the remote on this disk is read directly | `a_remote_that_is_this_disk_is_read_directly` |
//! | squash-absorbed objects are a keep | `squash_absorbed_work_whose_objects_are_only_here_is_a_keep` |
//! | heavy ignored state is reconstructable | `heavy_ignored_state_is_reconstructable_and_priced_as_apparent` |
//! | owned runtime is named and does not block | `owned_runtime_is_named_and_is_not_a_reason_to_refuse` |
//! | a bystander blocks, and so it does for the reclaim | `a_bystander_blocks_the_move_and_the_reclaim_refuses_the_same_way` |
//! | another unit's process blocks in the list too | `another_units_process_in_this_home_blocks_the_list_and_the_check_alike` |
//! | a process this account cannot read, started from inside the home, blocks | `an_unreadable_process_whose_lineage_reaches_the_home_blocks_the_move_at_the_seam` |
//! | and one whose lineage reaches nothing here is counted, not hidden | `an_unreadable_process_unrelated_to_the_home_is_counted_and_refuses_nothing_at_the_seam` |
//! | it changes nothing | `the_check_changes_nothing_and_runs_no_hook` |
//! | one value, two renderings | `the_human_form_and_the_json_are_one_value` |
//! | it is not a way to force anything | `check_refuses_force_and_yes` |

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use nodal_core::lifecycle::assess::{Own, sort};
use nodal_core::lifecycle::journal;
use nodal_core::model::UnitId;
use nodal_core::runtime::processes::{Lineage, Running, Withheld};
use nodal_core::store::{environments, projects, units};
use nodal_safety::git::untouched;
use nodal_safety::{InState as _, Machine, Snapshot, answer, git, stderr};
use serde_json::Value;

/// The unit every property here asks about. One of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A second unit of the same project, for the one property that needs another unit's
/// identifier to exist.
const NEIGHBOUR: &str = "report-export";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// A path the fixture project commits, so a change to it is work no commit holds.
const TRACKED: &str = "apps/web/app/page.tsx";

/// The branch a home pushes its work to, as a review branch on the remote.
const TOPIC: &str = "topic";

/// The branch the checkout is left holding an object it cannot read on.
const DAMAGED: &str = "damaged";

/// What a loose object is set to before it is overwritten. Git writes them read-only.
const WRITABLE: u32 = 0o644;

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine with a remote, whose every Nodal command is held to the filesystem.
fn machine() -> Machine {
    Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY)
}

/// The preflight for one unit, as the value both renderings are made from.
fn check(machine: &Machine, slug: &str) -> Value {
    let asked = machine.nodal(&["reclaim", slug, "--check", "--json"]);
    let printed = answer(&asked);
    let read: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("--check --json is one document: {printed}{}", stderr(&asked)));
    assert_eq!(
        read["safe_to_reclaim"],
        Value::Bool(asked.status.success()),
        "the exit code and the verdict disagree: {printed}"
    );
    read
}

/// Insist that the preflight calls this home safe.
fn assert_safe(answer: &Value) {
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "{answer:#}");
}

/// The commit group of one disposition, or nothing when the reading found none.
fn commits<'a>(answer: &'a Value, kind: &str) -> Option<&'a Value> {
    answer["commits"]
        .as_array()?
        .iter()
        .find(|group| group["copies"]["kind"] == Value::String(kind.to_owned()))
}

/// The path group of one kind, or nothing when the reading found none.
fn paths<'a>(answer: &'a Value, held: &str) -> Option<&'a Value> {
    answer["paths"].as_array()?.iter().find(|group| group["held"] == Value::String(held.to_owned()))
}

/// How many things a group is about, and zero when there is no such group.
fn count(group: Option<&Value>) -> u64 {
    group.and_then(|group| group["count"].as_u64()).unwrap_or_default()
}

/// A unit whose one commit exists only in its home, pushed to `origin` as `TOPIC`.
///
/// The push is what writes the home's own `refs/remotes/origin/topic`. After it, the
/// home holds a ref naming a commit, and what the remote does with that branch next is
/// what each property here decides.
fn pushed(machine: &Machine, slug: &str) -> (PathBuf, String) {
    let home = machine.unit(slug);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    git(&home, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{TOPIC}")]);
    (home, tip)
}

/// The same, with the branch then deleted on the remote, as a host does after a merge.
fn stranded(machine: &Machine, slug: &str) -> (PathBuf, String) {
    let (home, tip) = pushed(machine, slug);
    git(machine.origin(), &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
    (home, tip)
}

/// The checkout as the registry names it, which is the name every report uses.
///
/// Resolved, because that is what a project row holds. On a machine whose temporary
/// directory is reached through a symbolic link — which macOS gives every test for free,
/// and `ci/acceptance-safety.sh` makes on Linux — the path a test built and the path a
/// report prints are two names for one directory.
fn checkout(machine: &Machine) -> Value {
    let resolved = std::fs::canonicalize(&machine.source).unwrap();
    Value::from(resolved.to_str().unwrap())
}

/// A second repository beside the checkout, which the sibling walk finds.
///
/// The walk starts at the checkout's parent, so a repository there is one of the other
/// copies a uniqueness reading asks about (`nodal_core::doctor::scan::siblings`).
fn sibling(machine: &Machine) -> PathBuf {
    let parent = machine.source.parent().unwrap().to_path_buf();
    git(&parent, &["init", "--quiet", "--initial-branch", "main", "sibling"]);
    parent.join("sibling")
}

/// A path as the registry and every report name it.
fn resolved(path: &Path) -> Value {
    Value::from(std::fs::canonicalize(path).unwrap().to_str().unwrap())
}

/// Insist that the home is where it was and plain Git still reaches the commit.
fn intact(machine: &Machine, home: &Path, tip: &str) {
    assert!(home.is_dir(), "the check moved the home");
    assert_eq!(machine.homes(), vec![home.to_path_buf()], "the home is still the project's");
    assert!(machine.trashed().is_empty(), "the check put something in the trash");
    assert_eq!(git(home, &["cat-file", "-t", tip]), "commit", "the commit is not readable");
}

/// Insist that an ordinary reclaim refuses the same unit, naming the same trouble.
///
/// A refused reclaim changes nothing — the uniqueness check runs before the first hook
/// and before any plan — so this is a reading of the operation's answer and not a use of
/// a destructive command as an oracle.
fn reclaim_also_refuses(machine: &Machine, slug: &str, naming: &str) {
    let refused = machine.nodal(&["reclaim", slug]);
    assert!(
        !refused.status.success(),
        "the check refused and the reclaim did not: {}",
        answer(&refused)
    );
    let told = stderr(&refused);
    assert!(told.contains(naming), "the reclaim refused for another reason: {told}");
}

/// The invariant, in the shape a person meets it in. A home with work in the tree is a
/// home a reclaim will not take, and the preflight says so before anything is typed.
#[test]
fn work_in_the_tree_refuses_and_the_reclaim_refuses_the_same_way() {
    let machine = machine();
    let home = machine.unit(SLUG);
    std::fs::write(home.join(TRACKED), "edited, not committed\n").unwrap();
    std::fs::write(home.join(ONLY), "written, not added\n").unwrap();

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false));
    assert_eq!(count(paths(&answer, "uncommitted")), 1, "{answer:#}");
    assert_eq!(count(paths(&answer, "untracked")), 1, "{answer:#}");
    for group in ["uncommitted", "untracked"] {
        assert_eq!(paths(&answer, group).unwrap()["disposition"], Value::from("must_survive"));
        assert!(paths(&answer, group).unwrap()["why"].as_str().is_some_and(|why| !why.is_empty()));
    }
    assert_eq!(answer["reasons"][0]["needs"], Value::from("unique_loss"));

    reclaim_also_refuses(&machine, SLUG, "uncommitted changes (1)");
    assert!(home.is_dir(), "the pair of readings moved the home");
}

/// The commit was never pushed, and the newest reading of the remote on this machine
/// does not reach it. Nothing else here holds it either, so it is only here as far as
/// anything on this disk can say — and the words say "by the newest reading" rather than
/// claiming the remote is empty, which is a claim a clone cannot make.
#[test]
fn a_commit_nothing_else_has_is_reported_as_only_here() {
    let machine = machine();
    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false));
    assert_eq!(count(commits(&answer, "only_here")), 1, "{answer:#}");
    assert_eq!(count(commits(&answer, "not_checked")), 0, "a reading was taken: {answer:#}");
    let group = commits(&answer, "only_here").unwrap();
    assert_eq!(group["sample"][0], Value::from(tip.as_str()), "the group does not name it");
    assert_eq!(group["copies"]["witness"]["kind"], Value::from("checked"), "{answer:#}");

    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// A second repository on this disk has the commit on a branch. That survives the
/// removal of this home whatever any remote has, and it is a different answer from a
/// remote proof: one depends on a server keeping a branch and the other does not.
#[test]
fn a_commit_on_a_sibling_branch_is_a_second_local_copy() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    let beside = sibling(&machine);
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD:refs/heads/copy"]);

    let answer = check(&machine, SLUG);
    assert_safe(&answer);
    assert_eq!(count(commits(&answer, "second_local_copy")), 1, "{answer:#}");
    assert_eq!(count(commits(&answer, "remote_proved")), 0, "the remote proved nothing here");
    let held_by = &commits(&answer, "second_local_copy").unwrap()["copies"]["held_by"];
    assert_eq!(held_by, &resolved(&beside));
    intact(&machine, &home, &tip);
}

/// The commit is in a sibling's object store and no ref of that sibling reaches it.
///
/// This is the reading that made a home look safe. `git fetch <url> HEAD` writes the objects
/// and `FETCH_HEAD`, and `FETCH_HEAD` is no ref: the commit is there and it is what the
/// next `git gc` in that repository removes. A second copy one ordinary command takes
/// away is no second copy, so the answer is `only_here` and the reclaim refuses.
#[test]
fn a_commit_a_sibling_has_under_no_ref_is_only_here() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    let beside = sibling(&machine);
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    assert_eq!(git(&beside, &["cat-file", "-t", &tip]), "commit", "the sibling has the object");

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "second_local_copy")), 0, "an object is not a copy");
    assert_eq!(count(commits(&answer, "only_here")), 1, "{answer:#}");
    assert_eq!(commits(&answer, "only_here").unwrap()["sample"][0], Value::from(tip.as_str()));

    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// `git gc --prune=now` in another repository moves no verdict, in either direction.
///
/// The commit under no ref reads `only_here` before the collection and `only_here`
/// after it, because the object the collection removes was never counted. The commit a
/// current reading of the remote proves reads `remote_proved` both times, because that
/// proof is about a server and not about a second object store on this disk.
#[test]
fn garbage_collection_in_a_sibling_moves_no_verdict() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    let beside = sibling(&machine);
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let before = check(&machine, SLUG);
    git(&beside, &["gc", "--quiet", "--prune=now"]);
    assert!(
        !nodal_safety::try_git(&beside, &["cat-file", "-e", &tip]).status.success(),
        "the collection kept the object, so this proves nothing"
    );
    let after = check(&machine, SLUG);
    assert_eq!(before["commits"], after["commits"], "the collection moved the reading");
    assert_eq!(count(commits(&after, "only_here")), 1, "{after:#}");

    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// A ref reaches a commit through its ancestry, and that is a second copy as well.
///
/// Every other property here puts the sibling's ref on the commit the reading asks
/// about. This one does not, so the exclusion side of the reading has to walk. The
/// sibling names the tip alone. The two commits behind it are reached only by going back
/// through that tip. A reading that stopped at the ref targets would call those two the
/// only copy, and refuse over work a second repository has.
#[test]
fn a_sibling_branch_reaches_the_commits_behind_its_tip() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let mut made = Vec::new();
    for number in 1..=3 {
        std::fs::write(home.join(ONLY), format!("the copy, written {number} times\n")).unwrap();
        git(&home, &["add", "--all"]);
        git(&home, &["commit", "--quiet", "--message", &format!("work number {number}")]);
        made.push(git(&home, &["rev-parse", "HEAD"]));
    }
    let beside = sibling(&machine);
    let tip = made[2].clone();
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), &format!("{tip}:refs/heads/copy")]);
    assert_eq!(git(&beside, &["rev-parse", "refs/heads/copy"]), tip, "the ref names the tip");

    let answer = check(&machine, SLUG);
    assert_safe(&answer);
    assert_eq!(count(commits(&answer, "second_local_copy")), 3, "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 0, "{answer:#}");
    let group = commits(&answer, "second_local_copy").unwrap();
    assert_eq!(&group["copies"]["held_by"], &resolved(&beside));
    let sample = group["sample"].as_array().expect("the group names what it found");
    let listed: Vec<&str> = sample.iter().filter_map(Value::as_str).collect();
    assert!(listed.contains(&made[0].as_str()), "the oldest commit is behind the tip: {answer:#}");
    intact(&machine, &home, &tip);
}

/// The same collection, on a unit whose work a current reading of the remote proves.
///
/// The remote proof rests on a witness the checkout read, so nothing another repository
/// on this disk drops may touch it.
#[test]
fn garbage_collection_in_a_sibling_leaves_a_remote_proof_standing() {
    let machine = machine();
    let (home, tip) = pushed(&machine, SLUG);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    let beside = sibling(&machine);
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);
    git(&beside, &["gc", "--quiet", "--prune=now"]);

    let answer = check(&machine, SLUG);
    assert_safe(&answer);
    assert_eq!(count(commits(&answer, "remote_proved")), 1, "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 0, "{answer:#}");
    intact(&machine, &home, &tip);
}

/// The person fetched, so their checkout is the newest reading of the remote and it
/// reaches the branch. The work is on a server, and the report says which reading says so.
#[test]
fn a_commit_a_current_reading_reaches_is_proved_on_the_remote() {
    let machine = machine();
    let (home, tip) = pushed(&machine, SLUG);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let answer = check(&machine, SLUG);
    assert_safe(&answer);
    assert_eq!(count(commits(&answer, "remote_proved")), 1, "{answer:#}");
    let witness = &commits(&answer, "remote_proved").unwrap()["copies"]["witness"];
    assert_eq!(witness["kind"], Value::from("checked"));
    assert_eq!(witness["by"][0], checkout(&machine));
    intact(&machine, &home, &tip);
}

/// Three ways a checkout fails to answer for a remote, and one answer to all of them.
///
/// It read the remote before the home pushed; it fetches one branch and so cannot say
/// another is gone; or nothing here ever read it. None of the three is a reading of the
/// remote as it is now, so the commits are `not_checked` — which is neither `only_here`,
/// a claim this machine did not earn, nor safe.
#[test]
fn a_witness_that_cannot_answer_leaves_the_commits_not_checked() {
    for (case, prepare) in [
        ("an older reading", older as fn(&Machine)),
        ("a partial reading", partial),
        ("no reading at all", |_: &Machine| {}),
    ] {
        let machine = machine();
        prepare(&machine);
        let (home, tip) = stranded(&machine, SLUG);

        let answer = check(&machine, SLUG);
        assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{case}: {answer:#}");
        assert_eq!(count(commits(&answer, "not_checked")), 1, "{case}: {answer:#}");
        assert_eq!(count(commits(&answer, "only_here")), 0, "{case}: a claim it did not earn");
        let witness = &commits(&answer, "not_checked").unwrap()["copies"]["witness"];
        assert_eq!(witness["kind"], Value::from("unchecked"), "{case}");
        assert_eq!(answer["reasons"][0]["needs"], Value::from("unknown_evidence"), "{case}");

        reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
        intact(&machine, &home, &tip);
    }
}

/// The checkout read `origin` before the home pushed, so it has never seen the branch.
fn older(machine: &Machine) {
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
}

/// The checkout fetches one branch, so it cannot say that another is gone.
fn partial(machine: &Machine) {
    git(
        &machine.source,
        &["config", "remote.origin.fetch", "+refs/heads/main:refs/remotes/origin/main"],
    );
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
}

/// A witness vouches with its objects and never with a name. The checkout names the
/// home's commit on `origin/topic` and its object store does not hold it, which is the
/// shape a pruned store leaves. The preflight must not read the name as a copy.
#[test]
fn a_ref_with_no_object_behind_it_proves_nothing() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    let reference = machine.source.join(".git/refs/remotes/origin").join(TOPIC);
    std::fs::create_dir_all(reference.parent().unwrap()).unwrap();
    std::fs::write(&reference, format!("{tip}\n")).unwrap();
    assert!(
        !nodal_safety::try_git(&machine.source, &["cat-file", "-e", &tip]).status.success(),
        "the checkout holds the object, so this asserts nothing about a name"
    );

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "remote_proved")), 0, "a name proved a copy");
    assert_eq!(count(commits(&answer, "second_local_copy")), 0, "a name proved a copy");
    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// Leave the checkout a ref whose object it cannot read, and change nothing else.
///
/// `git for-each-ref` prints the object a ref names without opening it, so the refs
/// still list and the checkout still reads as a repository. `git rev-list` opens the
/// object and stops. That is the shape a half-written or pruned store has.
///
/// The commit is made with `commit-tree` from the tree `HEAD` already has, so the
/// working tree and the branch a person is on are untouched.
fn damage_a_ref(checkout: &Path) {
    let tree = git(checkout, &["rev-parse", "HEAD^{tree}"]);
    let made = git(checkout, &["commit-tree", &tree, "-m", "an object that will not read"]);
    git(checkout, &["update-ref", &format!("refs/heads/{DAMAGED}"), &made]);
    let (directory, file) = made.split_at(2);
    let object = checkout.join(".git/objects").join(directory).join(file);
    std::fs::set_permissions(&object, std::fs::Permissions::from_mode(WRITABLE)).unwrap();
    std::fs::write(&object, b"this is not a Git object").unwrap();
}

/// A reading that failed is not a reading that found nothing.
///
/// The set-up is `a_remote_that_is_this_disk_is_read_directly` exactly: the project has
/// no remote of its own, so the home's `origin` is the person's checkout, and reading
/// that checkout is reading the remote. That is what earns the settled words "only
/// here". The one difference is that the checkout cannot answer for its own object
/// store any more.
///
/// Every one of those words rests on a reading of a second object store. A `rev-list`
/// that did not run is not that reading, and treating its silence as an empty answer
/// puts a claim on the report that nothing earned: the reading says the checkout holds
/// none of the commits it names, which is the sentence that makes a home look like the
/// only copy. It is worse now that one reading is taken for a whole survey, because one
/// checkout Git cannot answer for would say it about every home under it.
///
/// So the failure is carried. The commits are `not_checked`, which is neither safe nor
/// a claim, and the reclaim refuses over the same home for the honest reason.
#[test]
fn a_checkout_whose_object_store_will_not_answer_proves_nothing() {
    let machine = Machine::new().with_env(ONLY_LOCAL).with_env(NO_PROXY);
    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    damage_a_ref(&machine.source);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 0, "a claim the reading did not earn");
    assert!(count(commits(&answer, "not_checked")) >= 1, "{answer:#}");
    let group = commits(&answer, "not_checked").unwrap();
    assert_eq!(group["copies"]["witness"]["kind"], Value::from("unchecked"), "{answer:#}");
    assert_eq!(answer["reasons"][0]["needs"], Value::from("unknown_evidence"), "{answer:#}");

    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has");
    intact(&machine, &home, &tip);
}

/// A project with no remote of its own is cloned from the person's checkout, so the
/// checkout is what `origin` names. Reading it is reading the remote, and what it does
/// not hold, the remote does not hold. The words "only here" are earned there.
#[test]
fn a_remote_that_is_this_disk_is_read_directly() {
    let machine = Machine::new().with_env(ONLY_LOCAL).with_env(NO_PROXY);
    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    let group = commits(&answer, "only_here").expect("the commit is only here");
    assert_eq!(group["count"], Value::from(1));
    let kind = &group["copies"]["witness"]["kind"];
    assert!(
        kind == &Value::from("direct") || kind == &Value::from("no_remote"),
        "the remote question was not settled: {kind}"
    );

    reclaim_also_refuses(&machine, SLUG, "commits on no remote (1)");
    intact(&machine, &home, &tip);
}

/// A squash merge leaves the content on `main` and the commit objects in this home and
/// nowhere else. The content is safe; the objects are not, and the objects are what a
/// removal takes. So the answer is a keep, and it says which of the two it is about.
///
/// This is the case a reader is most likely to think is finished, which is why it is a
/// test: the branch reads as merged everywhere a person looks, and removing its home
/// still loses something no other tree on this machine holds.
///
/// The size of the loss is not priced: there is no portable way to say what a set of
/// commit objects holds that nothing else does, so the report says how many commits and
/// leaves the bytes alone rather than printing a figure it cannot stand behind.
#[test]
fn squash_absorbed_work_whose_objects_are_only_here_is_a_keep() {
    let machine = machine();
    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), "the content that was absorbed\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work the reviewer squashed"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);

    // The reviewer's squash: the same content, a different commit, on main.
    std::fs::write(machine.source.join(ONLY), "the content that was absorbed\n").unwrap();
    git(&machine.source, &["add", "--all"]);
    git(&machine.source, &["commit", "--quiet", "--message", "squashed on main"]);
    git(&machine.source, &["push", "--quiet", "origin", "main"]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    assert!(
        !nodal_safety::try_git(&machine.source, &["cat-file", "-e", &tip]).status.success(),
        "the checkout holds the home's commit, so nothing here is squash-absorbed"
    );

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "absorbed content made it safe");
    assert_eq!(count(commits(&answer, "only_here")), 1, "{answer:#}");
    assert!(
        commits(&answer, "only_here").unwrap()["bytes"].is_null(),
        "a commit group priced bytes it cannot know"
    );
    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// What a tool writes again is reconstructable, and it is the same classification the
/// trash prune acts on. The figure is apparent bytes and the value says why that is not
/// what the disk gives back.
#[test]
fn heavy_ignored_state_is_reconstructable_and_priced_as_apparent() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let git_dir = git(&home, &["rev-parse", "--git-dir"]);
    let exclude = home.join(git_dir.trim()).join("info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    let mut rules = std::fs::read_to_string(&exclude).unwrap_or_default();
    rules.push_str("target/\n.env.local\n");
    std::fs::write(&exclude, rules).unwrap();
    std::fs::create_dir_all(home.join("target/debug")).unwrap();
    std::fs::write(home.join("target/debug/app"), vec![0_u8; 4 * 1024 * 1024]).unwrap();
    std::fs::write(home.join(".env.local"), "TOKEN=only-in-this-home\n").unwrap();

    let answer = check(&machine, SLUG);
    let generated = paths(&answer, "generated").expect("the build output is classified");
    assert_eq!(generated["disposition"], Value::from("reconstructable"));
    assert!(generated["bytes"]["apparent"].as_u64().unwrap() >= 4 * 1024 * 1024, "{answer:#}");
    assert!(
        generated["bytes"]["exclusive_unknown"].as_str().is_some_and(|why| !why.is_empty()),
        "a size was printed with no word about what it is not"
    );
    let local = paths(&answer, "local_state").expect("the local state is classified");
    assert_eq!(local["disposition"], Value::from("must_survive"));
    assert_eq!(local["sample"][0], Value::from(".env.local"));

    // Neither is a reason to refuse: the trash keeps the one and no tool needs the other.
    assert_safe(&answer);
    assert!(home.join("target/debug/app").exists(), "the check removed the build output");
    assert!(home.join(".env.local").exists(), "the check removed local state");
}

/// A process carrying the unit's identifier is the unit's to stop. It is named as
/// something a reclaim would stop, it is not a reason to refuse, and reading it is not
/// signalling it.
#[test]
fn owned_runtime_is_named_and_is_not_a_reason_to_refuse() {
    let (machine, home, owned) = attributed_unit();

    let answer = check(&machine, SLUG);
    let processes = answer["runtime"]["processes"].as_array().unwrap();
    assert!(
        processes.iter().any(|pid| *pid == owned.pid()),
        "the process carrying the unit's id is not named: {answer:#}"
    );
    assert!(answer["runtime"]["bystanders"].as_array().unwrap().is_empty(), "{answer:#}");
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "owned runtime blocked: {answer:#}");
    assert!(nodal_safety::process::alive(owned.pid()), "the check signalled what it read");
    assert!(home.is_dir(), "the check moved the home");
}

/// A process matched by its working directory alone is a tmux pane or a teammate's
/// shell. It is named, it is never signalled, and it is exactly what a reclaim would
/// refuse to move the home out from under.
///
/// The comparison with the real `nodal reclaim` is made last, and that is not tidiness.
/// The teardown runs before the move, so a reclaim that refuses over a bystander has
/// already stopped what carries the unit's identifier (`docs/contracts.md`, Reclaim).
/// Every reading it could disturb is taken before it.
#[test]
fn a_bystander_blocks_the_move_and_the_reclaim_refuses_the_same_way() {
    let (machine, home, _owned) = attributed_unit();
    let bystander = nodal_safety::process::standing_in(&home);

    let answer = check(&machine, SLUG);
    let standing = answer["runtime"]["bystanders"].as_array().unwrap();
    assert!(
        standing.iter().any(|row| row["pid"] == bystander.pid()),
        "the bystander is not named: {answer:#}"
    );
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "a bystander did not block");
    assert_eq!(answer["reasons"][0]["needs"], Value::from("blocking_runtime"), "{answer:#}");
    assert!(nodal_safety::process::alive(bystander.pid()), "the check signalled a bystander");

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a bystander did not refuse the reclaim");
    let told = stderr(&refused);
    assert!(told.contains("still has work standing in its home"), "{told}");
    assert!(told.contains(&bystander.pid().to_string()), "the refusal does not name it: {told}");
    assert!(home.is_dir(), "the refusal moved the home");
    assert!(machine.trashed().is_empty(), "the refusal trashed the home");
}

/// A process of **another** unit standing in this unit's home blocks it in both
/// readings, and that is one predicate rather than two that agree by luck.
///
/// The process carries a `NODAL_ID`, so it is something Nodal started; the identifier is
/// not this unit's, so a reclaim here will never signal it and will move the home out
/// from under it. `nodal reclaim --check` refused over it and `nodal ls` said nothing,
/// which is the list and the preflight printing two different answers about one process
/// — against the rule the list's own module states, that one word means one thing in
/// both places.
///
/// Both sides are read from real commands against a real process, because the property is
/// about two readings of one machine and a table a test wrote is only one of them.
#[test]
fn another_units_process_in_this_home_blocks_the_list_and_the_check_alike() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let other = machine.unit(NEIGHBOUR);
    let their_id = std::fs::read_to_string(other.join(".nodal/id")).unwrap();
    let intruder = nodal_safety::process::of_another_unit(their_id.trim(), &other, &home);

    let answer = check(&machine, SLUG);
    let standing = answer["runtime"]["bystanders"].as_array().unwrap();
    assert!(
        standing.iter().any(|row| row["pid"] == intruder.pid()),
        "the check did not name another unit's process as a bystander: {answer:#}"
    );
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "it did not block: {answer:#}");

    assert_eq!(needs(&machine, SLUG), "blocking_runtime", "the list did not agree with the check");
    assert_ne!(
        needs(&machine, NEIGHBOUR),
        "blocking_runtime",
        "the process blocked the unit it belongs to, which it is standing nowhere near"
    );
    assert!(nodal_safety::process::alive(intruder.pid()), "a reading signalled the process");
}

/// A process this account cannot read, started from inside the home, blocks the move.
///
/// **Asserted at the `Processes` seam, and the name says so.** The shape is a process
/// whose `/proc/<pid>` this account may not read — another account's on a shared host, or
/// this account's own under a binary the kernel marks undumpable, which is what a setuid
/// program becomes. Nothing unprivileged can make a machine hold one on demand: a test
/// cannot become another account, and a setuid binary that blocks forever on a readable
/// input is not a thing a suite may rely on existing. So the table is stated and the arm
/// that judges it is what is pinned. `nodal-core/tests/attribution.rs` holds the other
/// half — that the live scan really does keep such a process rather than drop it.
///
/// `linux::read` used to return `None` for exactly this process, so it left the
/// table with no row, no note and nothing for a verdict to rest on, and a home with a
/// stranger's command standing in it read as safe.
///
/// What refuses is the lineage. `/proc/<pid>/stat` stays world-readable when the rest of
/// the directory does not, so a process whose parent, group or session reaches something
/// already found in the home is a command started from inside the home, and the move
/// refuses to go out from under it.
#[test]
fn an_unreadable_process_whose_lineage_reaches_the_home_blocks_the_move_at_the_seam() {
    let home = PathBuf::from("/homes/one");
    let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let shell = Running::new(21, BTreeMap::new()).running("bash").in_directory(&home);
    let hidden = Running::new(22, BTreeMap::new())
        .running("passwd")
        .from(Lineage { parent: Some(21), group: Some(21), session: Some(21) })
        .withholding(Withheld::AnotherAccount);

    let (_, standing) =
        sort(&[shell, hidden], Own::of(unit, &[]), std::slice::from_ref(&home), &[]);
    let blocked: Vec<u32> = standing.iter().map(|row| row.pid).collect();
    assert_eq!(blocked, [21, 22], "the process nobody can read did not block: {standing:?}");
    let named = standing.iter().find(|row| row.pid == 22).unwrap();
    assert!(
        named.holding.as_deref().is_some_and(|why| why.contains("may not read")),
        "the refusal must say why it cannot say more: {named:?}"
    );
}

/// And a process this account cannot read that has nothing to do with this home refuses
/// nothing — it is counted instead.
///
/// The other half of the rule, and the reason the first half can ship. Unreadability alone
/// is not a refusal: on the machine this was written on, 36 processes are withheld at any
/// moment and 3 of them are this account's own, every one of them there from boot to
/// shutdown. A rule that refused over unreadability would refuse every reclaim on this
/// host for ever, with nothing a person could do to clear it, and a rule nobody can
/// satisfy is a rule people turn off.
///
/// "Cannot see, therefore not safe" is a claim about *this home*. What the reading cannot
/// close it counts, in the evidence record, where a person or a test can see the number
/// and disagree with the verdict — which `evidence_record.rs` holds.
#[test]
fn an_unreadable_process_unrelated_to_the_home_is_counted_and_refuses_nothing_at_the_seam() {
    let home = PathBuf::from("/homes/one");
    let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let shell = Running::new(21, BTreeMap::new()).running("bash").in_directory(&home);
    let elsewhere = Running::new(99, BTreeMap::new())
        .running("systemd")
        .from(Lineage { parent: Some(1), group: Some(1), session: Some(1) })
        .withholding(Withheld::AnotherAccount);

    let (_, standing) =
        sort(&[shell, elsewhere], Own::of(unit, &[]), std::slice::from_ref(&home), &[]);
    let blocked: Vec<u32> = standing.iter().map(|row| row.pid).collect();
    assert_eq!(blocked, [21], "an unrelated hidden process refused a reclaim: {standing:?}");
}

/// What one row of `nodal ls` says it needs, and `absent` where it says nothing.
fn needs(machine: &Machine, slug: &str) -> String {
    let listed = machine.nodal(&["ls", "--json"]);
    let printed = answer(&listed);
    let read: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("ls --json is one document: {printed}"));
    read["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == slug)
        .unwrap_or_else(|| panic!("no row for {slug}: {printed}"))["needs"]
        .as_str()
        .unwrap_or("absent")
        .to_owned()
}

/// A unit with one process carrying its identifier.
fn attributed_unit() -> (Machine, PathBuf, nodal_safety::process::Owned) {
    let machine = machine();
    let home = machine.unit(SLUG);
    let id = std::fs::read_to_string(home.join(".nodal/id")).unwrap();
    let owned = nodal_safety::process::carrying(id.trim(), &home);
    (machine, home, owned)
}

/// The whole of the read-only promise, asserted on a machine that has something to lose.
///
/// The registry is read as rows rather than as bytes, for the reason
/// `tests/doctor_writes_nothing.rs` states: every command that opens it finishes an
/// interrupted operation first, and SQLite writes its own sidecars on every open. The
/// rows are what a command would have changed, and they are the same rows.
///
/// The home's **index** is watched byte for byte, and the home is left in the one state
/// that would move it ([`stale_index`]). A `git status` that finds the index's cached
/// stat information out of date refreshes it and writes the file back, which would be a
/// read command changing the file the person's own `git` is about to use. What stops it
/// is `GIT_OPTIONAL_LOCKS=0`, which [`nodal_core::git`] sets on every invocation it
/// makes and which Git documents as equivalent to `--no-optional-locks`. That is a
/// property of the one spawn seam rather than of this command, and this is where the
/// preflight's share of it is held: drop the variable and these bytes move.
///
/// The hook is the other half. The project declares a `pre_reclaim` that would write a
/// file, and this machine has never approved it — so a command that reached the hook
/// would be refused. The check answers instead, and no file appears.
#[test]
fn the_check_changes_nothing_and_runs_no_hook() {
    let machine = machine();
    let marker = machine.source.join("the-hook-ran");
    declare_unapproved_hook(&machine, &marker);
    let (home, tip) = pushed(&machine, SLUG);
    std::fs::write(home.join(ONLY), "and something uncommitted beside it\n").unwrap();
    stale_index(&home);

    let checkout = untouched(&machine.source);
    let before = untouched(&home);
    let state = Snapshot::of_except(&machine.state, is_registry);
    let rows = rows(&machine);
    let operations = journal::recent(machine.store().conn(), 100).unwrap().len();

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");

    checkout.assert_unchanged(&untouched(&machine.source), "the check wrote in the checkout");
    before.assert_unchanged(&untouched(&home), "the check wrote in the home");
    state.assert_unchanged(&Snapshot::of_except(&machine.state, is_registry), "the state moved");
    assert_eq!(rows, self::rows(&machine), "the check wrote a registry row");
    assert_eq!(
        journal::recent(machine.store().conn(), 100).unwrap().len(),
        operations,
        "the check opened an operation"
    );
    assert!(!marker.exists(), "the check ran the project's hook");
    assert!(machine.trashed().is_empty(), "the check trashed something");
    intact(&machine, &home, &tip);
}

/// Leave the home's index out of date with the working tree, without changing a byte of
/// content.
///
/// This is the state a `git status` writes in. The index caches each tracked file's stat
/// information, and a status that finds the cache out of date refreshes it and writes
/// the file back — an ordinary read command changing the file the person's own `git` is
/// about to use. Rewriting a tracked file with the bytes it already holds moves its
/// modification time and nothing else, which is exactly the condition.
///
/// Nothing here runs `git`. A `git status` of any kind would refresh the cache itself and
/// leave the assertion with nothing to catch.
fn stale_index(home: &Path) {
    let tracked = home.join(TRACKED);
    let file = std::fs::File::options().write(true).open(&tracked).unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    file.set_times(std::fs::FileTimes::new().set_modified(long_ago)).unwrap();
    file.sync_all().unwrap();
}

/// Declare a `pre_reclaim` this machine has not approved, so that reaching it refuses.
fn declare_unapproved_hook(machine: &Machine, marker: &Path) {
    let path = machine.source.join(nodal_fixture::RECIPE);
    let recipe = std::fs::read_to_string(&path).unwrap();
    let line = format!("pre_reclaim = 'touch {}'", marker.display());
    std::fs::write(&path, format!("{recipe}\n[hooks]\n{line}\n")).unwrap();
    git(&machine.source, &["add", "--", nodal_fixture::RECIPE]);
    git(&machine.source, &["commit", "--quiet", "--message", "declare a hook"]);
}

/// Whether a path under the state directory is the registry, which every command writes.
fn is_registry(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("registry.db"))
}

/// Every row of the registry the check could touch, as text.
fn rows(machine: &Machine) -> String {
    let store = machine.store();
    let mut lines = Vec::new();
    for project in projects::list(store.conn()).unwrap() {
        lines.push(format!("{} {}", project.id, project.name));
        for unit in units::list(store.conn(), project.id).unwrap() {
            lines.push(format!("  {} {} {:?}", unit.id, unit.slug, unit.status));
            for row in environments::list_for_unit(store.conn(), unit.id).unwrap() {
                lines.push(format!("    {} {} {:?}", row.id, row.home.display(), row.state));
            }
        }
    }
    lines.join("\n")
}

/// The two renderings are one value. What a person reads and what a script gates on are
/// the same verdict, over the same groups.
#[test]
fn the_human_form_and_the_json_are_one_value() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);

    let printed = machine.nodal(&["reclaim", SLUG, "--check"]);
    assert!(!printed.status.success(), "the verdict and the exit code disagree");
    let text = answer(&printed);
    assert!(text.contains("refuse — a reclaim would stop"), "{text}");
    assert!(text.contains("not checked (1)"), "{text}");
    assert!(text.contains("this command changed nothing"), "{text}");
    assert!(text.contains(&tip[..8]), "the answer does not name the commit: {text}");

    let answer = check(&machine, SLUG);
    assert_eq!(count(commits(&answer, "not_checked")), 1, "{answer:#}");

    reclaim_also_refuses(&machine, SLUG, "commits no current reading proves a remote has (1)");
    intact(&machine, &home, &tip);
}

/// The preflight is not a way to do anything, so it refuses the two flags that are.
#[test]
fn check_refuses_force_and_yes() {
    let machine = machine();
    let (home, tip) = stranded(&machine, SLUG);
    for flag in ["--force", "--yes"] {
        let refused = machine.nodal(&["reclaim", SLUG, "--check", flag]);
        assert!(!refused.status.success(), "--check accepted {flag}");
        assert!(stderr(&refused).contains("cannot be used with"), "{}", stderr(&refused));
    }
    intact(&machine, &home, &tip);
}
