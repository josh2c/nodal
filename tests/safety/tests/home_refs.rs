//! The verdict reads every ref the home holds, and not only the branch it is on.
//!
//! A person works on the branch the home is checked out on, and most of the time every
//! commit they made is behind `HEAD`. Most of the time is not the promise. `git switch
//! -c`, `git stash` and `git tag` all write a ref, and a commit under one of those refs
//! is work the home holds and nothing else has. A reading that walked `HEAD` alone
//! reported `commits: []` over such a home and called it safe to remove, and the
//! directory then went to the trash whole and out of it on a timer.
//!
//! So the assessed set is every ref under `refs/` of the home, plus `HEAD`. Two
//! namespaces are left out of it, and both are copies of the person's own checkout that
//! the create wrote in: `refs/nodal/origin/` is the checkout's reading of the remote and
//! `refs/nodal/checkout/` is the checkout's own branches. Neither is work this home did.
//! Nodal's own namespace is left out with them, and for the same reason. Every ref under
//! `refs/nodal/` is one Nodal wrote: two are those copies, and the rest are records of
//! runs whose trees and whose parents this reading already has from the working tree and
//! from the home's own branch. The sweep of the trash draws the same line and adds the
//! snapshot ref to its own reading, because nothing is checked out in the trash.
//!
//! Every property here runs on a machine whose project has no remote, so the checkout is
//! what `origin` names and the remote question is settled by reading it. A commit the
//! checkout does not hold is then only here as a fact, and the report says so in those
//! words.
//!
//! | property | test |
//! |---|---|
//! | a side branch is work | `a_commit_on_a_branch_the_head_does_not_reach_is_only_here` |
//! | a stash is work | `a_stash_is_work_the_home_holds` |
//! | a tag is work | `a_tag_that_is_the_one_ref_on_a_commit_is_work` |
//! | the records Nodal writes are not work | `the_records_nodal_writes_are_not_the_homes_own_work` |
//! | the copied-in refs are not work | `the_refs_the_create_copied_in_are_not_the_homes_own_work` |
//! | a side branch a second copy holds is safe | `a_side_branch_a_second_copy_holds_is_safe` |

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::InState as _;
use nodal_safety::{Machine, git, stderr, stdout};
use serde_json::Value;

/// The unit every property here reads. It is one of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// A path the fixture tracks, which is what a stash needs to have something to hold.
const TRACKED: &str = "apps/web/app/page.tsx";

/// Where a home records which unit it is. The snapshot refs are under that identifier.
const MARKER: &str = ".nodal/id";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine whose every Nodal command is held to the filesystem.
///
/// The project has no remote, so the home's `origin` is the checkout itself. That is the
/// relation in which reading the checkout is reading the remote, and a commit it does
/// not hold is only here rather than unchecked.
fn machine() -> Machine {
    Machine::new().with_env(ONLY_LOCAL).with_env(NO_PROXY)
}

/// The preflight for one unit, as the value every property reads.
fn check(machine: &Machine, slug: &str) -> Value {
    let asked = machine.nodal(&["reclaim", slug, "--check", "--json"]);
    let printed = nodal_safety::answer(&asked);
    let read: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("--check --json is one document: {printed}{}", stderr(&asked)));
    assert_eq!(
        read["safe_to_reclaim"],
        Value::Bool(asked.status.success()),
        "the exit code and the verdict disagree: {printed}"
    );
    read
}

/// The commit group of one disposition, or nothing when the reading found none.
fn commits<'a>(answer: &'a Value, kind: &str) -> Option<&'a Value> {
    answer["commits"]
        .as_array()?
        .iter()
        .find(|group| group["copies"]["kind"] == Value::String(kind.to_owned()))
}

/// How many commits a group is about, and zero when there is no such group.
fn count(group: Option<&Value>) -> u64 {
    group.and_then(|group| group["count"].as_u64()).unwrap_or_default()
}

/// A commit in the home that `HEAD` does not reach afterwards, and its identifier.
///
/// The branch is made, committed on and left. `HEAD` goes back to the unit's own branch,
/// so the commit is reachable from `refs/heads/side-work` and from nothing else.
fn on_a_side_branch(home: &Path) -> String {
    let branch = git(home, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(home, &["switch", "--quiet", "--create", "side-work"]);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(home, &["add", "--all"]);
    git(home, &["commit", "--quiet", "--message", "work on a branch nothing else has"]);
    let tip = git(home, &["rev-parse", "HEAD"]);
    git(home, &["switch", "--quiet", &branch]);
    assert_ne!(git(home, &["rev-parse", "HEAD"]), tip, "HEAD still reaches the commit");
    tip
}

/// Insist that the home is where it was and plain Git still reaches the commit.
fn intact(machine: &Machine, home: &Path, tip: &str) {
    assert!(home.is_dir(), "the refusal moved the home");
    assert_eq!(machine.homes(), vec![home.to_path_buf()], "the home is still the project's");
    assert!(machine.trashed().is_empty(), "the refusal put something in the trash");
    assert_eq!(git(home, &["cat-file", "-t", tip]), "commit", "the commit is not readable");
}

/// Insist that the reclaim itself refuses the unit the preflight refused, and says why.
fn reclaim_also_refuses(machine: &Machine, slug: &str, naming: &str) {
    let refused = machine.nodal(&["reclaim", slug]);
    assert!(!refused.status.success(), "the reclaim went ahead: {}", stdout(&refused));
    let told = stderr(&refused);
    assert!(told.contains(naming), "the refusal does not name the work: {told}");
}

/// The invariant this file exists for. The commit is on a branch, the home is checked
/// out on another, and the verdict names it.
#[test]
fn a_commit_on_a_branch_the_head_does_not_reach_is_only_here() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let tip = on_a_side_branch(&home);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 1, "{answer:#}");
    assert_eq!(commits(&answer, "only_here").unwrap()["sample"][0], Value::from(tip.as_str()));

    reclaim_also_refuses(&machine, SLUG, "commits on no remote (1)");
    intact(&machine, &home, &tip);
}

