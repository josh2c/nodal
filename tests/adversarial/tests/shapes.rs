//! The full shape set: every defect closed on the way to rc.4, reproduced and asked of both
//! answerers.
//!
//! The grid draws from a space. These are the shapes a person already found, and they are
//! named tests that always run, because a shape that is only in the sample is a shape a
//! change to the seed can release. Each one reproduces the defect's own setup, asks the
//! predicate, asks the oracle, and asserts that the two agree.
//!
//! | shape | what it was | test |
//! |---|---|---|
//! | FS-1, order (a) | the remote dropped the branch and the witness then pruned | `a_dropped_branch_the_witness_pruned_is_refused` |
//! | FS-1, order (b) | the witness fetched without pruning and the remote then dropped | `a_prune_less_fetch_before_a_dropped_branch_stays_safe` |
//! | FS-2, branch | a commit on a branch `HEAD` does not reach | `work_on_a_side_branch_is_refused` |
//! | FS-2, stash | a stash, and a clone of the home that fetched none of it | `work_in_the_stash_is_refused` |
//! | FS-2, detached | a commit on a detached `HEAD` | `work_on_a_detached_head_is_refused` |
//! | FS-2, wip | a record under `refs/nodal/` | `a_record_under_the_nodal_namespace_is_not_the_homes_own_work` |
//! | FS-3, the file | `packed-refs` rewritten with no fetch | `a_rewrite_of_packed_refs_moves_no_verdict` |
//! | FS-3, the clock | a branch moved on to a commit made years ago | `a_branch_moved_on_to_an_older_commit_is_dated_by_the_move` |
//! | FS-6 | a process of this account it may not read | `a_process_this_account_may_not_read_refuses_the_move` |
//! | FS-8 | a write from a directory elsewhere | `a_write_from_a_directory_elsewhere_refuses_the_move` |
//! | FS-14 | a blobless partial clone offered as the copy | `a_blobless_clone_is_no_copy_of_the_work` |
//! | FS-15 | doctor's own predicate | `doctor_and_the_gate_give_one_verdict` |
//! | DL-069 | the copy a trashed home rested on went | `a_trashed_home_whose_copy_went_is_kept` |
//! | DL-072 | a lock holder this account may not read | `a_hold_whose_holder_cannot_be_read_is_kept` |
//! | the server remote | `origin` is an `ssh` or an `https` URL and not a path | `a_witness_of_a_served_remote_is_read_through_the_witness_path` |
//! | the trash record | a `rested` record nothing can parse | `a_trash_record_nothing_can_parse_is_read_again` |
//!
//! Two of them state a defect and not a fix, because this lane writes no product code. Each
//! says so in its own note, and each fails on the day the defect closes, which is how whoever
//! closes it finds the test to turn round.
//!
//! | open defect | test |
//! |---|---|
//! | a `refs/nodal/` record can be the only holder of content | `a_record_under_the_nodal_namespace_is_not_the_homes_own_work` |
//! | the sweep reads `HEAD` where the gate reads every ref | `a_side_branch_whose_copy_went_is_swept_with_no_reading_of_it` |
//!
//! Every test that reads the process table names both hosts: the reading on Linux, and the
//! claim not made on macOS with the reason printed.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_adversarial::build::{self, Built, SLUG};
use nodal_adversarial::check::{Check, Copies, Needs};
use nodal_adversarial::compare;
use nodal_adversarial::oracle::Answer;
use nodal_adversarial::shape::{Observed, Occupant, Refs, Shape, Tree, Witness};
use nodal_safety::{InState as _, Machine, git, platform, stderr, stdout};
use serde_json::Value;

/// A shape, by its five values, with the words in axis order.
fn shape(
    witness: Witness,
    refs: Refs,
    observed: Observed,
    tree: Tree,
    occupant: Occupant,
) -> Shape {
    Shape { witness, refs, observed, tree, occupant }
}

/// Build one shape and ask both answerers, insisting that they agree.
///
/// Every named case goes through here, so no named case can assert a verdict and leave the
/// oracle unasked. The agreement is the load-bearing half: a test that asserted only what
/// Nodal said would pass on the day Nodal and the contract both moved the wrong way.
fn asked(shape: Shape) -> (Built, Check, Answer) {
    let built = build::build(shape);
    let check = built.check();
    let oracle = built.oracle();
    if let Some(found) = compare::compare(shape, &built, &check, &oracle) {
        assert!(!found.fails(), "{found}");
    }
    (built, check, oracle)
}

