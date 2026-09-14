//! Locks: who may write a unit, and from when.
//!
//! A lock is about a unit's home. It is never about a checkout, because a project row
//! hands each caller the project standing in their own checkout, so a hold that named
//! a checkout would be a hold on one person's working copy.
//!
//! The lock is advisory. Nodal refuses its own verbs to a second actor and stops
//! nothing else: an editor opens, `git` runs, and a process starts in the home as
//! before. What the lock prevents is two writers using Nodal's own operations on one
//! home at the same time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::actor::Actor;
use crate::model::environment::HostName;
use crate::model::ids::UnitId;
use crate::model::timestamp::Timestamp;

/// Hours a lock survives with nothing entering the home, when a recipe does not say.
pub const DEFAULT_IDLE_HOURS: u32 = 8;

/// The single-writer claim on a unit.
///
/// Two clocks, and they answer different questions. `expires_at` is the absolute lapse
/// a transfer bundle carries from the host that made it. `refreshed_at` is when an
/// entry last touched the home, and the idle window is measured from it, so a person
/// working all day keeps the lock and a home nobody has entered since yesterday
/// releases it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Lock {
    /// The unit being held.
    pub unit_id: UnitId,
    /// The host that holds the write.
    pub host: HostName,
    /// Who holds it. `None` for a row written before locks carried an actor: such a row
    /// names a host and holds no actor, and the next entry rewrites it.
    pub actor: Option<Actor>,
    /// The process that took the hold. Recorded so a person can look. Nothing signals
    /// it, and a lock is never expired because the process is gone.
    pub pid: Option<u32>,
    /// When the hold began, which a hand-off resets.
    pub taken_at: Timestamp,
    /// When an entry last touched the home.
    pub refreshed_at: Timestamp,
    /// When the claim lapses unless renewed.
    pub expires_at: Timestamp,
}

impl Lock {
    /// Whether the lock has lapsed at `now`, by either clock.
    ///
    /// Two ways to lapse and both count. The absolute lapse is what a bundle from
    /// another host carries. The idle window is what a person on this host meets: a
    /// home nobody has entered for `idle_hours` is nobody's.
    #[must_use]
    pub fn has_lapsed(&self, now: Timestamp, idle_hours: u32) -> bool {
        now >= self.expires_at || now.unix_seconds() >= self.idle_deadline(idle_hours)
    }

    /// The instant the idle window runs out, in seconds since the epoch.
    ///
    /// Saturating, so a window a recipe made absurdly long is a lock that does not lapse
    /// on the clock rather than one whose deadline wrapped into the past.
    #[must_use]
    pub fn idle_deadline(&self, idle_hours: u32) -> i64 {
        let window = i64::from(idle_hours).saturating_mul(3_600);
        self.refreshed_at.unix_seconds().saturating_add(window)
    }

    /// Whether anybody holds this lock at `now`.
    ///
    /// Two ways to hold nobody, and this is the one place both live. A hold the clock
    /// has released is nobody's. So is a row that records no actor: it was written
    /// before locks carried one, it names a host rather than a writer, and refusing
    /// somebody on the strength of it would be a refusal whose reason is that Nodal does
    /// not know who holds it.
    #[must_use]
    pub fn holds_anyone(&self, now: Timestamp, idle_hours: u32) -> bool {
        self.actor.is_some() && !self.has_lapsed(now, idle_hours)
    }

    /// Whether `actor` on `host` is the one holding this lock.
    ///
    /// Both halves are required. The same name on two hosts is two writers, and a row
    /// with no actor is held by nobody, so nobody matches it.
    #[must_use]
    pub fn is_held_by(&self, actor: &Actor, host: &HostName) -> bool {
        &self.host == host && self.actor.as_ref() == Some(actor)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::Lock;
    use crate::model::{Actor, ActorKind, ActorName, HostName, Timestamp, UnitId};

    /// A person at a terminal, by the name they log in as.
    fn actor(name: &str) -> Actor {
        Actor { kind: ActorKind::Human, name: ActorName::parse(name).unwrap() }
    }

    /// A hold taken at `taken`, refreshed at `refreshed`, lapsing absolutely at `until`.
    fn lock(taken: i64, refreshed: i64, until: i64) -> Lock {
        Lock {
            unit_id: "01J9X2K4Q7QW8QG4M2N5B3T6HP".parse::<UnitId>().unwrap(),
            host: HostName::parse("laptop").unwrap(),
            actor: Some(actor("ada")),
            pid: Some(4_120),
            taken_at: Timestamp::from_unix_seconds(taken).unwrap(),
            refreshed_at: Timestamp::from_unix_seconds(refreshed).unwrap(),
            expires_at: Timestamp::from_unix_seconds(until).unwrap(),
        }
    }

    /// The instant `seconds` past the epoch.
    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_seconds(seconds).unwrap()
    }

    #[test]
    fn the_idle_window_runs_from_the_last_entry_and_not_from_the_first() {
        // Taken at zero, entered again eight hours later, with an eight-hour window.
        let held = lock(0, 28_800, 1_000_000);
        assert!(!held.has_lapsed(at(57_599), 8), "the window ran from the first entry");
        assert!(held.has_lapsed(at(57_600), 8), "the window did not run out");
    }

    #[test]
    fn the_absolute_expiry_releases_a_hold_the_idle_window_would_keep() {
        // Entered a moment ago, but the absolute clock a bundle carried has run out.
        let held = lock(0, 100, 200);
        assert!(held.has_lapsed(at(200), 8), "the absolute expiry did not release the hold");
    }

    #[test]
    fn a_hold_is_one_actor_on_one_host() {
        let held = lock(0, 0, 1_000_000);
        let laptop = HostName::parse("laptop").unwrap();
        let desktop = HostName::parse("desktop").unwrap();
        assert!(held.is_held_by(&actor("ada"), &laptop));
        assert!(!held.is_held_by(&actor("bo"), &laptop), "a second actor matched the hold");
        assert!(
            !held.is_held_by(&actor("ada"), &desktop),
            "the same name on a second host matched"
        );
    }

    #[test]
    fn a_row_that_records_no_actor_holds_nobody() {
        let held = Lock { actor: None, ..lock(0, 0, 1_000_000) };
        assert!(!held.holds_anyone(at(1), 8), "a row with no actor held the unit");
        assert!(lock(0, 0, 1_000_000).holds_anyone(at(1), 8), "a live hold held nobody");
    }

    /// A recipe may ask for any number of hours, and the widest one a person can write
    /// must not wrap the deadline into the past.
    ///
    /// The arithmetic saturates rather than wrapping. Nothing a recipe can say reaches
    /// the saturation point, which is the answer: the widest window there is leaves the
    /// hold held, and the absolute expiry is what releases it.
    #[test]
    fn the_widest_window_a_recipe_can_ask_for_does_not_wrap_the_deadline() {
        let held = lock(0, 0, 1_000_000);
        assert!(held.idle_deadline(u32::MAX) > 0, "the deadline wrapped into the past");
        assert!(!held.has_lapsed(at(999_999), u32::MAX), "the widest window lapsed at once");
        assert!(held.has_lapsed(at(1_000_000), u32::MAX), "the absolute expiry was not consulted");
    }
}
