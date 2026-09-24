//! A store may supply a second copy only when it holds the work, not only the commit.
//!
//! Nodal's second-copy reading asks whether a ref of another repository reaches the
//! commit. That establishes one thing: the commit object is in that store and a ref
//! keeps it there. It does not establish that the store holds the commit's tree or its
//! blobs, and the work is in the trees and the blobs.
//!
//! The two come apart in four ways, and every one of them was reproduced:
//!
//! | store | the commit graph | the objects | what it is worth |
//! |---|---|---|---|
//! | full clone | whole | there | a second copy |
//! | blobless partial clone | whole | **absent** | no copy |
//! | a clone that borrows objects (`--shared`) | whole | in the store the removal may take | no copy |
//! | shallow clone | **cut** | n/a | no copy |
//! | a worktree of this home | whole | in the home | no copy |
//!
//! A blobless partial clone was the worst of them, because it answered the old reading
//! perfectly. Nodal called the home safe and named that directory as the proof, and the
//! directory then answered `fatal: bad object <sha>:only-here.txt`.
//!
//! So a store is read before it may vouch for anything. Three configuration reads say
//! whether it borrows, whether it is partial and whether it is shallow, and one object
//! walk says whether the work behind the commits is there. A store that fails any of
//! them holds the commits as far as this machine can say and proves nothing, and the row
//! says which property failed and names the directory.
//!
//! | property | test |
//! |---|---|
//! | a blobless clone is no copy | `a_partial_clone_holds_the_commit_and_not_the_work` |
//! | a borrowing clone is no copy | `a_clone_that_borrows_its_objects_is_no_second_copy` |
//! | a shallow clone is no copy | `a_shallow_clone_is_no_second_copy` |
//! | a worktree of the home is no copy | `a_worktree_of_this_home_is_no_second_copy` |
//! | a full clone still counts | `a_full_clone_is_still_a_second_copy` |
//! | no store here reaches a network | `a_partial_clone_is_read_without_a_fetch` |

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::{Machine, git, stderr};
use serde_json::Value;

/// The unit every property here reads. It is one of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// What the one commit of every property here holds.
const CONTENT: &str = "the only copy\n";

/// The branch a home pushes its work to, as a review branch on the remote.
const TOPIC: &str = "topic";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine with a remote, whose every Nodal command is held to the filesystem.
fn machine() -> Machine {
    Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY)
}

/// A unit with one commit of its own, pushed and then dropped by the remote.
///
/// The remote no longer reaches the commit, so nothing about it is settled by the remote
/// and every property below turns on what the store beside the checkout is worth.
fn stranded(machine: &Machine) -> (PathBuf, String, String) {
    let home = machine.unit(SLUG);
    std::fs::write(home.join(ONLY), CONTENT).unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    let blob = git(&home, &["rev-parse", &format!("HEAD:{ONLY}")]);
    git(&home, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{TOPIC}")]);
    git(machine.origin(), &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    (home, tip, blob)
}

/// Where a store beside the checkout goes, which is where the sibling scan looks.
fn beside(machine: &Machine, name: &str) -> PathBuf {
    machine.source.parent().unwrap().join(name)
}

/// A URL for a local repository, so that a clone uses the transport a filter needs.
///
/// A clone given a plain path copies or links the whole object store and ignores every
/// filter, which would make the partial store in these properties a full one.
fn url(path: &Path) -> String {
    format!("file://{}", std::fs::canonicalize(path).unwrap().display())
}

/// Let a repository serve a filtered fetch, which it refuses by default.
fn allow_filters(repo: &Path) {
    git(repo, &["config", "uploadpack.allowFilter", "true"]);
}


/// Whether a store really has an object, without letting Git fetch it on demand.
///
/// A promisor store fills a missing object from its remote the moment anything asks for
/// one, so a plain `cat-file` in a partial clone answers yes about an object the store
/// has not got. `GIT_NO_LAZY_FETCH` is what makes the question about the disk.
fn holds_object(store: &Path, oid: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(store)
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["cat-file", "-e", oid])
        .status()
        .expect("git runs")
        .success()
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

/// A path as the registry and every report name it.
fn resolved(path: &Path) -> String {
    std::fs::canonicalize(path).unwrap().to_str().unwrap().to_owned()
}

/// The one shape every refusing property here asserts.
///
/// The store holds the commit, so nothing may call it a second copy; the reading of it
/// could not be made, so the commits are not checked; and the row names the directory
/// and the property that failed, because a refusal a person cannot act on is a wall.
fn lacking(answer: &Value, store: &Path, kind: &str, words: &str) {
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(answer, "second_local_copy")), 0, "it vouched anyway: {answer:#}");
    assert_eq!(count(commits(answer, "not_checked")), 1, "{answer:#}");
    let group = commits(answer, "not_checked").unwrap();
    let named = &group["copies"]["stores"][0];
    assert_eq!(named["store"], Value::from(resolved(store)), "{answer:#}");
    assert_eq!(named["lacking"]["kind"], Value::from(kind), "{answer:#}");
    assert_eq!(answer["reasons"][0]["needs"], Value::from("unknown_evidence"), "{answer:#}");
    let detail = answer["reasons"][0]["detail"].as_str().unwrap_or_default();
    assert!(detail.contains(&resolved(store)), "the reason names no directory: {detail}");
    assert!(detail.contains(words), "the reason does not say what failed: {detail}");
}

