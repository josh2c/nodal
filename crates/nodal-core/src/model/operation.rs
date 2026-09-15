//! How far a run of an operation got.
//!
//! The state of a run is a fact about a row of the registry, like a unit's status and an
//! environment's, so it lives here with them rather than inside the module that writes
//! it. A report reads it and says what it means ([`crate::output::view`]); the runner
//! writes it ([`crate::lifecycle`]); neither has to turn it into a word to hand it to
//! the other.

use serde::{Deserialize, Serialize};

/// How far an operation got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Started, and not yet finished by the process that started it.
    Running,
    /// Finished: every step applied and the registry write committed.
    Committed,
    /// Undone: nothing it did is left.
    RolledBack,
    /// An undo failed. Something it did is still out there, and every invocation says
    /// so until a person deals with it.
    Failed,
}
