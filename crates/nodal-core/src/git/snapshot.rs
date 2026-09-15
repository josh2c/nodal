//! The work-in-progress snapshot: one commit that holds everything a home has, made
//! without touching the index the person is using.
//!
//! There are two callers and they take the same kind of commit for two reasons.
//! `nodal done` and `nodal reclaim --force` write the work-in-progress ref: a person who
//! forces a reclaim past the uniqueness check is saying they accept losing the home, not
//! that they want the work destroyed, so the work is committed to a ref first and the
//! ref goes to trash with the home.
//!
//! The other caller is the runner ([`crate::lifecycle::run`]). Every operation that
//! changes a unit's tree or its refs — a merge, an adoption of a checkout that is
//! already here, an ordinary reclaim — records the home as it was before its first step
//! runs, on a ref named by the run: `refs/nodal/<unit>/pre/<operation>`. One ref per run,
//! so a second merge never writes over the record of the first.
//!
//! Recovering either is `git`, in the home or in the trashed copy of it:
//! `git fetch <path> refs/nodal/<id>/pre/<operation>` and then `git checkout FETCH_HEAD`
//! (`docs/contracts.md`). There is no restore verb, and a snapshot is never pushed.
//!
//! **Why a temporary index.** `git add -A` writes the repository's index, which is a
//! person's staged work. A snapshot that stages every file has changed the state of the
//! thing it was supposed to be preserving. So the whole commit is built in an index
//! file of its own, named by `GIT_INDEX_FILE`: `read-tree` fills it from `HEAD`,
//! `add -A` records the working tree into it, `write-tree` turns it into a tree object,
//! and `commit-tree` makes the commit. The repository's own index is never opened.
//!
//! # A killed snapshot must not block the next one
//!
//! `git` guards an index with a lock file beside it, `<index>.lock`, which it makes
//! before it writes and renames away after. A run killed inside [`take`] leaves that
//! lock where it is, and `git read-tree` then refuses the home with "Unable to create
//! ... File exists" for as long as the file is there. The verb that pays is
//! `nodal reclaim --force`: it snapshots the work before it moves the home, so a person
//! who forced a reclaim once and interrupted it could not force it again, and nothing
//! told them the reason was one stale file.
//!
//! So [`take`] clears both files on entry as well as on exit, and a lock of this name
//! at entry is treated as the residue of a run that ended. Two facts make that sound.
//! [`INDEX_FILE`] is Nodal's own name: no other tool and no other part of Nodal writes
//! it, so a lock of that name was written by a `take` of this home. And a `take` runs
//! inside a lifecycle operation, which the registry's single writer and the home's own
//! one-writer hold ([`crate::runtime::lock`]) keep to one actor at a time.
//!
//! The case those two do not cover is one actor running two operations on one home from
//! two terminals at the same instant. That was already broken here — the second `take`
//! failed outright — and it stays the narrow case: the window is one `git` invocation
//! wide, and what it can cost is one snapshot commit built from the wrong index, not a
//! lost working tree.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::cmd;
use super::oid::Oid;
use crate::error::Result;

/// The variable that points `git` at another index file.
const INDEX_VAR: &str = "GIT_INDEX_FILE";

/// The name of the index the snapshot builds in, inside the repository's Git directory
/// so that it is on the same filesystem and goes away with the repository.
///
/// The name is Nodal's own, which is what lets [`take`] clear a lock beside it: nothing
/// else on the machine writes a file of this name.
const INDEX_FILE: &str = "nodal-wip-index";

/// What `git` appends to an index's name for the lock it holds while it writes.
const LOCK_SUFFIX: &str = ".lock";

/// The reflog reason the ref carries, so `git reflog` says where the commit came from.
const REASON: &str = "nodal: work-in-progress snapshot";

/// Who a snapshot commit is by.
///
/// Nodal's own name, not the person's, and stated rather than left to `git` to work
/// out. A home is a clone with no identity configured in it, so a commit that took
/// whatever the machine offered would fail on a machine with no global identity — which
/// is every continuous-integration runner — and would otherwise attribute a machine's
/// safety net to a person who did not write it.
pub(super) const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "nodal"),
    ("GIT_AUTHOR_EMAIL", "nodal@localhost"),
    ("GIT_COMMITTER_NAME", "nodal"),
    ("GIT_COMMITTER_EMAIL", "nodal@localhost"),
];

/// What a snapshot recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The ref the commit was written to.
    pub reference: String,
    /// The commit itself.
    pub commit: Oid,
    /// Whether the working tree held anything `HEAD` did not. `false` means the commit
    /// is `HEAD`'s own tree, kept so that the branch's commits survive as well.
    pub had_changes: bool,
}

