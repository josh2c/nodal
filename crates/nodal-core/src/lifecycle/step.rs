//! What an operation is made of: idempotent steps, and the registry write that ends it.
//!
//! A [`Plan`] is a value. Working out which steps an operation needs is pure and
//! testable without touching a disk; [`crate::lifecycle::run`] is the only part that
//! acts. That split is what lets the same plan be rebuilt later by a different process
//! and undone step by step (`docs/code-structure.md`).

use std::collections::BTreeMap;

use rusqlite::Transaction;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// What one step hands to the registry write that ends the operation.
///
/// JSON, because it is written to the journal and read back by a process that has only
/// the journal: the value a step produced has to survive the death of the process that
/// produced it, and a Rust type does not.
pub type Output = Value;

/// The answer of a step that learned nothing the commit needs.
///
/// Most steps are like this. They change the world and the change is the whole of what
/// they have to say, so there is nothing for the journal to keep.
#[must_use]
pub const fn nothing() -> Output {
    Value::Null
}

/// One unit of work in a [`Plan`], with the change that takes it back.
///
/// Both halves are idempotent, and that is a requirement rather than a nicety. A run
/// interrupted mid-`apply` is journalled as having reached the step but not finished
/// it, and the next invocation will either apply it again or undo it without knowing
/// how far the first attempt got. `apply` must therefore accept a world it has already
/// changed, and `undo` a world it never changed.
///
/// A step takes `&self`, so it cannot pass a value to the step after it. Everything a
/// later step needs — generated identifiers included — is decided while the plan is
/// built, which is what makes the plan reproducible from the journal. What a step
/// *learns* goes the other way: it is returned as an [`Output`], journalled beside the
/// step's own row, and handed to the [`Commit`] as part of the [`Outputs`].
pub trait Step {
    /// What this step is called in the journal.
    ///
    /// Stable for a given plan: the key is how a rebuilt plan is lined up with the
    /// record of the run that was interrupted, so it must not carry a clock reading or
    /// anything else that differs between two builds of the same plan.
    fn key(&self) -> String;

    /// Bring the world to the state this step is responsible for, and say what the
    /// commit needs to know about it.
    ///
    /// The answer is written to the journal in the same statement that records the step
    /// as applied, so a run rebuilt in another process reads back exactly what this
    /// returned. A step with nothing to say returns [`nothing`].
    ///
    /// Idempotence covers the answer as well as the world: a second `apply` of a step
    /// whose work is already done must return the same [`Output`] as the first, because
    /// a resumed run applies it again and the commit must not see a different value.
    ///
    /// # Errors
    /// Whatever the work itself reports. A failure stops the operation and the steps
    /// already applied are undone.
    fn apply(&self) -> Result<Output>;

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

/// What every step of a run produced, by step key.
///
/// This is the framework's answer to a step that has to tell the registry write
/// something: rather than a channel the operation carries between the two, the value
/// goes through the journal, and the commit is handed the whole set. A first run's set
/// is built as the steps apply; a rebuilt run's is read back from the journal and
/// added to by the steps the resume still has to apply. Either way the commit sees the
/// same map, which is the property that makes a resumed run and a first run the same
/// run.
///
/// Keys are [`Step::key`]s, which a plan is required to keep stable across two builds
/// of itself, so the map survives the rebuild for the same reason the step rows do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outputs(BTreeMap<String, Output>);

impl Outputs {
    /// An empty set, which is what a plan that has not run yet produced.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Record what one step produced. [`nothing`] is not recorded: a step with nothing
    /// to say is indistinguishable from a step that has not run, and both are absent.
    pub fn record(&mut self, key: String, output: Output) {
        if output.is_null() {
            return;
        }
        self.0.insert(key, output);
    }

    /// What one step produced, as the type the commit expects it in.
    ///
    /// `None` means the step has not run, or ran and had nothing to say. A commit that
    /// treats those as a default value is choosing what a resumed run does when the
    /// journal is silent, so it says so where a reader can see it.
    ///
    /// # Errors
    /// [`Error::InvalidValue`] when a value is there but is not what the commit asked
    /// for. This is a build reading a journal an older build wrote, and guessing at it
    /// is how a resumed run stops being the run it is resuming.
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let Some(value) = self.0.get(key) else { return Ok(None) };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| Error::InvalidValue { kind: "step output", value: key.to_owned() })
    }

    /// Whether any step recorded anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The registry write that finishes an operation.
///
/// It runs inside one transaction together with the journal's own move to `committed`,
/// so the rows an operation adds and the record that it finished cannot come apart. It
/// is given what every step produced ([`Outputs`]), and returns whatever the caller's
/// report needs of the write itself — [`nothing`] when the report needs none of it.
///
/// The commit's own answer is not journalled. Unlike a step's, it is produced afresh
/// every time the commit runs, and it is read by the process that ran the commit; a
/// resumed run's commit hands it to [`crate::lifecycle::resolve`], which reports what
/// it resolved rather than what the operation would have printed.
pub type Commit = Box<dyn Fn(&Transaction<'_>, &Outputs) -> Result<Output>>;

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
