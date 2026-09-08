//! The Git half of `nodal merge`: commit, squash, rebase, fast-forward.
//!
//! Four rewrites of one branch, each of them a plumbing command rather than a porcelain
//! one wherever the porcelain would open an editor or read a configuration file that
//! belongs to the person. Nothing here talks to a network: a fast-forward moves a local
//! branch, and the objects it needs are fetched from a repository on this machine by
//! path (`decisions/DL-034`).
//!
//! # The one rule the fast-forward keeps
//!
//! A branch is moved only when the commit it points at is an ancestor of the commit it
//! is moving to. That is the whole of "never rewrites the target's history": a target
//! somebody else has moved is refused ([`crate::Error::MergeTargetMoved`]) and the
//! person is told to rebase again. There is no flag here that forces it, and there is
//! no push.
//!
//! # A conflict is an answer
//!
//! A rebase that stops for a conflict has not failed. It has left the repository in a
//! state a person can finish, which is what `git rebase` is for, so it answers
//! [`Outcome::Conflict`] and the operation reports where the work stopped. A rebase that
//! could not run at all — an unknown revision, a locked index — is a failure and is
//! returned as one.
//!
//! # Identity
//!
//! A unit's home is a copy of a base and carries no `user.name` of its own. When the
//! repository can name a committer, these commits are made by that person, as a commit
//! of theirs should be. When it cannot — a continuous-integration runner with no global
//! configuration — Nodal's own name is used rather than letting `git` refuse.

use std::ffi::OsStr;
use std::path::Path;

use super::oid::Oid;
use super::{cmd, preflight};
use crate::error::{Error, Result};

/// What a rebase did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The branch is on the target and the working tree is clean.
    Done,
    /// It stopped for a conflict, and the repository is in the middle of a rebase.
    Conflict,
}

/// Commit every tracked change and every untracked file that no ignore rule covers.
///
/// `None` when the working tree held nothing to commit, which is what makes this
/// repeatable: a second run of a killed merge finds the commit already made.
///
/// # Errors
/// [`Error::Git`] when a plumbing command failed.
pub fn commit_all(repo: &Path, message: &str) -> Result<Option<Oid>> {
    cmd::run_ok(repo, &["add", "--all", "--", "."])?;
    let staged = cmd::run(repo, &["diff", "--cached", "--quiet", "--exit-code"])?;
    if staged.ok() {
        return Ok(None);
    }
    let identity = identity(repo)?;
    let borrowed: Vec<(&str, &OsStr)> =
        identity.iter().map(|(name, value)| (*name, OsStr::new(value.as_str()))).collect();
    cmd::run_ok_with(repo, &["commit", "--quiet", "--no-verify", "-m", message], &borrowed)?;
    Ok(Some(head(repo)?))
}

/// Fold everything the branch has since its merge base with `onto` into one commit.
///
/// The tree is not touched: the commit this writes has exactly the tree the branch
/// already had, so the working tree stays as it is and the squash cannot change a file.
/// `None` when there was nothing to fold, which is a branch of one commit or of none.
///
/// # Errors
/// [`Error::Git`] when a plumbing command failed, [`Error::GitOid`] when a command did
/// not answer with an object id.
pub fn squash(repo: &Path, onto: &Oid, message: &str) -> Result<Option<Oid>> {
    let base = merge_base(repo, onto.as_str(), "HEAD")?;
    if count(repo, &format!("{}..HEAD", base.as_str()))? < 2 {
        return Ok(None);
    }
    let tree = rev(repo, "HEAD^{tree}")?;
    let mut environment = identity(repo)?;
    environment.extend(author_of(repo, "HEAD")?);
    let borrowed: Vec<(&str, &OsStr)> =
        environment.iter().map(|(name, value)| (*name, OsStr::new(value.as_str()))).collect();
    let made = cmd::run_ok_with(
        repo,
        &["commit-tree", tree.as_str(), "-p", base.as_str(), "-m", message],
        &borrowed,
    )?;
    let commit = Oid::parse(made.text()?)?;
    cmd::run_ok(repo, &["reset", "--soft", commit.as_str()])?;
    Ok(Some(commit))
}

/// Rebase the checked-out branch onto a commit.
///
/// Repeatable: a branch already on the target is left alone, and a repository already
/// in the middle of a rebase is carried on rather than started again.
///
/// # Errors
/// [`Error::Git`] when the rebase could not run at all.
pub fn rebase(repo: &Path, git_dir: &Path, onto: &Oid) -> Result<Outcome> {
    if rebasing(git_dir) {
        return resume(repo, git_dir);
    }
    if is_ancestor(repo, onto.as_str(), "HEAD")? {
        return Ok(Outcome::Done);
    }
    run_rebase(repo, git_dir, &["rebase", "--quiet", onto.as_str()])
}

/// Carry on a rebase a person has resolved the conflicts of.
///
/// # Errors
/// [`Error::Git`] when the rebase could not run at all.
pub fn resume(repo: &Path, git_dir: &Path) -> Result<Outcome> {
    if !rebasing(git_dir) {
        return Ok(Outcome::Done);
    }
    run_rebase(repo, git_dir, &["rebase", "--continue"])
}

