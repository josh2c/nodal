//! What `nodal adopt --all` answers with.
//!
//! Each row is one worktree `git worktree list` named. The summary counts what became a
//! unit, what was skipped, and what failed. The main checkout and an already-adopted
//! worktree are skips, not failures.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::output::Render;
use crate::output::human::{Block, Doc, Table};

/// How one worktree of `nodal adopt --all` ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum AdoptOutcome {
    /// The worktree is now a unit where it stands.
    Adopted {
        /// The handle the unit was given.
        slug: String,
    },
    /// The row was the main checkout, or it was already a unit.
    Skipped {
        /// Why it was not adopted, in the words the report prints.
        reason: String,
    },
    /// The existing adopt path refused this row for another reason.
    Failed {
        /// What the refusal said.
        reason: String,
    },
}

/// One worktree `nodal adopt --all` considered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptedRow {
    /// Where the worktree is.
    pub path: PathBuf,
    /// What happened to it.
    pub outcome: AdoptOutcome,
}

impl AdoptedRow {
    /// The worktree became a unit with this handle.
    #[must_use]
    pub fn adopted(path: PathBuf, slug: String) -> Self {
        Self { path, outcome: AdoptOutcome::Adopted { slug } }
    }

    /// The worktree was left alone, and the report says why.
    #[must_use]
    pub fn skipped(path: PathBuf, reason: impl Into<String>) -> Self {
        Self { path, outcome: AdoptOutcome::Skipped { reason: reason.into() } }
    }

    /// The existing adopt path refused this worktree.
    #[must_use]
    pub fn failed(path: PathBuf, reason: impl Into<String>) -> Self {
        Self { path, outcome: AdoptOutcome::Failed { reason: reason.into() } }
    }

    /// Whether this row stopped the command from succeeding.
    #[must_use]
    pub const fn is_failed(&self) -> bool {
        matches!(self.outcome, AdoptOutcome::Failed { .. })
    }

    fn cells(&self) -> Vec<String> {
        vec![self.path.display().to_string(), self.result_cell()]
    }

    fn result_cell(&self) -> String {
        match &self.outcome {
            AdoptOutcome::Adopted { slug } => format!("adopted {slug}"),
            AdoptOutcome::Skipped { reason } => format!("skipped {reason}"),
            AdoptOutcome::Failed { reason } => format!("failed {reason}"),
        }
    }
}

/// Every worktree of the project, one row each, and a count of what happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptedAll {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// One row per worktree `git worktree list` named, in that order.
    pub rows: Vec<AdoptedRow>,
}

impl AdoptedAll {
    /// Whether any worktree failed to become a unit.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.rows.iter().any(AdoptedRow::is_failed)
    }

    fn counts(&self) -> (usize, usize, usize) {
        let mut adopted = 0;
        let mut skipped = 0;
        let mut failed = 0;
        for row in &self.rows {
            match row.outcome {
                AdoptOutcome::Adopted { .. } => adopted += 1,
                AdoptOutcome::Skipped { .. } => skipped += 1,
                AdoptOutcome::Failed { .. } => failed += 1,
            }
        }
        (adopted, skipped, failed)
    }

    fn summary(&self) -> String {
        let (adopted, skipped, failed) = self.counts();
        let summary = format!(
            "adopted {} {}; skipped {}",
            adopted,
            if adopted == 1 { "worktree" } else { "worktrees" },
            skipped
        );
        if failed == 0 {
            return summary;
        }
        format!("{summary}; {failed} failed")
    }
}

impl Render for AdoptedAll {
    const KIND: &'static str = "adopted worktrees";

    fn doc(&self) -> Doc {
        let mut table = Table::new(&["worktree", "result"]);
        for row in &self.rows {
            table.push(row.cells());
        }
        Doc::from_iter([Block::table(table), Block::line(self.summary())])
    }
}

/// The exact command reclaim prints for a done adopted worktree.
#[must_use]
pub fn removal_command(path: &Path) -> String {
    format!("git worktree remove {}", path.display())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{AdoptedAll, AdoptedRow};
    use crate::model::Timestamp;
    use crate::output::Render;
    use std::path::PathBuf;

    #[test]
    fn a_summary_counts_worktrees_and_names_skips() {
        let report = AdoptedAll {
            now: Timestamp::parse("2026-09-07T09:00:00Z").unwrap(),
            rows: vec![
                AdoptedRow::skipped(PathBuf::from("/p"), "the main checkout"),
                AdoptedRow::adopted(PathBuf::from("/p/a"), String::from("alpha")),
                AdoptedRow::adopted(PathBuf::from("/p/b"), String::from("beta")),
                AdoptedRow::skipped(PathBuf::from("/p/c"), "already a unit"),
            ],
        };
        let text = report.doc().to_string();
        assert!(text.contains("adopted 2 worktrees; skipped 2"), "{text}");
        assert!(text.contains("skipped the main checkout"), "{text}");
        assert!(text.contains("adopted alpha"), "{text}");
        assert!(!report.failed());
    }
}
