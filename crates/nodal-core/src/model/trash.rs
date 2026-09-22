//! A home that has been reclaimed: where it went, and when `nodal gc` may remove it.
//!
//! Reclaiming does not delete. It moves the home to the project's trash directory and
//! writes one of these rows, and the row is what makes the directory findable again:
//! a person who wants the work back reads the path and the snapshot ref out of it
//! rather than searching the filesystem for something that looks like their unit.
//!
//! `expires_at` is decided when the home is trashed rather than when `gc` runs. The
//! retention a project asked for is a promise made at the moment the work was taken
//! away, and a recipe edited a week later must not shorten a window somebody is
//! relying on.
//!
//! [`Trashed::rested`] is the other half of that promise. A reclaim goes ahead over
//! commits that exist somewhere else, and where that somewhere else is another
//! repository on this disk the verdict is only as true as that repository. The copy can
//! go after the home is in the trash — somebody deletes a branch in a clone, a remote
//! drops a merged branch — and until `gc` reads the home again nothing says the trash
//! now holds the only copy. So the reclaim writes down what it rested on, and
//! [`crate::lifecycle::ops::gc`] reads the home again before it removes the
//! directory and names what is gone.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::ids::{EnvId, ProjectId, UnitId};
use crate::model::timestamp::Timestamp;
use crate::model::unit::Slug;

/// One repository outside a home that held a copy of the home's commits.
///
/// Read once, when the home is trashed, and never read again as evidence: `gc` proves a
/// copy by asking the repositories on this disk again, not by believing this row. What
/// this carries is the name of what the verdict rested on, so that a sweep which finds
/// the copy gone can say which copy that was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Outside {
    /// Where the repository is.
    pub repository: PathBuf,
    /// The refs there that reached those commits, as that repository spells them. A
    /// sample of at most ten, in the order Git listed them.
    pub references: Vec<String>,
    /// How many of the home's commits it covered. Exact.
    pub commits: usize,
}

impl Outside {
    /// The repository and its refs as one clause, for the line a report prints.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.references.is_empty() {
            return self.repository.display().to_string();
        }
        format!("{} ({})", self.repository.display(), self.references.join(", "))
    }
}

/// What a reclaim's uniqueness check decided about the home it trashed.
///
/// Three answers and not two, because "nothing was written down" is a third fact and
/// reading it as either of the others would be a claim this machine did not make.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rested {
    /// The row says nothing, which is what a row an older Nodal wrote says. `gc` reads
    /// the home again and believes only what that reading shows it.
    #[default]
    Unrecorded,
    /// The check found nothing that existed only in the home. `copies` names the
    /// repositories and refs that held the commits it did not refuse over, and it is
    /// empty for a home that had no commit of its own to hold.
    Safe {
        /// What held them, one entry per repository.
        copies: Vec<Outside>,
    },
    /// The check found work that existed only in the home and `--force` went on. The
    /// person looked and said so, so the retention running out removes the directory
    /// as it always did.
    Forced,
}

impl Rested {
    /// Whether `gc` must read the home again before the retention may remove it.
    ///
    /// A forced reclaim is the one answer that does not ask: the loss was named, printed
    /// and accepted before the home was moved, and a sweep that refused to act on it
    /// would keep every forced reclaim's home for ever and make `--force` mean nothing.
    #[must_use]
    pub const fn re_asks(&self) -> bool {
        !matches!(self, Self::Forced)
    }

    /// What the verdict rested on, which is nothing for the other two answers.
    #[must_use]
    pub fn copies(&self) -> &[Outside] {
        match self {
            Self::Safe { copies } => copies,
            Self::Unrecorded | Self::Forced => &[],
        }
    }
}