/// Stop a rebase and put the branch back where it was. A repository that is not in one
/// is left alone.
///
/// # Errors
/// [`Error::Git`] when the abort itself failed.
pub fn abort(repo: &Path, git_dir: &Path) -> Result<()> {
    if !rebasing(git_dir) {
        return Ok(());
    }
    cmd::run_ok(repo, &["rebase", "--abort"])?;
    Ok(())
}

/// The branch a rebase in progress will put back when it finishes, `None` when there
/// is no rebase or Git did not record one.
///
/// A rebase detaches `HEAD`, so this is the only thing that says which branch a stopped
/// merge is about.
#[must_use]
pub fn rebasing_branch(git_dir: &Path) -> Option<String> {
    let recorded = std::fs::read_to_string(git_dir.join("rebase-merge").join("head-name")).ok()?;
    Some(recorded.trim().strip_prefix("refs/heads/")?.to_owned())
}

/// Whether this repository is in the middle of a rebase.
#[must_use]
pub fn rebasing(git_dir: &Path) -> bool {
    preflight::inspect(git_dir).states.contains(&preflight::State::Rebase)
}

/// Move a local branch to a commit, and only when that is a fast-forward.
///
/// Repeatable: a branch already at the commit is left alone.
///
/// # Errors
/// [`Error::MergeTargetMoved`] when the branch carries a commit the new tip does not,
/// [`Error::GitUnknownBranch`] when there is no such branch, [`Error::Git`] when the
/// move failed.
pub fn fast_forward(repo: &Path, branch: &str, to: &Oid) -> Result<bool> {
    let reference = format!("refs/heads/{branch}");
    let at = super::refs::read(repo, &reference)?
        .ok_or_else(|| Error::GitUnknownBranch { repo: repo.into(), branch: branch.to_owned() })?;
    if at == *to {
        return Ok(false);
    }
    if !is_ancestor(repo, at.as_str(), to.as_str())? {
        return Err(Error::MergeTargetMoved {
            branch: branch.to_owned(),
            found: at.as_str().to_owned(),
        });
    }
    move_branch(repo, branch, to)?;
    Ok(true)
}

/// Put a local branch back at a commit it used to point at, for the undo of a
/// fast-forward. Refuses to lose a change in the working tree.
///
/// # Errors
/// [`Error::Git`] when the branch could not be moved.
pub fn restore(repo: &Path, branch: &str, to: &Oid) -> Result<()> {
    if super::refs::read(repo, &format!("refs/heads/{branch}"))?.as_ref() == Some(to) {
        return Ok(());
    }
    move_branch(repo, branch, to)
}

/// Move a branch, through the working tree when that branch is the checked-out one.
///
/// Two commands and never `update-ref`, and that is the point. `git reset --keep`
/// refuses to lose a change somebody has in the working tree, and `git branch --force`
/// refuses a branch another worktree of the same repository has checked out. Writing the
/// ref directly would do both of those things silently, to a tree Nodal does not own.
fn move_branch(repo: &Path, branch: &str, to: &Oid) -> Result<()> {
    if checked_out(repo, branch)? {
        cmd::run_ok(repo, &["reset", "--keep", to.as_str()])?;
        return Ok(());
    }
    cmd::run_ok(repo, &["branch", "--force", "--", branch, to.as_str()])?;
    Ok(())
}

/// Whether a branch is the one this repository has checked out.
fn checked_out(repo: &Path, branch: &str) -> Result<bool> {
    let output = cmd::run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    Ok(output.ok() && output.text()? == branch)
}

/// The commit two revisions last had in common.
///
/// # Errors
/// [`Error::Git`] when either revision is unknown.
pub fn merge_base(repo: &Path, left: &str, right: &str) -> Result<Oid> {
    let output = cmd::run_ok(repo, &["merge-base", "--end-of-options", left, right])?;
    Oid::parse(output.text()?)
}

/// Whether every commit of `earlier` is in `later`.
///
/// # Errors
/// [`Error::GitSpawn`] when `git` could not be started.
pub fn is_ancestor(repo: &Path, earlier: &str, later: &str) -> Result<bool> {
    let args = ["merge-base", "--is-ancestor", "--end-of-options", earlier, later];
    Ok(cmd::run(repo, &args)?.ok())
}

/// How many commits a range holds.
///
/// # Errors
/// [`Error::Git`] when the range is not one this repository can read.
pub fn count(repo: &Path, range: &str) -> Result<u32> {
    let output = cmd::run_ok(repo, &["rev-list", "--count", "--end-of-options", range])?;
    let text = output.text()?;
    text.parse().map_err(|_| Error::GitParse { args: output.args.clone(), record: text.to_owned() })
}

