//! Why a unit of work needs a person, ranked.
//!
//! One enum, in `model/` rather than beside either of its readers, because two commands
//! answer with it and a person is meant to read one word in both places and understand
//! one thing. `nodal ls` prints it in a column, decided from the readings a list already
//! takes; `nodal reclaim --check` produces the first three of them with a proof behind
//! each ([`crate::lifecycle::assess`]). A second enum here would let those two drift.
//!
//! The order of the variants is the ranking, and [`Ord`] is derived from it, so "the top
//! reason" is `min` and nothing anywhere sorts these by hand.

use serde::{Deserialize, Serialize};

/// Why a unit needs a person, most actionable first.
///
/// The order of the variants is the ranking, and [`Ord`] is derived from it, so "the top
/// reason" is `min` and nothing anywhere sorts these by hand.
///
/// One enum serves two readers. The preflight emits the first three, which are the three
/// a reclaim refuses over; `nodal ls` emits all six, because its question is which unit
/// to open next rather than which unit is safe to end. A reader that saw two enums here
/// would have to be told that `unique_loss` in one is `unique_loss` in the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Work that may exist only here: uncommitted paths, or commits nothing else holds.
    UniqueLoss,
    /// Something Nodal did not start is standing in the home, so the home cannot move.
    BlockingRuntime,
    /// Nothing on this machine read the remote, so what it has is not known.
    UnknownEvidence,
    /// Merging would conflict, or the base has moved a long way under the branch.
    Diverged,
    /// The work is done, or is out for review, and the unit is a person's to end.
    Review,
    /// Nothing.
    #[default]
    Nothing,
}

impl Needs {
    /// The word a report prints for it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::UniqueLoss => "unique loss",
            Self::BlockingRuntime => "blocked",
            Self::UnknownEvidence => "unknown",
            Self::Diverged => "diverged",
            Self::Review => "review",
            Self::Nothing => "—",
        }
    }

    /// Whether a reclaim refuses rather than going ahead for this reason.
    ///
    /// The three that do are the three a reclaim already refuses over today: the
    /// uniqueness check's findings, and a process standing in the home it would move.
    #[must_use]
    pub const fn refuses(self) -> bool {
        matches!(self, Self::UniqueLoss | Self::BlockingRuntime | Self::UnknownEvidence)
    }
}