/// One reclaimed home, kept until it expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Trashed {
    /// The materialisation whose home this was. One row per environment: an
    /// environment is reclaimed once.
    pub environment_id: EnvId,
    /// The unit it belonged to.
    pub unit_id: UnitId,
    /// The project that unit belongs to, so `gc` can sweep one project's trash.
    pub project_id: ProjectId,
    /// The unit's handle, kept here so a listing reads without a join to a unit row.
    pub slug: Slug,
    /// Where the home was while it was live.
    pub home: PathBuf,
    /// Where it is now.
    pub path: PathBuf,
    /// The ref a `--force` reclaim committed the work to, inside the trashed
    /// repository. `None` when the home was clean and nothing had to be preserved.
    pub snapshot: Option<String>,
    /// What the prune dropped from the copy on its way in: the build output and the
    /// installed dependencies the home was keeping warm, in bytes.
    ///
    /// Zero is the honest answer for a home that held none, for one an ignore rule
    /// covered none of, and for a row written before Nodal pruned anything at all.
    /// Defaulted on the way in for that last case: a journalled reclaim from an older
    /// Nodal has no such field and still has to be rebuilt and finished.
    #[serde(default)]
    pub pruned_bytes: u64,
    /// What the uniqueness check decided, and what it rested on.
    ///
    /// Defaulted on the way in: a row written before this field says nothing, which is
    /// [`Rested::Unrecorded`] and not a claim that the home was safe.
    #[serde(default)]
    pub rested: Rested,
    /// When it was moved.
    pub trashed_at: Timestamp,
    /// The first instant `nodal gc` may remove it.
    pub expires_at: Timestamp,
}

impl Trashed {
    /// Whether `gc` may remove this now.
    #[must_use]
    pub fn has_expired(&self, now: Timestamp) -> bool {
        self.expires_at.unix_seconds() <= now.unix_seconds()
    }
}

/// The instant a retention of `days` from `at` runs out.
///
/// Saturating, because the alternative is a panic in a clock calculation: a retention
/// long enough to overflow a signed second count is a typo, and the answer to a typo is
/// "a very long time", not a crash in the middle of a reclaim.
#[must_use]
pub fn expiry(at: Timestamp, days: u32) -> Timestamp {
    const DAY: i64 = 24 * 60 * 60;
    let seconds = i64::from(days).saturating_mul(DAY);
    Timestamp::from_unix_seconds(at.unix_seconds().saturating_add(seconds)).unwrap_or(at)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Outside, Rested, expiry};
    use crate::model::Timestamp;

    /// One repository, for the properties about the words.
    fn held(references: &[&str]) -> Outside {
        Outside {
            repository: PathBuf::from("/w/project"),
            references: references.iter().map(|name| (*name).to_owned()).collect(),
            commits: 2,
        }
    }

    /// The clause names the repository and the refs in it, which is what a person needs
    /// to go and look.
    #[test]
    fn a_copy_names_the_repository_and_the_refs_in_it() {
        assert_eq!(
            held(&["refs/heads/topic", "refs/remotes/origin/topic"]).describe(),
            "/w/project (refs/heads/topic, refs/remotes/origin/topic)"
        );
    }

    /// A repository that held the commits under no name this run could read still held
    /// them. The clause says the repository and claims no ref.
    #[test]
    fn a_copy_with_no_ref_read_names_the_repository_alone() {
        assert_eq!(held(&[]).describe(), "/w/project");
    }

    /// A forced reclaim named its loss, printed it and went on, so the retention
    /// running out removes the directory as it always did.
    #[test]
    fn only_a_forced_reclaim_is_taken_without_asking_again() {
        assert!(!Rested::Forced.re_asks());
        assert!(Rested::Unrecorded.re_asks());
        assert!(Rested::Safe { copies: Vec::new() }.re_asks());
    }

    /// The other two answers rested on nothing this machine wrote down.
    #[test]
    fn only_a_safe_verdict_names_what_it_rested_on() {
        assert_eq!(Rested::Safe { copies: vec![held(&[])] }.copies(), &[held(&[])]);
        assert!(Rested::Forced.copies().is_empty());
        assert!(Rested::Unrecorded.copies().is_empty());
    }

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    #[test]
    fn a_retention_of_days_is_that_many_days_later() {
        let start = at("2026-09-07T09:00:00Z");
        assert_eq!(expiry(start, 14), at("2026-09-21T09:00:00Z"));
    }

    #[test]
    fn a_retention_of_nothing_expires_at_once() {
        let start = at("2026-09-07T09:00:00Z");
        assert_eq!(expiry(start, 0), start);
    }

    #[test]
    fn a_retention_no_clock_can_hold_is_a_long_time_rather_than_a_panic() {
        let start = at("2026-09-07T09:00:00Z");
        assert!(expiry(start, u32::MAX).unix_seconds() >= start.unix_seconds());
    }
}
