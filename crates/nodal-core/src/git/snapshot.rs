//! The work-in-progress snapshot: one commit that holds everything a home has, made
//! without touching the index the person is using.
//!
//! `nodal reclaim --force` is the only caller. A person who forces a reclaim past the
//! uniqueness check is saying they accept losing the home, not that they want the work
//! destroyed, so the work is committed to a ref first and the ref goes to trash with
//! the home. Recovering it is `git fetch <trash path> refs/nodal/<id>/wip`.
//!
//! This is the minimal form of the snapshot. The full feature — snapshots on a
//! schedule, a `nodal` command that restores one — is a later task; what is here is
//! the safety net a destructive flag must not be shipped without.
//!
//! **Why a temporary index.** `git add -A` writes the repository's index, which is a
//! person's staged work. A snapshot that stages every file has changed the state of the
//! thing it was supposed to be preserving. So the whole commit is built in an index
//! file of its own, named by `GIT_INDEX_FILE`: `read-tree` fills it from `HEAD`,
//! `add -A` records the working tree into it, `write-tree` turns it into a tree object,
//! and `commit-tree` makes the commit. The repository's own index is never opened.

use std::ffi::OsStr;
use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::Result;

/// The variable that points `git` at another index file.
const INDEX_VAR: &str = "GIT_INDEX_FILE";

/// The name of the index the snapshot builds in, inside the repository's Git directory
/// so that it is on the same filesystem and goes away with the repository.
const INDEX_FILE: &str = "nodal-wip-index";

/// The reflog reason the ref carries, so `git reflog` says where the commit came from.
const REASON: &str = "nodal: work-in-progress snapshot";

/// Who a snapshot commit is by.
///
/// Nodal's own name, not the person's, and stated rather than left to `git` to work
/// out. A home is a clone with no identity configured in it, so a commit that took
/// whatever the machine offered would fail on a machine with no global identity — which
/// is every continuous-integration runner — and would otherwise attribute a machine's
/// safety net to a person who did not write it.
const AUTHOR: [(&str, &str); 4] = [
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
/// # Errors
/// [`crate::Error::Git`] when any of the four plumbing commands failed,
/// [`crate::Error::GitOid`] when one of them did not answer with an object id, and
/// [`crate::Error::Io`] when the temporary index could not be removed.
pub fn take(
    repo: &Path,
    git_dir: &Path,
    reference: &str,
    message: &str,
) -> Result<Option<Snapshot>> {
    let Some(head) = head_commit(repo)? else { return Ok(None) };
    let index = git_dir.join(INDEX_FILE);
    let taken = build(repo, &index, &head, message);
    remove(&index)?;
    let (tree, commit) = taken?;
    let had_changes = tree != tree_of(repo, &head)?;
    super::refs::write(repo, reference, &commit, REASON)?;
    Ok(Some(Snapshot { reference: reference.to_owned(), commit, had_changes }))
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
    committing.extend(AUTHOR.iter().map(|(name, value)| (*name, OsStr::new(value))));
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

/// Remove the temporary index. One that is not there is not a failure: the build may
/// have failed before `git` created it.
fn remove(index: &Path) -> Result<()> {
    match std::fs::remove_file(index) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::Error::io(index)(error)),
    }
}
