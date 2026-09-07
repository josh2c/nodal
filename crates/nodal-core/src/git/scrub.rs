//! Post-clone scrub: make a copy-on-write clone of a base into a repository of its own.
//!
//! A clone that includes `.git` inherits the source's worktree registrations, HEAD, and
//! any hooks path pointing outside the copy. Left alone, a `git worktree`
//! command inside the unit would delete directories belonging to the source, and hooks
//! from another checkout would run against this one. Each step below is idempotent and
//! safe to repeat.
//!
//! Nodal installs no hook scripts here, in a unit or anywhere else; the scrub
//! only makes the repository use its own hooks directory again.

use std::path::{Path, PathBuf};

use super::{cmd, preflight, refs, worktree};
use crate::error::{Error, Result};

/// The inherited directory of worktree registrations, relative to the Git directory.
const WORKTREES: &str = "worktrees";

/// What the scrub should leave HEAD pointing at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Branch HEAD must name afterwards, short form. `None` leaves HEAD as it is.
    pub head_branch: Option<String>,
}

/// What the scrub changed. Every field is `false`/`None` on a repeat run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Inherited worktree registrations were present and were removed.
    pub worktrees_removed: bool,
    /// HEAD was repointed at this branch.
    pub head_set: Option<String>,
    /// `gc.auto` was not `0` and is now.
    pub gc_auto_disabled: bool,
    /// A `core.hooksPath` pointing outside this repository was removed.
    pub hooks_path_cleared: Option<PathBuf>,
}

/// What a scrub needs to know about the repository it is working on.
struct Target<'a> {
    /// The repository root.
    repo: &'a Path,
    /// Its Git directory.
    git_dir: PathBuf,
}

/// Scrub inherited Git state out of a freshly cloned unit repository.
///
/// # Errors
/// [`Error::GitLinkedWorktree`] when the repository is a linked worktree or bare,
/// [`Error::GitInProgress`] when the clone inherited a half-finished Git operation,
/// [`Error::GitUnknownBranch`] when `head_branch` does not exist, [`Error::Io`] when the
/// worktree registrations could not be removed, [`Error::Git`] when a `git` call failed.
pub(super) fn apply(repo: &Path, layout: &worktree::Layout, options: &Options) -> Result<Report> {
    if layout.kind != worktree::Kind::Main {
        return Err(Error::GitLinkedWorktree {
            repo: PathBuf::from(repo),
            git_dir: layout.git_dir.clone(),
        });
    }
    let inspected = preflight::inspect(&layout.git_dir);
    if !inspected.is_clear() {
        return Err(Error::GitInProgress { repo: PathBuf::from(repo), states: inspected.states });
    }
    let target = Target { repo, git_dir: layout.git_dir.clone() };
    Ok(Report {
        worktrees_removed: remove_worktrees(&target)?,
        head_set: set_head(&target, options.head_branch.as_deref())?,
        gc_auto_disabled: disable_gc(&target)?,
        hooks_path_cleared: clear_outside_hooks_path(&target)?,
    })
}

/// Remove the worktree registrations the clone inherited from its source.
///
/// Only the metadata directory is removed: `git worktree remove` would delete the
/// source's checkouts, which are not ours to touch.
fn remove_worktrees(target: &Target<'_>) -> Result<bool> {
    let path = target.git_dir.join(WORKTREES);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&path).map_err(Error::io(&path))?;
    Ok(true)
}

/// Point HEAD at the unit's branch, which must already exist.
fn set_head(target: &Target<'_>, branch: Option<&str>) -> Result<Option<String>> {
    let Some(branch) = branch else { return Ok(None) };
    let name = format!("refs/heads/{branch}");
    if refs::read(target.repo, &name)?.is_none() {
        return Err(Error::GitUnknownBranch {
            repo: PathBuf::from(target.repo),
            branch: branch.to_owned(),
        });
    }
    let current = cmd::run(target.repo, &["symbolic-ref", "--quiet", "HEAD"])?;
    if current.ok() && current.text()? == name {
        return Ok(None);
    }
    cmd::run_ok(target.repo, &["symbolic-ref", "HEAD", &name])?;
    Ok(Some(branch.to_owned()))
}

/// Turn automatic garbage collection off, so a unit never repacks under an agent.
fn disable_gc(target: &Target<'_>) -> Result<bool> {
    if config(target.repo, "gc.auto")? == Some("0".to_owned()) {
        return Ok(false);
    }
    cmd::run_ok(target.repo, &["config", "gc.auto", "0"])?;
    Ok(true)
}

/// Drop a `core.hooksPath` that points outside this repository.
///
/// A relative path (what `husky` and friends configure) resolves inside the unit and is
/// already per-unit, so it is left alone. Unsetting sends Git back to this repository's
/// own `hooks` directory.
fn clear_outside_hooks_path(target: &Target<'_>) -> Result<Option<PathBuf>> {
    let Some(configured) = config(target.repo, "core.hooksPath")? else { return Ok(None) };
    let path = PathBuf::from(&configured);
    if contains(target.repo, &path) {
        return Ok(None);
    }
    cmd::run_ok(target.repo, &["config", "--unset-all", "core.hooksPath"])?;
    Ok(Some(path))
}

/// Whether `path` resolves inside `root`. Relative paths always do; Git resolves them
/// against the repository root.
fn contains(root: &Path, path: &Path) -> bool {
    if path.is_relative() {
        return true;
    }
    let resolved = |p: &Path| p.canonicalize().unwrap_or_else(|_| PathBuf::from(p));
    resolved(path).starts_with(resolved(root))
}

/// Read one config value, `None` when it is unset.
fn config(repo: &Path, key: &str) -> Result<Option<String>> {
    let output = cmd::run(repo, &["config", "--get", key])?;
    if !output.ok() {
        return Ok(None);
    }
    Ok(Some(output.text()?.to_owned()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::contains;

    #[test]
    fn relative_hooks_paths_stay_and_outside_paths_do_not() {
        assert!(contains(Path::new("/home/u/unit"), Path::new(".husky")));
        assert!(contains(Path::new("/home/u/unit"), Path::new("/home/u/unit/.git/hooks")));
        assert!(!contains(Path::new("/home/u/unit"), Path::new("/home/u/source/.husky")));
    }
}
