//! One file per lifecycle operation: what its plan is, and how it is rebuilt.
//!
//! An operation composes the other modules and holds no mechanism of its own. The
//! runner is [`crate::lifecycle::run`]; what an operation contributes is the list of
//! steps, the registry write that finishes them, and a [`Rebuild`] so that a run of it
//! interrupted by a kill can be found again from the journal.

pub mod gc;
pub mod merge;
pub mod new;
pub mod reclaim;

use crate::lifecycle::Rebuild;

/// Every operation this build can rebuild an interrupted run of.
///
/// This is the table [`crate::lifecycle::resolve`] is given, and it is the reason
/// resolving is not a match arm inside the runner: a build that does not know an
/// operation reports it rather than guessing at it, and adding an operation is adding
/// a line here.
#[must_use]
pub fn rebuilders() -> Vec<&'static dyn Rebuild> {
    vec![&new::New, &merge::Merge, &reclaim::Reclaim, &crate::substrate::build::BaseBuild]
}
