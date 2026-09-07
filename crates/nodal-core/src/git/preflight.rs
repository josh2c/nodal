//! Refuse repositories with Git operations in progress.
//!
//! Cloning, adopting or scrubbing a repository that is mid-merge or mid-rebase copies a
//! half-finished operation into a unit, where the user cannot finish it. The markers are
//! a table, not code (`docs/code-structure.md`): each is a path under the Git directory.

use std::path::{Path, PathBuf};

/// A Git operation that is under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A merge stopped for conflict resolution.
    Merge,
    /// A rebase is in progress.
    Rebase,
    /// A cherry-pick stopped for conflict resolution.
    CherryPick,
    /// A revert stopped for conflict resolution.
    Revert,
    /// A bisect session is open.
    Bisect,
    /// `git am` is applying a patch series.
    ApplyPatch,
    /// Another process holds the index lock.
    IndexLock,
}

impl State {
    /// The marker whose presence under the Git directory means this state.
    #[must_use]
    pub fn marker(self) -> &'static str {
        MARKERS.iter().find(|(state, _)| *state == self).map_or("", |(_, marker)| *marker)
    }
}

/// Marker paths, relative to the Git directory. Order is the order they are reported in.
const MARKERS: &[(State, &str)] = &[
    (State::Merge, "MERGE_HEAD"),
    (State::Rebase, "rebase-merge"),
    (State::Rebase, "rebase-apply/rebasing"),
    (State::ApplyPatch, "rebase-apply/applying"),
    (State::CherryPick, "CHERRY_PICK_HEAD"),
    (State::Revert, "REVERT_HEAD"),
    (State::Bisect, "BISECT_LOG"),
    (State::IndexLock, "index.lock"),
];

/// What a preflight found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The Git directory that was inspected.
    pub git_dir: PathBuf,
    /// Every state whose marker is present, in table order and without repeats.
    pub states: Vec<State>,
}

impl Report {
    /// Whether the repository is safe to clone, scrub or adopt.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.states.is_empty()
    }
}

/// Inspect `git_dir` for in-progress markers. Pure but for the existence checks.
#[must_use]
pub(super) fn inspect(git_dir: &Path) -> Report {
    let mut states = Vec::new();
    for (state, marker) in MARKERS {
        if git_dir.join(marker).exists() && !states.contains(state) {
            states.push(*state);
        }
    }
    Report { git_dir: PathBuf::from(git_dir), states }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{MARKERS, State};

    #[test]
    fn every_state_has_a_marker_and_no_marker_repeats() {
        for (state, marker) in MARKERS {
            assert!(!marker.is_empty());
            assert!(!state.marker().is_empty());
        }
        assert_eq!(State::Merge.marker(), "MERGE_HEAD");
        let mut paths: Vec<&str> = MARKERS.iter().map(|(_, marker)| *marker).collect();
        paths.sort_unstable();
        let count = paths.len();
        paths.dedup();
        assert_eq!(paths.len(), count);
    }
}
