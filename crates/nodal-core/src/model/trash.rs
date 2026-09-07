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

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::ids::{EnvId, ProjectId, UnitId};
use crate::model::timestamp::Timestamp;
use crate::model::unit::Slug;

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

    use super::expiry;
    use crate::model::Timestamp;

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
