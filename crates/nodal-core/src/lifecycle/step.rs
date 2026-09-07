//! What an operation is made of: idempotent steps, and the registry write that ends it.
//!
//! A [`Plan`] is a value. Working out which steps an operation needs is pure and
//! testable without touching a disk; [`crate::lifecycle::run`] is the only part that
//! acts. That split is what lets the same plan be rebuilt later by a different process
//! and undone step by step (`docs/code-structure.md`).

use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

/// One unit of work in a [`Plan`], with the change that takes it back.
///
/// Both halves are idempotent, and that is a requirement rather than a nicety. A run
/// interrupted mid-`apply` is journalled as having reached the step but not finished
/// it, and the next invocation will either apply it again or undo it without knowing
/// how far the first attempt got. `apply` must therefore accept a world it has already
/// changed, and `undo` a world it never changed.
///
/// A step takes `&self`, so it cannot pass a value to the step after it. Everything an
/// operation needs — generated identifiers included — is decided while the plan is
/// built, which is what makes the plan reproducible from the journal.
pub trait Step {
    /// What this step is called in the journal.
    ///
    /// Stable for a given plan: the key is how a rebuilt plan is lined up with the
    /// record of the run that was interrupted, so it must not carry a clock reading or
    /// anything else that differs between two builds of the same plan.
    fn key(&self) -> String;

    /// Bring the world to the state this step is responsible for.
    ///
    /// # Errors
    /// Whatever the work itself reports. A failure stops the operation and the steps
    /// already applied are undone.
    fn apply(&self) -> Result<()>;

    /// Take the world back to the state it was in before [`Step::apply`].
    ///
    /// # Errors
    /// Whatever the work itself reports. A failure leaves the operation `failed` in
    /// the journal, and every later invocation reports it until a person clears it.
    fn undo(&self) -> Result<()>;
}

/// What to do with an operation whose process died before it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    /// Carry on from where it stopped. For operations whose steps are expensive to
    /// redo — building a base, restoring a snapshot — and whose half-done state is
    /// harmless to leave in place for the moments before the next command.
    Resume,
    /// Undo what was applied and leave nothing behind. The default, and what every
    /// operation that creates something a user can see should choose: a home directory
    /// that exists but has no registry row is worse than no home directory.
    RollBack,
}

/// The registry write that finishes an operation.
///
/// It runs inside one transaction together with the journal's own move to `committed`,
/// so the rows an operation adds and the record that it finished cannot come apart.
pub type Commit = Box<dyn Fn(&Transaction<'_>) -> Result<()>>;

/// An operation as a list of steps and the registry write that finishes it.
///
/// `kind` and `params` are what a later process rebuilds this plan from
/// ([`crate::lifecycle::Rebuild`]), so `params` must hold everything the plan was built
/// out of — including the identifiers it generated.
pub struct Plan {
    /// Which operation this is: `new`, `reclaim`, and so on. Matches the `kind` of the
    /// [`crate::lifecycle::Rebuild`] that can rebuild it.
    pub kind: &'static str,
    /// What the operation acts on, in the words a report uses. For a person reading
    /// "rolled back new (fix-worker-import)", this is the part in brackets.
    pub subject: String,
    /// The plan's own input, as JSON. Written to the journal and handed back to
    /// [`crate::lifecycle::Rebuild::rebuild`] unchanged.
    pub params: Value,
    /// What to do with a run of this plan that was interrupted.
    pub recovery: Recovery,
    /// The work, in the order it is applied and the reverse of the order it is undone.
    pub steps: Vec<Box<dyn Step>>,
    /// The registry rows the operation adds, written once at the end.
    pub commit: Commit,
}

impl Plan {
    /// A plan that rolls back if it is interrupted, which is what almost every
    /// operation wants.
    #[must_use]
    pub fn new(kind: &'static str, subject: String, params: Value, commit: Commit) -> Self {
        Self { kind, subject, params, recovery: Recovery::RollBack, steps: Vec::new(), commit }
    }

    /// Add a step to the end of the plan.
    #[must_use]
    pub fn then(mut self, step: impl Step + 'static) -> Self {
        self.steps.push(Box::new(step));
        self
    }

    /// Choose what happens to an interrupted run of this plan.
    #[must_use]
    pub fn recovering(mut self, recovery: Recovery) -> Self {
        self.recovery = recovery;
        self
    }

    /// The key of every step, in order. What the journal records.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.steps.iter().map(|step| step.key()).collect()
    }
}

impl core::fmt::Debug for Plan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Plan")
            .field("kind", &self.kind)
            .field("subject", &self.subject)
            .field("recovery", &self.recovery)
            .field("steps", &self.keys())
            .finish_non_exhaustive()
    }
}