/// Insist that the predicate refuses, and that the oracle says work would go with the home.
fn both_refuse(check: &Check, oracle: &Answer, claim: &str) {
    assert!(!check.safe_to_reclaim, "{claim}: nodal called it safe");
    assert!(oracle.loses(), "{claim}: the oracle found a copy of everything, so it proves nothing");
}

/// Insist that both answerers call the home safe to remove.
fn both_agree_it_is_safe(check: &Check, oracle: &Answer, claim: &str) {
    assert!(!oracle.loses(), "{claim}: the oracle says work goes with the home");
    assert!(check.safe_to_reclaim, "{claim}: nodal refused");
}

/// The disposition of the one commit group a reading found, when it found exactly one.
fn one_group(check: &Check) -> Copies {
    assert_eq!(check.commits.len(), 1, "one group was expected: {:?}", check.commits);
    check.commits[0].copies
}

// ---------------------------------------------------------------------------
// FS-1 — the two orderings of a dropped branch.
// ---------------------------------------------------------------------------

/// The remote dropped the branch and the witness then pruned, so nothing names the work.
///
/// This is the ordering a local reading can close, and it closes it: `FETCH_HEAD` no longer
/// lists the branch, the tracking ref is gone, and no store on the disk reaches the commit.
#[test]
fn a_dropped_branch_the_witness_pruned_is_refused() {
    let (built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::Pruned,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_refuse(&check, &oracle, "a branch the remote dropped and the witness pruned");
    assert!(built.home.is_dir(), "the check moved the home");
}

/// The witness fetched without pruning and the remote then dropped the branch. Safe, by rule.
///
/// DL-073 and the founder's ruling of 2026-09-24 settle this one. The record still names the
/// branch at a sha that reaches the work, dated after the push, and that is a true dated
/// observation: at that instant the remote reported that state. Nodal never claims the remote
/// is correct now, and no local reading can tell a fetch that pruned from one that did not.
///
/// The oracle agrees for the same reason, off the same record and none of the same code.
#[test]
fn a_prune_less_fetch_before_a_dropped_branch_stays_safe() {
    let (_built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::DroppedAfterFetch,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_agree_it_is_safe(&check, &oracle, "a prune-less fetch before a dropped branch");
    assert_eq!(one_group(&check), Copies::RemoteProved, "{:?}", check.commits);
}

// ---------------------------------------------------------------------------
// FS-2 — work under a ref `HEAD` does not reach.
// ---------------------------------------------------------------------------

/// A commit on a branch the home is not checked out on is work the home holds.
#[test]
fn work_on_a_side_branch_is_refused() {
    let (_built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_refuse(&check, &oracle, "a commit on a side branch");
    // Not `only_here`: nothing on this machine read the remote in this shape, and a reading
    // nobody took is not a fact about the remote. The disposition says so, and both of the
    // two that are not proofs refuse.
    assert!(!one_group(&check).proves(), "{:?}", check.commits);
}

/// A stash is work the home holds, and a clone of the home is no copy of it.
///
/// The clone is the load-bearing half. `git clone` fetches `refs/heads/*` and the tags, and no
/// stash — so a second clone of your own repository, which is the store a person would point at
/// and say "it is in there", holds none of it.
///
/// The shape asserted nothing for a while, and the reason is worth keeping: the fixture cloned
/// the home by its **path**, which is the local transport, which copies the whole object
/// database. The stash commit was in that clone, `git branch adversarial-copy <it>` succeeded,
/// and both answerers found a copy that no real clone would hold. Over the four clone-shaped
/// topologies the stash, the note and the `wip` values of the [`Refs`] axis could then never
/// report a loss. The clones fetch now, and the last assertion here is the one that says so.
#[test]
fn work_in_the_stash_is_refused() {
    let (built, check, oracle) = asked(shape(
        Witness::FullClone,
        Refs::Stash,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_refuse(&check, &oracle, "a stash, with a clone of the home beside the checkout");
    assert!(!oracle.unproved.is_empty(), "the oracle named no commit, so the tree refused it");
    let store = built.root.join("witness");
    assert!(store.is_dir(), "the shape built no clone, so it asserts nothing about one");
    let stash = git(&built.home, &["rev-parse", "refs/stash"]);
    assert!(
        !holds(&store, stash.trim()),
        "the clone fetched the stash, so the copy it offers is the local transport and not a fetch"
    );
}

/// A commit made with `HEAD` detached is work the home holds.
#[test]
fn work_on_a_detached_head_is_refused() {
    let (_built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::DetachedHead,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_refuse(&check, &oracle, "a commit on a detached HEAD");
}

/// A record under `refs/nodal/` is not the home's own work, and both answerers say so.
///
/// **This test documents a difference and does not close it.** Nodal leaves `refs/nodal/`
/// out of the assessed set, for the reason `tests/safety/tests/home_refs.rs` gives: a record's
/// tree is the home's working tree and its parent is the home's own branch, so counting it
/// would keep every home that has ever run a command, for ever. The oracle leaves it out with
/// Nodal, so the two agree here.
///
/// §1 of the safety contract does not. Its table puts `refs/nodal/*` — `wip` and `premerge` —
/// inside the guarantee. This shape builds the case where the two readings come apart: the
/// record is the only holder of its content, the working tree holds none of it, and both
/// answerers call the home safe to remove. What is asserted is what the code and the oracle
/// do today. Which of the two readings is right is for the contract text to settle, and the
/// lane's report says so.
#[test]
fn a_record_under_the_nodal_namespace_is_not_the_homes_own_work() {
    let (built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::Wip,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_agree_it_is_safe(&check, &oracle, "a wip record holding content nothing else holds");
    let records = git(&built.home, &["for-each-ref", "--format=%(refname)", "refs/nodal/**"]);
    assert!(records.contains("/wip"), "the shape wrote no record: {records}");
    assert!(check.commits.is_empty(), "the record was counted after all: {:?}", check.commits);
}

// ---------------------------------------------------------------------------
// FS-3 — `packed-refs` is not a reading of a remote.
// ---------------------------------------------------------------------------

/// A rewrite of `packed-refs` with no fetch moves no verdict.
///
/// `git gc`, `git pack-refs` and `git maintenance` rewrite that file, and Git runs the first
/// of those on its own after many commands. A freshness rule that read its modification time
/// as the time of a fetch would therefore believe a stale remote-tracking ref after a command
/// that touched no remote.
///
/// The property is stated as a comparison and not as a verdict, because that is what it is:
/// two machines built the same way, one of them packed and dated far into the future, and the
/// two answers are the same answer.
#[test]
fn a_rewrite_of_packed_refs_moves_no_verdict() {
    let asking = shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::FetchedBefore,
        Tree::Clean,
        Occupant::Nothing,
    );
    let (plain, first, _) = asked(asking);
    let (packed, second, _) = asked(asking);
    git(&packed.machine.source, &["pack-refs", "--all"]);
    dated_far_ahead(&packed.machine.source.join(".git/packed-refs"));
    let after = packed.check();

    assert_eq!(first.safe_to_reclaim, second.safe_to_reclaim, "the two machines differ at birth");
    assert_eq!(
        second.safe_to_reclaim, after.safe_to_reclaim,
        "packing the refs of the checkout moved the verdict"
    );
    assert_eq!(needs(&second), needs(&after), "packing the refs of the checkout moved the reasons");
    assert!(plain.home.is_dir() && packed.home.is_dir());
}

/// The reasons of one answer, as the words alone, so two machines can be compared.
fn needs(check: &Check) -> Vec<Needs> {
    check.reasons.iter().map(|reason| reason.needs).collect()
}

/// Date a file a long way into the future, which is what a rewrite by a later command looks
/// like to a reading that takes modification times.
fn dated_far_ahead(path: &Path) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(86_400);
    let handle = std::fs::File::options().write(true).open(path).expect("the file is there");
    handle.set_times(std::fs::FileTimes::new().set_modified(when)).expect("the file is dated");
}

/// Date a file behind now, which is what a reading taken before the last local change looks
/// like.
fn dated_seconds_ago(path: &Path, seconds: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
    let handle = std::fs::File::options().write(true).open(path).expect("the file is there");
    handle.set_times(std::fs::FileTimes::new().set_modified(when)).expect("the file is dated");
}

/// An instant long before any run of this suite: 13 September 2020, in whole seconds.
///
/// A fixed instant and not an offset from now, so the commit it dates is the same commit on
/// every host and in every year this suite runs in.
const LONG_AGO: &str = "1600000000 +0000";

/// A commit of the tree `HEAD` holds, dated [`LONG_AGO`], on top of `HEAD`.
fn backdated(home: &Path) -> String {
    let tree = git(home, &["rev-parse", "HEAD^{tree}"]);
    let parent = git(home, &["rev-parse", "HEAD"]);
    let made = std::process::Command::new("git")
        .arg("-C")
        .arg(home)
        .args(["commit-tree", &tree, "-p", &parent, "-m", "a commit made long ago"])
        .env("GIT_AUTHOR_DATE", LONG_AGO)
        .env("GIT_COMMITTER_DATE", LONG_AGO)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs");
    assert!(made.status.success(), "the backdated commit was not made: {}", stderr(&made));
    String::from_utf8_lossy(&made.stdout).trim().to_owned()
}

/// A branch moved on to an older commit is dated by the move and not by the commit.
///
/// FS-3 in the direction that loses work. The freshness rule of §3 is an order between two
/// instants: a reading of a remote proves work only where the reading is **after** the last
/// local change. So the local instant has to be when the ref moved. A reading that took the
/// commit's own committer date instead would date a ref pointed at an older commit by the age
/// of that commit, a stale reading would stand ahead of it, and the reading would prove work
/// it never saw.
///
/// The shape is the ordinary one: a commit made years ago that the remote was seen to hold, a
/// reading of that remote taken ten minutes ago, and a branch of the home moved on to the
/// commit **after** the reading. Nothing here is strange; a rebase, a `reset --hard` on to a
/// release tag and a `branch --force` all do it.
///
/// Both halves are asserted, because the assertion is about the order of two instants and not
/// about reachability: the sha the reading carries reaches the commit, so a reading dated after
/// the move would prove it and the only thing that refuses it is the clock.
#[test]
fn a_branch_moved_on_to_an_older_commit_is_dated_by_the_move() {
    let (built, _, _) = asked(shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::FetchedBefore,
        Tree::Clean,
        Occupant::Nothing,
    ));
    let checkout = built.machine.source.clone();

    let older = backdated(&built.home);
    git(&built.home, &["push", "--quiet", "origin", &format!("{older}:refs/heads/older")]);
    git(&checkout, &["fetch", "--quiet", "origin"]);
    dated_seconds_ago(&checkout.join(".git/FETCH_HEAD"), 600);
    git(&built.home, &["branch", "moved-here", &older]);

    let read = std::fs::read_to_string(checkout.join(".git/FETCH_HEAD")).expect("a record");
    let mut seen = read.lines().filter_map(|line| line.split_whitespace().next());
    assert!(
        seen.any(|sha| sha == older),
        "the reading does not name the commit, so the clock is not what refuses it: {read}"
    );
    let made: u64 = git(&built.home, &["log", "-1", "--format=%ct", &older]).parse().unwrap();
    assert!(
        made < dated_at(&checkout.join(".git/FETCH_HEAD")),
        "the commit is not older than the reading, so this shape asserts nothing"
    );

    let again = built.oracle();
    assert!(
        again.unproved.iter().any(|lost| lost.oid == older),
        "a reading taken before the branch moved was called a proof of it: {:?}",
        again.unproved
    );
    let check = built.check();
    assert!(!check.safe_to_reclaim, "the predicate called the stale reading a proof: {check:?}");
}

/// When a file was last written, in seconds.
fn dated_at(path: &Path) -> u64 {
    let modified = std::fs::metadata(path).expect("the file is there").modified().expect("a time");
    modified.duration_since(std::time::UNIX_EPOCH).expect("an instant after the epoch").as_secs()
}

// ---------------------------------------------------------------------------
// FS-6 and FS-8 — occupancy.
// ---------------------------------------------------------------------------

/// A process of this account that this account may not read refuses the move.
///
/// The reading used to drop such a process altogether: neither a bystander nor a note, and the
/// home moved out from under it with no line. An unreadable process is now a reading that was
/// not made, and a reading that was not made refuses a managed move.
///
/// The process is made with `prctl(PR_SET_DUMPABLE, 0)`, which needs no second account. macOS
/// publishes no such attribute, so the claim is not made there and the reason is printed.
#[test]
fn a_process_this_account_may_not_read_refuses_the_move() {
    let (built, check, _) = asked(shape(
        Witness::FullClone,
        Refs::Branch,
        Observed::FetchedAfter,
        Tree::Clean,
        Occupant::Unreadable,
    ));
    if let Some(why) = built.skipped.clone() {
        assert!(platform::skipped("an unreadable process refuses a managed move", &why));
        return;
    }
    assert!(!check.safe_to_reclaim, "a process nothing could read was passed over: {check:?}");
    let table = check.runtime.as_ref().expect("the preflight read the runtime");
    assert!(table.withheld > 0, "the record says nothing was withheld: {table:?}");
}

/// A process writing into the home from a directory elsewhere refuses the move.
///
/// The ordinary shape of it: a dev server started from a terminal that has since changed
/// directory, writing its database into a git-ignored corner of the home. Its working
/// directory is `/`, it carries no identifier, and the file it writes is ignored — so no other
/// conjunct can see it at all.
#[test]
fn a_write_from_a_directory_elsewhere_refuses_the_move() {
    if cfg!(not(target_os = "linux"))
        && platform::skipped(
            "a write descriptor refuses a managed move",
            "this host publishes no per-descriptor open flags",
        )
    {
        return;
    }
    let (_built, check, _) = asked(shape(
        Witness::FullClone,
        Refs::Branch,
        Observed::FetchedAfter,
        Tree::IgnoredOnly,
        Occupant::WriteDescriptor,
    ));
    assert!(!check.safe_to_reclaim, "a writer standing nowhere was passed over: {check:?}");
    assert!(check.blocked_by_occupancy(), "it refused for some other reason: {:?}", check.reasons);
    let ignored = check.paths.iter().any(|group| !group.held.in_the_loss_set());
    assert!(ignored, "the file it writes is not ignored, so this shape proves nothing");
}

// ---------------------------------------------------------------------------
// FS-14 — a store that holds the commit and not the work.
// ---------------------------------------------------------------------------

/// A blobless partial clone answers every reachability question and holds no content.
///
/// It was the worst of the family, because it answered the old reading perfectly: Nodal called
/// the home safe and named that directory as the proof, and the directory then answered
/// `fatal: bad object <sha>:only-here.txt`.
#[test]
fn a_blobless_clone_is_no_copy_of_the_work() {
    let (built, check, oracle) = asked(shape(
        Witness::BloblessPartial,
        Refs::Branch,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_refuse(&check, &oracle, "a blobless clone offered as the copy");
    assert_eq!(one_group(&check), Copies::NotChecked, "{:?}", check.commits);
    let witness = built.root.join("witness");
    assert!(witness.is_dir(), "the shape built no partial clone");
    let detail = check.reasons.first().map(|reason| reason.detail.clone()).unwrap_or_default();
    assert!(detail.contains("partial"), "the reason does not say what failed: {detail}");
}

// ---------------------------------------------------------------------------
// FS-15 — one predicate, whichever surface asks it.
// ---------------------------------------------------------------------------

/// Doctor and the gate give one verdict about one home.
///
/// Doctor had a predicate of its own, `rev-list --not --remotes`, which the gate exists to
/// replace: on the sequence push, merge, remote delete, no prune it printed that no branch
/// held commits the remote had not got, about a branch holding exactly that. Doctor removes
/// nothing, but the founding questions call it the wedge, so the most-read surface gave the
/// weak answer and a person deleted by hand.
///
/// Both directions are asserted, because a surface that always says "unique" agrees with a
/// refusal and is no better than one that always says "safe".
#[test]
fn doctor_and_the_gate_give_one_verdict() {
    let refused =
        shape(Witness::Nothing, Refs::Branch, Observed::Pruned, Tree::Clean, Occupant::Nothing);
    let (built, check, _) = asked(refused);
    assert!(!check.safe_to_reclaim);
    assert!(named_by_doctor(&built.machine), "the gate refuses and doctor names nothing");

    let safe = shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::FetchedAfter,
        Tree::Clean,
        Occupant::Nothing,
    );
    let (whole, went, _) = asked(safe);
    assert!(went.safe_to_reclaim);
    assert!(!named_by_doctor(&whole.machine), "the gate is content and doctor names the unit");
}

/// Whether doctor's only-here survey names this project's one unit.
fn named_by_doctor(machine: &Machine) -> bool {
    let asked = machine.nodal(&["doctor", "--json"]);
    let printed = stdout(&asked);
    let report: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("doctor --json is one document: {printed}{}", stderr(&asked)));
    report["here"].as_array().is_some_and(|rows| {
        rows.iter().any(|row| {
            row["kind"] == "unique_work"
                && row["what"].as_str().is_some_and(|what| what.contains(SLUG))
        })
    })
}

// ---------------------------------------------------------------------------
// The served remote — the shape whose absence hid a regression.
// ---------------------------------------------------------------------------

/// A remote named by a server URL is read through the witness path, for both schemes.
///
/// A remote that is a directory on this disk is read **directly**: the refs under
/// `refs/heads/` of that directory are the remote's own refs, and the witness and freshness
/// rule is never reached. Every test that used a local path therefore asserted the short path,
/// and that is how a regression in the witness path went unseen.
///
/// `url.<path>.insteadOf` carries the transport back to the directory, so the reading is of a
/// served remote and nothing reaches a network.
#[test]
fn a_witness_of_a_served_remote_is_read_through_the_witness_path() {
    let (built, check, oracle) = asked(shape(
        Witness::Nothing,
        Refs::Branch,
        Observed::FetchedAfter,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_agree_it_is_safe(&check, &oracle, "work a served remote was seen to hold");
    assert_eq!(one_group(&check), Copies::RemoteProved, "{:?}", check.commits);
    let named = git(&built.home, &["config", "--get", "remote.origin.url"]);
    assert!(named.starts_with("ssh://"), "the remote is not a server: {named}");

    let over_https = built.machine.source.clone();
    let key = format!("url.{}.insteadOf", std::fs::canonicalize(&built.origin).unwrap().display());
    let https = "https://example.invalid/project.git";
    for repo in [&over_https, &built.home] {
        git(repo, &["config", "--local", &key, https]);
        git(repo, &["remote", "set-url", "origin", https]);
    }
    let again = built.check();
    assert!(again.safe_to_reclaim, "the same reading over https refused: {again:?}");
    assert_eq!(one_group(&again), Copies::RemoteProved, "{:?}", again.commits);
}

// ---------------------------------------------------------------------------
// DL-069 — the copy a trashed home rested on can go.
// ---------------------------------------------------------------------------

/// A reclaim that rested on a copy keeps its home when that copy goes.
///
/// The trash is the last line, and it used to be swept on a timer with no reading. A verdict
/// of "second local copy" rests on a ref in another repository, and that ref goes on ordinary
/// days: a branch deleted, a `fetch --prune`, a `gc`. The sweep now reads the home again and
/// keeps it over an open question, and the line says which question.
///
/// The copy is a clone with a branch on the work, because a branch is a ref the store keeps of
/// its own accord. What a clone puts under `refs/remotes/origin/` is its record of a fetch, and
/// `git fetch --prune` deletes one the moment the remote drops the branch, so it is not a copy.
///
/// The work is on the home's `HEAD` here, which is the one ref the sweep reads. The next test is
/// about what happens when it is not.
#[test]
fn a_trashed_home_whose_copy_went_is_kept() {
    let (built, check, oracle) = asked(shape(
        Witness::FullClone,
        Refs::DetachedHead,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_agree_it_is_safe(&check, &oracle, "work a clone beside the checkout names on a branch");
    assert_eq!(one_group(&check), Copies::SecondLocalCopy, "{:?}", check.commits);
    let work = check.commits[0].sample.first().cloned().expect("the group names its commits");

    keeps_nothing(&built.machine);
    let done = built.machine.nodal(&["reclaim", SLUG, "--yes"]);
    assert!(done.status.success(), "the reclaim refused: {}", stderr(&done));
    let trashed = built.machine.trashed();
    assert_eq!(trashed.len(), 1, "the trash holds the home that was reclaimed");

    let store = built.root.join("witness");
    took_the_copy_away(&store);
    assert!(!reaches(&store, &work), "the store still reaches the work, so this proves nothing");

    let swept = built.machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&work[..8]), "the sweep does not name the commit: {report}");
    assert!(trashed[0].is_dir(), "the sweep removed the home it rested on a copy that is gone");
    assert_eq!(built.machine.trashed(), trashed, "and the row went with it");
}

/// A `rested` record nothing can parse is read again, and never as a loss the person accepted.
///
/// Three answers and not two: a record that says the check found nothing only here, a record
/// that says `--force` went on over work only here, and a row that says nothing at all. The
/// third is the safe one, and a record this release cannot parse has to fall into it. Reading
/// an unparseable record as the second would remove a home on a timer on the strength of a
/// sentence nobody could read.
#[test]
fn a_trash_record_nothing_can_parse_is_read_again() {
    let (built, check, _) = asked(shape(
        Witness::FullClone,
        Refs::DetachedHead,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    assert!(check.safe_to_reclaim, "{check:?}");
    keeps_nothing(&built.machine);
    let done = built.machine.nodal(&["reclaim", SLUG, "--yes"]);
    assert!(done.status.success(), "the reclaim refused: {}", stderr(&done));
    let trashed = built.machine.trashed();

    unparseable(&built.machine);
    took_the_copy_away(&built.root.join("witness"));

    let swept = built.machine.nodal(&["gc"]);
    assert!(
        swept.status.success(),
        "a record nobody could read stopped the sweep: {}",
        stderr(&swept)
    );
    assert!(trashed[0].is_dir(), "a record nobody could read was read as a loss a person accepted");
    assert_eq!(built.machine.trashed(), trashed, "and the row went with it");
}

/// **An open false-safe, found by this lane. The assertion is what happens today.**
///
/// Work on a side branch, a copy in another store at reclaim time, the copy gone while the home
/// sits in the trash: the sweep removes the home and the work is then nowhere on the machine.
/// Reproduced end to end by this test, which reads every store afterwards and finds none of them
/// holds the commit.
///
/// Where the two readings come apart:
///
/// | reading | what it walks |
/// |---|---|
/// | the reclaim gate | every ref the home holds, less `refs/remotes/` and `refs/nodal/`, plus `HEAD` |
/// | the sweep (`lifecycle::ops::gc::work_tips`) | `HEAD` and the `wip` snapshot, and nothing else |
///
/// The sweep's reading was right when the gate read `HEAD` alone: a commit the gate never looked
/// at could not be one the gate had rested a removal on. The gate reads every ref now
/// (`tests/safety/tests/home_refs.rs`), and the sweep was not widened with it — so DL-069's
/// promise, that `gc` never removes the only copy, holds for `HEAD` and for nothing else.
///
/// This lane writes no product code, so the test states the defect rather than the fix. **When
/// the sweep reads the refs the gate reads, this test fails**, and whoever closes it turns the
/// three assertions at the end into their opposites and renames it
/// `a_side_branch_whose_copy_went_is_kept`.
#[test]
fn a_side_branch_whose_copy_went_is_swept_with_no_reading_of_it() {
    let (built, check, oracle) = asked(shape(
        Witness::FullClone,
        Refs::Branch,
        Observed::NeverPushed,
        Tree::Clean,
        Occupant::Nothing,
    ));
    both_agree_it_is_safe(&check, &oracle, "work a clone beside the checkout names on a branch");
    let work = check.commits[0].sample.first().cloned().expect("the group names its commits");

    keeps_nothing(&built.machine);
    let done = built.machine.nodal(&["reclaim", SLUG, "--yes"]);
    assert!(done.status.success(), "the reclaim refused: {}", stderr(&done));
    let trashed = built.machine.trashed();
    assert_eq!(trashed.len(), 1);

    let store = built.root.join("witness");
    took_the_copy_away(&store);
    git(&store, &["update-ref", "-d", "refs/remotes/origin/side-work"]);
    git(&store, &["gc", "--prune=now", "--quiet"]);
    assert!(!holds(&store, &work), "the copy is still there, so this proves nothing");

    let swept = built.machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    assert!(!trashed[0].is_dir(), "the sweep kept it: the defect is closed, see this test's note");
    assert!(built.machine.trashed().is_empty(), "the row was kept, see this test's note");
    for store in [built.machine.source.clone(), store, built.origin.clone()] {
        assert!(!holds(&store, &work), "{} still holds the work", store.display());
    }
}

/// Whether a repository has an object at all, without letting Git fetch one.
fn holds(store: &Path, oid: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(store)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["cat-file", "-e", oid])
        .status()
        .expect("git runs")
        .success()
}

/// Let the project's trash keep nothing, so the next sweep is the one that decides.
///
/// A fortnight of waiting is no part of either property here. What both are about is what the
/// sweep reads when it does run.
fn keeps_nothing(machine: &Machine) {
    let recipe = machine.source.join("nodal.toml");
    let written = std::fs::read_to_string(&recipe).expect("the project has a recipe");
    std::fs::write(&recipe, format!("{written}\n[reclaim]\ntrash_retention = 0\n"))
        .expect("the recipe is writable");
}

/// Take every ref of the store that reaches the work off it, and leave no reflog of them.
///
/// Ordinary commands in a repository Nodal never touched, which is the whole point of the rule:
/// the copy a verdict rests on is not the verdict's to keep.
///
/// The store's own `HEAD` is one of the refs, and it has to be moved rather than deleted. A clone
/// of a home whose `HEAD` was detached checks that commit out, so the clone's `HEAD` reaches the
/// work and is a copy of it — a real one, and the reading is right to count it. A test that
/// deleted the branch alone would take away one copy of two and assert nothing.
fn took_the_copy_away(store: &Path) {
    let elsewhere = git(store, &["rev-parse", "refs/remotes/origin/main"]);
    git(store, &["update-ref", "--no-deref", "HEAD", &elsewhere]);
    git(store, &["update-ref", "-d", "refs/heads/adversarial-copy"]);
    git(store, &["reflog", "expire", "--expire=now", "--all"]);
}

/// Whether a ref the store keeps of its own accord still reaches the work.
///
/// `refs/remotes/` is left out for the reason the reading leaves it out: a record of a fetch is
/// not a copy. So this asks the same question `crates/nodal-core/src/git/outside.rs` asks.
fn reaches(store: &Path, work: &str) -> bool {
    git(
        store,
        &[
            "rev-list",
            "--no-walk",
            "--ignore-missing",
            work,
            "--not",
            "--exclude=refs/remotes/*",
            "--all",
        ],
    )
    .trim()
    .is_empty()
        && nodal_safety::try_git(store, &["cat-file", "-e", work]).status.success()
}

/// Put a word in the trash row's `rested` column that no release of Nodal writes.
fn unparseable(machine: &Machine) {
    let store = machine.store();
    let changed = store
        .conn()
        .execute("UPDATE trash SET rested = ?1", ["{\"kind\":\"a word from a later release\"}"])
        .expect("the column is writable");
    assert_eq!(changed, 1, "the trash holds one row to rewrite");
}

// ---------------------------------------------------------------------------
// DL-072 — a hold is held unless a reading proves its holder gone.
// ---------------------------------------------------------------------------

/// A hold whose holder this account may not read keeps the unit held.
///
/// The reading that let this through called every holder it could not resolve gone, and handed
/// the home to the next actor while the first was still writing in it. A process of another
/// account is the ordinary shape of "cannot resolve": `kill(pid, 0)` answers `EPERM`, which is
/// the host saying the process is there and not mine.
///
/// The hold is written by hand, because a hold taken by a process of another account cannot be
/// taken by this test. `tests/safety/tests/lock_liveness.rs` states the same rule at the seam
/// over a table it writes; this one states it through the binary a person types.
#[test]
fn a_hold_whose_holder_cannot_be_read_is_kept() {
    let (built, _, _) = asked(shape(
        Witness::Bare,
        Refs::Branch,
        Observed::FetchedAfter,
        Tree::Clean,
        Occupant::Nothing,
    ));
    let Some(lineage) = another_accounts_session() else {
        assert!(platform::skipped(
            "a hold whose holder cannot be read keeps the unit",
            "this host holds no live process of another account to pin a hold to"
        ));
        return;
    };
    hold_by_a_stranger(&built.machine, lineage);

    let entered = built.machine.nodal_in(&built.home, &["run", "--", "true"]);
    assert!(!entered.status.success(), "the hold was passed over: {}", stdout(&entered));
    let said = stderr(&entered);
    assert!(said.contains("held"), "the refusal does not say the unit is held: {said}");
}

/// A live session on this host that belongs to another account, or nothing.
///
/// Session one is the first process the host started, and it is not this account's on any
/// machine this suite runs on. A host that will not answer about it is a host the claim is not
/// made on.
fn another_accounts_session() -> Option<u32> {
    let live = nodal_core::runtime::processes::session_is_live(1)?;
    live.then_some(1)
}

/// Write a hold on this project's one unit, by an actor this account is not, from `lineage`.
fn hold_by_a_stranger(machine: &Machine, lineage: u32) {
    use nodal_core::model::{Actor, ActorKind, ActorName, HostName, Lock, Timestamp};

    let store = machine.store();
    let project = machine.project(&store);
    let unit = nodal_core::store::units::list(store.conn(), project.id)
        .expect("the units are readable")
        .into_iter()
        .find(|unit| unit.slug.as_str() == SLUG)
        .expect("the unit is registered");
    let now = Timestamp::now();
    let lock = Lock {
        unit_id: unit.id,
        host: HostName::current(),
        actor: Some(Actor {
            kind: ActorKind::Agent,
            name: ActorName::parse("another-worker").expect("a name"),
        }),
        process: None,
        session: Some(lineage),
        taken_at: now,
        refreshed_at: now,
        expires_at: Timestamp::from_unix_seconds(now.unix_seconds() + 28_800).expect("an instant"),
    };
    let taken = nodal_core::store::locks::take(store.conn(), &lock, now, 28_800)
        .expect("the registry takes the hold");
    assert!(taken, "the unit was held already");
}