/// A stash is a commit nobody else has. It leaves the working tree clean, so nothing in
/// the tree refuses over it, and the only thing that names it is `refs/stash`.
#[test]
fn a_stash_is_work_the_home_holds() {
    let machine = machine();
    let home = machine.unit(SLUG);
    std::fs::write(home.join(TRACKED), "edited, then stashed\n").unwrap();
    git(&home, &["stash", "push", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "refs/stash"]);
    assert!(git(&home, &["status", "--porcelain"]).is_empty(), "the stash left the tree dirty");

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert!(count(commits(&answer, "only_here")) >= 1, "{answer:#}");

    reclaim_also_refuses(&machine, SLUG, "commits on no remote");
    intact(&machine, &home, &tip);
}

/// A tag is a ref like any other. The branch that carried the commit is deleted, so the
/// tag is the one name on it, and a reading that walked branches alone would miss it.
#[test]
fn a_tag_that_is_the_one_ref_on_a_commit_is_work() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let tip = on_a_side_branch(&home);
    git(&home, &["tag", "--annotate", "--message", "a release", "kept", &tip]);
    git(&home, &["branch", "--quiet", "--delete", "--force", "side-work"]);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 1, "{answer:#}");
    assert_eq!(commits(&answer, "only_here").unwrap()["sample"][0], Value::from(tip.as_str()));

    reclaim_also_refuses(&machine, SLUG, "commits on no remote (1)");
    intact(&machine, &home, &tip);
}

/// The refs Nodal writes for itself are not the home's work, and the reading says so.
///
/// Every ref under `refs/nodal/` is one Nodal put there. Two are copies of the person's
/// own checkout. The rest are records of runs: what an operation wrote before it ran, the
/// branch a squash folded, and the snapshot of the working tree a `done` takes. A record
/// holds the tree the working tree held and the commits the home's own branch reaches, so
/// this reading has its content already. Counting them would keep every home that has
/// ever run a command, for ever, and `nodal done` writes one every time.
///
/// The sweep of the trash draws the same line and adds one ref to it
/// (`nodal_core::lifecycle::ops::gc`): nothing is checked out in the trash, so a forced
/// reclaim's snapshot is named there, because it holds a working tree no tree reading can
/// reach any more.
#[test]
fn the_records_nodal_writes_are_not_the_homes_own_work() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let tip = on_a_side_branch(&home);
    let unit = std::fs::read_to_string(home.join(MARKER)).unwrap();
    for name in ["wip", "premerge", "pre/01JRUN"] {
        git(&home, &["update-ref", &format!("refs/nodal/{}/{name}", unit.trim()), &tip]);
    }
    git(&home, &["branch", "--quiet", "--delete", "--force", "side-work"]);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 0, "{answer:#}");
    let record = &answer["reading"]["refs"]["not_walked"];
    assert!(
        record.to_string().contains("refs/nodal/"),
        "the record does not say what was left out: {record}"
    );
}

/// The control that keeps the widening honest. A create copies the person's own checkout
/// into the home under two namespaces of Nodal's own. They are the checkout's refs, not
/// the home's work, and a reading that counted them would refuse every reclaim of every
/// home on the machine.
#[test]
fn the_refs_the_create_copied_in_are_not_the_homes_own_work() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let copied = git(&home, &["for-each-ref", "--format=%(refname)", "refs/nodal/"]);
    assert!(copied.contains("refs/nodal/checkout/"), "the create copied nothing in: {copied}");

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "{answer:#}");
    assert_eq!(count(commits(&answer, "only_here")), 0, "{answer:#}");
    assert_eq!(count(commits(&answer, "not_checked")), 0, "{answer:#}");
}

/// The second control. The widening finds more commits and it does not change what
/// proves one: another repository on this disk holds the side branch, so removing the
/// home loses nothing and the verdict says safe.
#[test]
fn a_side_branch_a_second_copy_holds_is_safe() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let tip = on_a_side_branch(&home);
    let beside = sibling(&machine);
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), "side-work:refs/heads/copy"]);
    assert_eq!(git(&beside, &["cat-file", "-t", &tip]), "commit", "the sibling has nothing");

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "{answer:#}");
    assert_eq!(count(commits(&answer, "second_local_copy")), 1, "{answer:#}");
}

/// An empty repository beside the checkout, which the sibling scan finds.
fn sibling(machine: &Machine) -> PathBuf {
    let parent = machine.source.parent().unwrap().to_path_buf();
    git(&parent, &["init", "--quiet", "--initial-branch", "main", "sibling"]);
    parent.join("sibling")
}