/// Fetch one branch of a repository on this machine, keeping no ref for it.
///
/// What a fast-forward needs and no more: the objects. The branch that is about to be
/// moved is what makes them reachable a moment later, so nothing of Nodal's is left
/// behind in a person's own checkout.
///
/// # Errors
/// [`Error::Git`] when the fetch failed, [`Error::InvalidValue`] when the path is not
/// UTF-8.
pub fn fetch_objects(repo: &Path, from: &Path, branch: &str) -> Result<()> {
    let source = text_of(from)?;
    let spec = format!("refs/heads/{branch}");
    cmd::run_ok(repo, &["fetch", "--quiet", "--no-tags", "--", source, &spec])?;
    Ok(())
}

/// A path as an argument, refused rather than mangled when it is not UTF-8.
fn text_of(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| Error::InvalidValue {
        kind: "path",
        value: path.to_string_lossy().into_owned(),
    })
}

/// Fetch one branch of a repository on this machine into a ref of Nodal's own.
///
/// By path and by name: the objects come from a directory, and the ref they land on is
/// inside `refs/nodal/`, so nothing of the person's own remote configuration decides
/// what a merge measures itself against.
///
/// # Errors
/// [`Error::Git`] when the fetch failed, [`Error::InvalidValue`] when the path is not
/// UTF-8.
pub fn fetch_branch(repo: &Path, from: &Path, branch: &str, into: &str) -> Result<Oid> {
    let source = text_of(from)?;
    let spec = format!("+refs/heads/{branch}:{into}");
    cmd::run_ok(repo, &["fetch", "--quiet", "--no-tags", "--", source, &spec])?;
    super::refs::read(repo, into)?
        .ok_or_else(|| Error::GitUnknownBranch { repo: repo.into(), branch: branch.to_owned() })
}

/// Take back a commit and leave what it held in the working tree.
///
/// The undo of [`commit_all`]: the branch goes back to the commit it was at, the index
/// goes back with it, and every file the commit recorded is a change again.
///
/// # Errors
/// [`Error::Git`] when the reset failed.
pub fn uncommit(repo: &Path, to: &Oid) -> Result<()> {
    if head(repo)? == *to {
        return Ok(());
    }
    cmd::run_ok(repo, &["reset", "--mixed", "--quiet", to.as_str()])?;
    Ok(())
}

/// The paths a stopped rebase could not merge.
///
/// # Errors
/// [`Error::Git`] when the index could not be read.
pub fn conflicts(repo: &Path) -> Result<Vec<String>> {
    let output =
        cmd::run_ok(repo, &["diff", "--name-only", "--diff-filter=U", "-z", "--no-color"])?;
    Ok(output.records()?.iter().map(|record| (*record).to_owned()).collect())
}

/// The commit `HEAD` names.
fn head(repo: &Path) -> Result<Oid> {
    rev(repo, "HEAD")
}

/// Resolve a revision.
fn rev(repo: &Path, revision: &str) -> Result<Oid> {
    let output = cmd::run_ok(repo, &["rev-parse", "--verify", "--end-of-options", revision])?;
    Oid::parse(output.text()?)
}

/// Run one rebase command and read a conflict as an answer rather than a failure.
fn run_rebase(repo: &Path, git_dir: &Path, args: &[&str]) -> Result<Outcome> {
    let mut environment = identity(repo)?;
    // A rebase writes a commit message, and a person's editor must never open inside a
    // command that is not a conversation.
    environment.push(("GIT_EDITOR", String::from("true")));
    let borrowed: Vec<(&str, &OsStr)> =
        environment.iter().map(|(name, value)| (*name, OsStr::new(value.as_str()))).collect();
    let output = cmd::run_with(repo, args, &borrowed)?;
    if output.ok() {
        return Ok(Outcome::Done);
    }
    if rebasing(git_dir) {
        return Ok(Outcome::Conflict);
    }
    Err(Error::Git {
        repo: repo.to_path_buf(),
        args: output.args,
        code: output.code,
        stderr: output.stderr,
    })
}

/// Who these commits are by: the repository's own identity when it has one, and Nodal's
/// when it has none.
fn identity(repo: &Path) -> Result<Vec<(&'static str, String)>> {
    let configured = cmd::run(repo, &["config", "--get", "user.email"])?;
    if configured.ok() && !configured.text()?.is_empty() {
        return Ok(Vec::new());
    }
    Ok(super::snapshot::IDENTITY.iter().map(|(name, value)| (*name, (*value).to_owned())).collect())
}

/// The author of a commit, so a rewrite of it keeps the person who wrote it.
fn author_of(repo: &Path, revision: &str) -> Result<Vec<(&'static str, String)>> {
    let format = "--format=%an%n%ae%n%aI";
    let output = cmd::run_ok(repo, &["log", "-1", format, "--end-of-options", revision])?;
    let read = output.lines()?;
    let [name, email, date] = read.as_slice() else {
        return Ok(Vec::new());
    };
    Ok(vec![
        ("GIT_AUTHOR_NAME", (*name).to_owned()),
        ("GIT_AUTHOR_EMAIL", (*email).to_owned()),
        ("GIT_AUTHOR_DATE", (*date).to_owned()),
    ])
}
