//! When a unit changes state on its own, and the two signals that decide it.
//!
//! Most of a unit's life is moved by a command: `nodal new` opens one, `nodal done`
//! puts one up for review. One transition has no command, because the thing that causes
//! it happens somewhere else entirely — a reviewer presses a button on a website — and
//! the first Nodal hears of it is that the world has changed under a unit it is
//! listing.
//!
//! That transition is **merged**, and it takes three signals:
//!
//! - the branch has **landed something**: it is ahead of the base, so it carries at
//!   least one commit the base does not. A branch ahead of nothing has contributed
//!   nothing that was not already the base's;
//! - the work is **integrated**: the branch it merges into carries every change of the
//!   branch. This is the list's own verdict ([`crate::git::Integration`]), read from
//!   trees, so a squash merge and a rebase both count — which is the whole point, since
//!   neither leaves a commit of the unit on the base;
//! - the branch is **contained**: every commit of it is on a remote
//!   ([`crate::git::remote::Containment`]).
//!
//! None alone is enough, and the failure each one prevents is a different one.
//! Integration alone would call a unit merged the moment somebody rebased the base
//! under it locally, before anything left the machine. Containment alone would call a
//! unit merged as soon as it was pushed, which is the state before review, not after
//! it. Together they say what a person means by merged: the work is on the base, and it
//! got there somewhere other people can see.
//!
//! ## Why the first signal is needed, and why it is `ahead > 0`
//!
//! Without it every unit is merged the moment it is made. A unit `nodal new` has just
//! created is on the base's own commit: it is `integrated (ancestor)` because its tip is
//! in the base's history, and it is contained by every remote because the commits it is
//! made of are the project's, already pushed. Both readings are true and neither is
//! about this unit. The flip then starts the retention `nodal gc` measures, and one
//! sweep later the home of a unit nobody had begun is reclaimed on a state that was
//! never true.
//!
//! `ahead > 0` is the exact question "did this branch contribute anything". It keeps
//! the case that must be kept: a unit whose commits were **squash-absorbed** or rebased
//! into the base still has those commits, and only those commits, on its own branch, so
//! it is ahead of the base by construction — `Reason::Absorbed` is only ever reached
//! for a branch that is ahead ([`crate::git::integration::standing`]). What it drops is
//! `Reason::Ancestor`, which is the same shape as a unit that never committed: in both
//! the branch tip is in the base's history and the branch is ahead by nothing.
//!
//! That is a real cost and it is taken deliberately. A unit merged with an ordinary
//! merge commit *is* an ancestor of the base afterwards, and it is not flipped. It stays
//! in `review` and a person reclaims it themselves, which is the direction to fail in:
//! this flip is the start of a clock that ends in a home being taken away.
//!
//! It is also the only honest answer available here. Git alone cannot tell the two
//! ancestor cases apart — the branch tip is in the base's history either way — and
//! Nodal keeps no record of the commit a branch forked at that survives a rebase.
//! Telling them apart needs one, and recording one is a model change and its own task.
//!
//! The flip is **recorded**, not just shown. A unit that reads as merged today and is
//! then reclaimed, or whose base moves again, would otherwise read as something else
//! tomorrow, and the retention window `nodal gc` measures from has to start somewhere.
//! Writing it down is what makes the state a fact of the registry rather than a
//! rendering of whatever Git happened to say the last time somebody looked.

use rusqlite::Connection;

use crate::Result;
use crate::git::remote::Containment;
use crate::git::{Divergence, Integration};
use crate::model::{Timestamp, Unit, UnitStatus};
use crate::store::units;

/// Whether the branch has work of its own that the base has taken.
///
/// The cheap half of the answer, and the one a caller asks first: it is read from what
/// the list has already measured, so a unit that cannot have landed anything costs no
/// further Git call.
#[must_use]
pub const fn has_landed(integration: Integration, divergence: Divergence) -> bool {
    integration.is_integrated() && divergence.ahead > 0
}

/// Whether these readings say the unit's work has landed somewhere other people see.
///
/// A unit already merged or archived is not flipped again: the answer is about a
/// transition, and both of those are past it.
#[must_use]
pub fn is_merged(
    status: UnitStatus,
    integration: Integration,
    divergence: Divergence,
    contained: &Containment,
) -> bool {
    matches!(status, UnitStatus::Open | UnitStatus::Review)
        && has_landed(integration, divergence)
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
    use crate::git::{Divergence, Integration, Oid};
    use crate::model::UnitStatus;

    fn containment(remotes: &[&str], unpushed: usize) -> Containment {
        let oid = Oid::parse(&"a".repeat(40)).expect("forty hex characters are an object id");
        Containment {
            remotes: remotes.iter().map(|name| (*name).to_owned()).collect(),
            unpushed: std::iter::repeat_n(oid, unpushed).collect(),
        }
    }

    /// A branch with commits of its own, which is every branch that has done anything.
    const fn ahead(commits: u32) -> Divergence {
        Divergence { ahead: commits, behind: 0 }
    }

    #[test]
    fn work_that_is_on_the_base_and_on_a_remote_is_merged() {
        assert!(is_merged(
            UnitStatus::Review,
            Integration::Integrated(Reason::Absorbed),
            ahead(1),
            &containment(&["origin"], 0)
        ));
    }

    /// The case the whole predicate is shaped around: a squash merge leaves the unit's
    /// commits on its own branch and only the changes on the base, so the branch is
    /// still ahead and must still count as merged.
    #[test]
    fn a_squash_absorbed_unit_is_merged_although_no_commit_of_it_is_on_the_base() {
        assert!(is_merged(
            UnitStatus::Review,
            Integration::Integrated(Reason::Absorbed),
            ahead(3),
            &containment(&["origin"], 0)
        ));
    }

    /// The defect this predicate exists to stop.
    ///
    /// A unit `nodal new` has just made sits on the base's own commit. Its tip is in the
    /// base's history, so the verdict is `integrated (ancestor)`; the commits it is made
    /// of are the project's and are already pushed, so every remote contains it. Both
    /// readings are true and neither is about this unit, and flipping it would start the
    /// clock that ends in its home being reclaimed.
    #[test]
    fn a_unit_that_has_committed_nothing_is_never_merged() {
        let nothing = Divergence { ahead: 0, behind: 0 };
        let moved_on = Divergence { ahead: 0, behind: 12 };
        for divergence in [nothing, moved_on] {
            assert!(!is_merged(
                UnitStatus::Open,
                Integration::Integrated(Reason::Ancestor),
                divergence,
                &containment(&["origin"], 0)
            ));
        }
    }

    #[test]
    fn work_nobody_else_has_is_not_merged_however_integrated_it_looks() {
        let integrated = Integration::Integrated(Reason::Absorbed);
        assert!(!is_merged(UnitStatus::Open, integrated, ahead(1), &containment(&["origin"], 2)));
        assert!(!is_merged(UnitStatus::Open, integrated, ahead(1), &containment(&[], 0)));
    }

    #[test]
    fn a_branch_the_base_does_not_carry_is_not_merged_however_pushed_it_is() {
        for verdict in [Integration::Open, Integration::Conflict, Integration::Unknown] {
            assert!(!is_merged(UnitStatus::Open, verdict, ahead(1), &containment(&["origin"], 0)));
        }
    }

    #[test]
    fn a_unit_that_is_already_past_this_is_not_flipped_again() {
        let integrated = Integration::Integrated(Reason::Absorbed);
        for status in [UnitStatus::Merged, UnitStatus::Archived] {
            assert!(!is_merged(status, integrated, ahead(1), &containment(&["origin"], 0)));
        }
    }
}