/// The invariant. A blobless clone answers every reachability question about the commit
/// and holds none of the content, and it may not make a home safe to remove.
#[test]
fn a_partial_clone_holds_the_commit_and_not_the_work() {
    let machine = machine();
    let (home, tip, blob) = stranded(&machine);
    allow_filters(&home);
    let partial = beside(&machine, "partial");
    git(
        machine.source.parent().unwrap(),
        &["clone", "--quiet", "--filter=blob:none", "--no-checkout", &url(&home), "partial"],
    );
    assert_eq!(git(&partial, &["cat-file", "-t", &tip]), "commit", "it has not got the commit");
    assert!(!holds_object(&partial, &blob), "the clone holds the blob, so this proves nothing");

    lacking(&check(&machine, SLUG), &partial, "partial", "partial clone");
    assert!(home.is_dir(), "the refusal moved the home");
}

/// A clone made with `--shared` keeps its objects in the store it was made from. That
/// store is the home the removal takes, so the clone holds the name of a commit whose
/// objects go with the directory. Git documents the hazard itself.
#[test]
fn a_clone_that_borrows_its_objects_is_no_second_copy() {
    let machine = machine();
    let (home, tip, _) = stranded(&machine);
    let borrower = beside(&machine, "borrower");
    git(
        machine.source.parent().unwrap(),
        &["clone", "--quiet", "--shared", "--no-checkout", home.to_str().unwrap(), "borrower"],
    );
    assert!(borrower.join(".git/objects/info/alternates").is_file(), "it borrows nothing");
    assert_eq!(git(&borrower, &["cat-file", "-t", &tip]), "commit");

    lacking(&check(&machine, SLUG), &borrower, "borrowed", "borrows");
    assert!(home.is_dir(), "the refusal moved the home");
}

/// A shallow clone's object store stops at a depth, so a tip in it is no promise that
/// the history behind the tip is there. The witness path refused one already; the store
/// path did not, and the two now answer alike.
#[test]
fn a_shallow_clone_is_no_second_copy() {
    let machine = machine();
    let (home, tip, _) = stranded(&machine);
    let shallow = beside(&machine, "shallow");
    git(
        machine.source.parent().unwrap(),
        &["clone", "--quiet", "--depth", "1", "--no-checkout", &url(&home), "shallow"],
    );
    assert!(shallow.join(".git/shallow").is_file(), "the clone is not shallow");
    assert_eq!(git(&shallow, &["cat-file", "-t", &tip]), "commit");

    lacking(&check(&machine, SLUG), &shallow, "shallow", "shallow");
    assert!(home.is_dir(), "the refusal moved the home");
}

/// A worktree of the home is one more checkout of the home's own repository. Its refs
/// reach every commit the home has, and its objects are the home's objects: the removal
/// takes them both. A reading that compared directory names counted it as a second
/// store, because it is at a second path.
#[test]
fn a_worktree_of_this_home_is_no_second_copy() {
    let machine = machine();
    let (home, tip, _) = stranded(&machine);
    let linked = beside(&machine, "linked");
    git(&home, &["worktree", "add", "--quiet", linked.to_str().unwrap(), "-b", "linked"]);
    assert_eq!(git(&linked, &["cat-file", "-t", &tip]), "commit");

    lacking(&check(&machine, SLUG), &linked, "same_repository", "worktree of this home");
    assert!(home.is_dir(), "the refusal moved the home");
}

/// The control, and the reason the guard is a reading rather than a refusal to answer.
/// A full clone holds the commit and the work behind it, and it still makes the home
/// safe to remove.
#[test]
fn a_full_clone_is_still_a_second_copy() {
    let machine = machine();
    let (home, tip, blob) = stranded(&machine);
    let full = beside(&machine, "full");
    git(
        machine.source.parent().unwrap(),
        &["clone", "--quiet", "--no-checkout", home.to_str().unwrap(), "full"],
    );
    assert_eq!(git(&full, &["cat-file", "-t", &blob]), "blob", "the clone holds no content");

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(true), "{answer:#}");
    assert_eq!(count(commits(&answer, "second_local_copy")), 1, "{answer:#}");
    assert_eq!(
        commits(&answer, "second_local_copy").unwrap()["copies"]["held_by"],
        Value::from(resolved(&full)),
        "{answer:#}"
    );
    assert_eq!(git(&home, &["cat-file", "-t", &tip]), "commit");
}

/// Reading a partial store may not fetch. A promisor remote makes Git fill a missing
/// object from the network on demand, and a reading that did so would be a network call
/// of Nodal's own, which Nodal does not make. The property holds it by giving the
/// promisor a remote that cannot answer: a fetch would fail the command, and the check
/// answers.
#[test]
fn a_partial_clone_is_read_without_a_fetch() {
    let machine = machine();
    let (home, _, _) = stranded(&machine);
    allow_filters(&home);
    let partial = beside(&machine, "partial");
    git(
        machine.source.parent().unwrap(),
        &["clone", "--quiet", "--filter=blob:none", "--no-checkout", &url(&home), "partial"],
    );
    git(&partial, &["remote", "set-url", "origin", "file:///nodal/no/such/repository"]);

    let answer = check(&machine, SLUG);
    assert_eq!(answer["safe_to_reclaim"], Value::Bool(false), "{answer:#}");
    assert_eq!(count(commits(&answer, "not_checked")), 1, "{answer:#}");
}
