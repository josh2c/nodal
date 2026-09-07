//! Worktree detection, used only when adopting an existing checkout in place.
//!
//! Nodal never creates worktrees: a unit is an independent copy-on-write clone of a
//! base. A linked worktree is still something we must recognise, because operations that
//! are safe in an ordinary repository (removing `worktrees/`, deleting the Git directory)
//! would damage the repository it is linked to.

use std::path::PathBuf;

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

    use super::{Kind, classify};

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