/// Commit everything in `repo` to `reference`, and return what was written.
///
/// Idempotent in the way a step needs: taking a second snapshot of an unchanged home
/// writes the same tree and moves the ref to an equivalent commit.
///
/// A repository whose `HEAD` has no commit yet cannot be snapshotted — there is nothing
/// to parent the commit on and nothing committed to lose — and answers `None`.
///
/// A run killed inside this function leaves its index and `git`'s lock beside it. Both
/// are cleared on entry as well as on exit, so an interrupted snapshot costs the next
/// one nothing. The module says why a lock of this name is always the older run's.
///
/// # Errors
/// [`crate::Error::Git`] when any of the four plumbing commands failed,
/// [`crate::Error::GitOid`] when one of them did not answer with an object id, and
/// [`crate::Error::Io`] when the temporary index or its lock could not be removed.
pub fn take(
    repo: &Path,
    git_dir: &Path,
    reference: &str,
    message: &str,
) -> Result<Option<Snapshot>> {
    let Some(head) = head_commit(repo)? else { return Ok(None) };
    let index = git_dir.join(INDEX_FILE);
    remove(&index)?;
    let taken = build(repo, &index, &head, message);
    remove(&index)?;
    let (tree, commit) = taken?;
    let had_changes = tree != tree_of(repo, &head)?;
    super::refs::write(repo, reference, &commit, REASON)?;
    Ok(Some(Snapshot { reference: reference.to_owned(), commit, had_changes }))
}

/// One snapshot this repository holds, as a report lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// The ref it is on.
    pub reference: String,
    /// The commit.
    pub commit: Oid,
    /// When the commit was made, which is when the snapshot was taken.
    pub taken_at: crate::model::Timestamp,
}

/// Every snapshot of a unit this repository holds, oldest first.
///
/// One `for-each-ref` over the unit's own namespace. The instant is the commit's own,
/// because a snapshot commit is written once and never moved, so the commit dates the
/// ref exactly.
///
/// # Errors
/// [`crate::Error::Git`] when `git for-each-ref` failed, and
/// [`crate::Error::GitParse`] on a record this cannot read.
pub fn list(repo: &Path, unit_id: &str) -> Result<Vec<Taken>> {
    let prefix = format!("{}{unit_id}/", super::refs::NAMESPACE);
    let output = cmd::run_ok(
        repo,
        &[
            "for-each-ref",
            "--sort=committerdate",
            "--format=%(objectname) %(committerdate:unix) %(refname)",
            &prefix,
        ],
    )?;
    output.lines()?.iter().map(|line| read_record(line, &output.args)).collect::<Result<Vec<_>>>()
}

/// One `for-each-ref` record: the object, the instant, and the name.
fn read_record(line: &str, args: &[String]) -> Result<Taken> {
    let unreadable = || crate::Error::GitParse { args: args.to_vec(), record: line.to_owned() };
    let (commit, rest) = line.split_once(' ').ok_or_else(unreadable)?;
    let (seconds, name) = rest.split_once(' ').ok_or_else(unreadable)?;
    let taken_at = seconds
        .parse::<i64>()
        .ok()
        .and_then(|seconds| crate::model::Timestamp::from_unix_seconds(seconds).ok())
        .ok_or_else(unreadable)?;
    Ok(Taken { reference: name.to_owned(), commit: Oid::parse(commit)?, taken_at })
}

/// The commit `HEAD` names, `None` when the branch has none yet.
fn head_commit(repo: &Path) -> Result<Option<Oid>> {
    let output = cmd::run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if !output.ok() {
        return Ok(None);
    }
    Ok(Some(Oid::parse(output.text()?)?))
}

/// Fill the temporary index, write its tree, and commit it on top of `head`.
fn build(repo: &Path, index: &Path, head: &Oid, message: &str) -> Result<(Oid, Oid)> {
    let variable: [(&str, &OsStr); 1] = [(INDEX_VAR, index.as_os_str())];
    cmd::run_ok_with(repo, &["read-tree", head.as_str()], &variable)?;
    cmd::run_ok_with(repo, &["add", "--all", "--", "."], &variable)?;
    let tree = Oid::parse(cmd::run_ok_with(repo, &["write-tree"], &variable)?.text()?)?;
    let mut committing: Vec<(&str, &OsStr)> = vec![(INDEX_VAR, index.as_os_str())];
    committing.extend(IDENTITY.iter().map(|(name, value)| (*name, OsStr::new(value))));
    let made = cmd::run_ok_with(
        repo,
        &["commit-tree", tree.as_str(), "-p", head.as_str(), "-m", message],
        &committing,
    )?;
    Ok((tree, Oid::parse(made.text()?)?))
}

/// The tree a commit points at.
fn tree_of(repo: &Path, commit: &Oid) -> Result<Oid> {
    let peeled = format!("{}^{{tree}}", commit.as_str());
    Oid::parse(cmd::run_ok(repo, &["rev-parse", "--verify", peeled.as_str()])?.text()?)
}

/// Remove the temporary index and the lock `git` keeps beside it.
///
/// Both, because a killed run leaves both and the lock is the one that stops the next
/// `read-tree`. Either one missing is not a failure: the build may have failed before
/// `git` made it, and an uninterrupted `git` renames its own lock away.
fn remove(index: &Path) -> Result<()> {
    delete(index)?;
    delete(&lock_beside(index))
}

/// The lock `git` writes beside an index while it changes it.
fn lock_beside(index: &Path) -> PathBuf {
    let mut name = index.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// Remove one file. One that is not there is not a failure.
fn delete(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::Error::io(path)(error)),
    }
}
