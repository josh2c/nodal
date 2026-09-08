//! When a unit changes state on its own, and the two signals that decide it.
//!
//! Most of a unit's life is moved by a command: `nodal new` opens one, `nodal done`
//! puts one up for review. One transition has no command, because the thing that causes
//! it happens somewhere else entirely — a reviewer presses a button on a website — and
//! the first Nodal hears of it is that the world has changed under a unit it is
//! listing.
//!
//! That transition is **merged**, and it takes two signals rather than one:
//!
//! - the work is **integrated**: the branch it merges into carries every change of the
//!   branch. This is the list's own verdict ([`crate::git::Integration`]), read from
//!   trees, so a squash merge and a rebase both count — which is the whole point, since
//!   neither leaves a commit of the unit on the base;
//! - the branch is **contained**: every commit of it is on a remote
//!   ([`crate::git::remote::Containment`]).
//!
//! Neither alone is enough, and the failure each one prevents is a different one.
//! Integration alone would call a unit merged the moment somebody rebased the base
//! under it locally, before anything left the machine. Containment alone would call a
//! unit merged as soon as it was pushed, which is the state before review, not after
//! it. Together they say what a person means by merged: the work is on the base, and it
//! got there somewhere other people can see.
//!
//! The flip is **recorded**, not just shown. A unit that reads as merged today and is
//! then reclaimed, or whose base moves again, would otherwise read as something else
//! tomorrow, and the retention window `nodal gc` measures from has to start somewhere.
//! Writing it down is what makes the state a fact of the registry rather than a
//! rendering of whatever Git happened to say the last time somebody looked.

use rusqlite::Connection;

use crate::Result;
use crate::git::Integration;
use crate::git::remote::Containment;
use crate::model::{Timestamp, Unit, UnitStatus};
use crate::store::units;

/// Whether these two readings say the unit's work has landed.
///
/// A unit already merged or archived is not flipped again: the answer is about a
/// transition, and both of those are past it.
#[must_use]
pub fn is_merged(status: UnitStatus, integration: Integration, contained: &Containment) -> bool {
    matches!(status, UnitStatus::Open | UnitStatus::Review)
        && integration.is_integrated()
        && !contained.remotes.is_empty()
        && contained.is_contained()
}

/// Write the flip down, and say whether this call is the one that made it.
///
/// # Errors
/// [`Error::Store`] when the registry could not be written.
pub fn record_merged(conn: &Connection, unit: &Unit, now: Timestamp) -> Result<bool> {
    units::update_status(conn, unit.id, UnitStatus::Merged, now)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "tests fail by panicking")]

    use super::is_merged;
    use crate::git::integration::Reason;
    use crate::git::remote::Containment;
    use crate::git::{Integration, Oid};
    use crate::model::UnitStatus;

    fn containment(remotes: &[&str], unpushed: usize) -> Containment {
        let oid = Oid::parse(&"a".repeat(40)).expect("forty hex characters are an object id");
        Containment {
            remotes: remotes.iter().map(|name| (*name).to_owned()).collect(),
            unpushed: std::iter::repeat_n(oid, unpushed).collect(),
        }
    }

    #[test]
    fn work_that_is_on_the_base_and_on_a_remote_is_merged() {
        assert!(is_merged(
            UnitStatus::Review,
            Integration::Integrated(Reason::Absorbed),
            &containment(&["origin"], 0)
        ));
    }

    #[test]
    fn a_squash_merge_counts_the_same_as_an_ordinary_one() {
        for reason in [Reason::Absorbed, Reason::Ancestor] {
            assert!(is_merged(
                UnitStatus::Open,
                Integration::Integrated(reason),
                &containment(&["origin"], 0)
            ));
        }
    }

    #[test]
    fn work_nobody_else_has_is_not_merged_however_integrated_it_looks() {
        let integrated = Integration::Integrated(Reason::Absorbed);
        assert!(!is_merged(UnitStatus::Open, integrated, &containment(&["origin"], 2)));
        assert!(!is_merged(UnitStatus::Open, integrated, &containment(&[], 0)));
    }

    #[test]
    fn a_branch_the_base_does_not_carry_is_not_merged_however_pushed_it_is() {
        for verdict in [Integration::Open, Integration::Conflict, Integration::Unknown] {
            assert!(!is_merged(UnitStatus::Open, verdict, &containment(&["origin"], 0)));
        }
    }

    #[test]
    fn a_unit_that_is_already_past_this_is_not_flipped_again() {
        let integrated = Integration::Integrated(Reason::Ancestor);
        for status in [UnitStatus::Merged, UnitStatus::Archived] {
            assert!(!is_merged(status, integrated, &containment(&["origin"], 0)));
        }
    }
}
