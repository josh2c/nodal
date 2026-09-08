//! Worktree detection, and the record a repository keeps of the worktrees it has.
//!
//! Nodal never creates worktrees: a unit is an independent copy-on-write clone of a
//! base. A linked worktree is still something we must recognise, because operations that
//! are safe in an ordinary repository (removing `worktrees/`, deleting the Git directory)
//! would damage the repository it is linked to.
//!
//! [`Registered`] is the other half: what `git worktree list` says a checkout has. It is
//! what `nodal doctor` reads to find the worktrees another tool made inside a checkout,
//! and it is where a lock is stated. A locked worktree is one a tool says it is working
//! in, and doctor reads nothing further about one.

use std::path::{Path, PathBuf};

use super::cmd;
use crate::error::Result;

/// How a repository is laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary repository with its own Git directory.
    Main,
    /// A linked worktree whose objects and refs live in another repository.
    Linked,
    /// A repository with no working tree.
    Bare,
}

/// Where a repository keeps its Git state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// How the repository is laid out.
    pub kind: Kind,
    /// The Git directory of this checkout; per-worktree for a linked worktree.
    pub git_dir: PathBuf,
    /// The shared Git directory: equal to `git_dir` unless this is a linked worktree.
    pub common_dir: PathBuf,
}

impl Layout {
    /// Whether this checkout shares its objects and refs with another one.
    #[must_use]
    pub fn is_linked(&self) -> bool {
        self.kind == Kind::Linked
    }
}

/// One worktree of a repository, as `git worktree list --porcelain` states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// Where the working tree is.
    pub path: PathBuf,
    /// The branch it has checked out, `None` when its HEAD is detached.
    pub branch: Option<String>,
    /// The reason a lock was taken, when the worktree is locked. An empty string is a
    /// lock with no reason given, which is still a lock.
    pub locked: Option<String>,
    /// Why the repository calls this worktree prunable, when it does. An empty string
    /// is the verdict with no reason given, which is still the verdict.
    ///
    /// Git decides this, and nothing else may soften it. The usual reason is a `gitdir`
    /// file that points at a location which is not there. That is a statement about the
    /// repository's own record, not about the directory: a reaper that removes the
    /// files of a worktree and leaves its directories behind produces a path that
    /// exists and a worktree that is still prunable.
    pub prunable: Option<String>,
}

impl Registered {
    /// Whether a tool holds this worktree.
    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// Whether the repository calls this worktree prunable.
    #[must_use]
    pub const fn is_prunable(&self) -> bool {
        self.prunable.is_some()
    }
}

/// Every worktree the repository at `repo` has, the main one included.
///
/// # Errors
/// [`crate::Error::Git`] when `git worktree list` failed, [`crate::Error::GitEncoding`]
/// when its output is not UTF-8.
pub(super) fn list(repo: &Path) -> Result<Vec<Registered>> {
    let output = cmd::run_ok(repo, &["worktree", "list", "--porcelain"])?;
    Ok(parse_list(output.text()?))
}

/// Parse the blocks `git worktree list --porcelain` writes.
///
/// One block per worktree, separated by an empty line. The first line of a block names
/// the path; the lines after it are single words or `word value` pairs, and this reads
/// the three it needs.
fn parse_list(text: &str) -> Vec<Registered> {
    let mut worktrees: Vec<Registered> = Vec::new();
    for line in text.lines() {
        let (word, value) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "worktree" => worktrees.push(Registered {
                path: PathBuf::from(value),
                branch: None,
                locked: None,
                prunable: None,
            }),
            "branch" => {
                if let Some(last) = worktrees.last_mut() {
                    last.branch = Some(value.trim_start_matches("refs/heads/").to_owned());
                }
            }
            "locked" => {
                if let Some(last) = worktrees.last_mut() {
                    last.locked = Some(value.to_owned());
                }
            }
            "prunable" => {
                if let Some(last) = worktrees.last_mut() {
                    last.prunable = Some(value.to_owned());
                }
            }
            _ => {}
        }
    }
    worktrees
}

/// Classify from the three values `git rev-parse` reports.
#[must_use]
pub(super) fn classify(git_dir: PathBuf, common_dir: PathBuf, bare: bool) -> Layout {
    let kind = if bare {
        Kind::Bare
    } else if git_dir == common_dir {
        Kind::Main
    } else {
        Kind::Linked
    };
    Layout { kind, git_dir, common_dir }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::{Kind, classify, parse_list};

    #[test]
    fn a_locked_worktree_states_the_reason_it_was_locked() {
        let listed = parse_list(concat!(
            "worktree /r\nHEAD abc\nbranch refs/heads/main\n\n",
            "worktree /r/.claude/worktrees/w1\nHEAD abc\ndetached\n\n",
            "worktree /r/.claude/worktrees/w2\nHEAD abc\nbranch refs/heads/feat\nlocked an agent is here\n",
        ));
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].branch.as_deref(), Some("main"));
        assert!(!listed[0].is_locked());
        assert_eq!(listed[1].branch, None, "a detached worktree names no branch");
        assert_eq!(listed[2].locked.as_deref(), Some("an agent is here"));
        assert!(listed[2].is_locked());
    }

    /// Git's own verdict, read as git states it.
    ///
    /// `git worktree list --porcelain` prints `prunable` with the reason on the same
    /// line. Both shapes of a reaped worktree carry it: the one whose directory is gone
    /// and the hollow shell whose directory is still there.
    #[test]
    fn a_prunable_worktree_carries_gits_verdict_and_its_reason() {
        let listed = parse_list(concat!(
            "worktree /r\nHEAD abc\nbranch refs/heads/main\n\n",
            "worktree /tmp/gone\nHEAD abc\nbranch refs/heads/gone\n",
            "prunable gitdir file points to non-existent location\n\n",
            "worktree /tmp/hollow\nHEAD abc\nbranch refs/heads/hollow\nprunable\n",
        ));
        assert_eq!(listed.len(), 3);
        assert!(!listed[0].is_prunable());
        assert_eq!(
            listed[1].prunable.as_deref(),
            Some("gitdir file points to non-existent location")
        );
        assert!(listed[2].is_prunable(), "a verdict with no reason is still the verdict");
        assert_eq!(listed[2].prunable.as_deref(), Some(""));
    }

    #[test]
    fn a_lock_with_no_reason_is_still_a_lock() {
        let listed = parse_list("worktree /r/w\nHEAD abc\nlocked\n");
        assert!(listed[0].is_locked());
        assert_eq!(listed[0].locked.as_deref(), Some(""));
    }

    #[test]
    fn separate_directories_mean_a_linked_worktree() {
        let main = classify(PathBuf::from("/r/.git"), PathBuf::from("/r/.git"), false);
        assert_eq!(main.kind, Kind::Main);
        assert!(!main.is_linked());

        let linked =
            classify(PathBuf::from("/r/.git/worktrees/w"), PathBuf::from("/r/.git"), false);
        assert!(linked.is_linked());

        let bare = classify(PathBuf::from("/r.git"), PathBuf::from("/r.git"), true);
        assert_eq!(bare.kind, Kind::Bare);
    }
}
