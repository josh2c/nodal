//! Operations: run a plan, and clean up after one that never finished.
//!
//! Every operation Nodal performs — creating a unit, reclaiming one, garbage-collecting
//! a base — creates directories, databases and containers before it writes a single
//! registry row. The half-finished states in between are the ones that hurt: a home on
//! disk that no `nodal ls` knows about is worse than no home at all, because nothing
//! will ever clean it up.
//!
//! So an operation is a [`Plan`]: a list of idempotent [`Step`]s, each with an undo,
//! and one registry write at the end. [`run`] applies the steps one at a time
//! and writes down where it has got to *before* and *after* each one, so the record on
//! disk is never ahead of the world. The registry write and the journal's own move to
//! `committed` share one transaction, which is what makes an operation either wholly
//! done or wholly not.
//!
//! Two things can go wrong, and they are handled differently.
//!
//! A step that *fails* is handled on the spot: [`run`] undoes the steps it applied, in
//! reverse, and returns the failure.
//!
//! A process that *dies* — `^C` between two steps, a laptop closing, an OOM kill —
//! cannot handle anything, so the next `nodal` does it. [`resolve`] reads the journal
//! for runs still marked `running`, works out which of them belonged to a process that
//! is no longer there ([`owner`]), rebuilds each one's plan from what the journal
//! recorded ([`Rebuild`]), and either finishes it or takes it back, as the plan asked
//! ([`Recovery`]). What it did is returned as [`Resolution`]s for the command to print,
//! because a user whose `nodal new` was killed should be told that the leftovers were
//! cleaned up, not left to wonder.

pub mod guard;
pub mod journal;
pub mod marker;
pub mod ops;
pub mod owner;
pub mod step;

use crate::lifecycle::journal::{Operation, State, StepRecord, StepState};
use crate::lifecycle::owner::{Liveness, Owner};
use crate::model::{OperationId, Timestamp};
use crate::store::Store;
use crate::{Error, Result};

pub use crate::lifecycle::step::{Commit, Plan, Recovery, Step};

/// How an interrupted operation's plan is found again.
///
/// The process that resolves a run is not the process that started it, so it does not
/// have the plan — only the `kind` and `params` the journal kept. One implementation
/// per operation turns those back into a [`Plan`], and the set of them is a table the
/// caller passes in rather than a match arm somewhere in here.
pub trait Rebuild {
    /// The [`Plan::kind`] this rebuilds.
    fn kind(&self) -> &'static str;

    /// The plan the interrupted run was following.
    ///
    /// # Errors
    /// Whatever building the plan reports, including a `params` value this build no
    /// longer understands.
    fn rebuild(&self, record: &Operation) -> Result<Plan>;
}

/// What [`resolve`] did about one unfinished operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Its process is still running it. Nothing was done.
    InProgress,
    /// It belongs to another machine, which is the only one that can undo its work.
    Elsewhere,
    /// Its steps were undone and nothing it did is left.
    RolledBack {
        /// The keys undone, in the order they were undone.
        undone: Vec<String>,
    },
    /// Its remaining steps were applied and its registry write committed.
    Resumed {
        /// The keys applied to finish it, in order.
        applied: Vec<String>,
    },
    /// No [`Rebuild`] in the table claims its kind, so its plan cannot be rebuilt.
    /// Left as it is, and reported every time until a build that knows it comes along.
    Unknown,
    /// Its plan could not be rebuilt from what the journal kept. Left as it is: one
    /// operation this build cannot read must not stop the others being cleaned up.
    Unresolved {
        /// Why the plan could not be rebuilt, rendered.
        why: String,
    },
    /// An undo failed. Something it did is still out there; the message says what.
    Failed {
        /// The step whose undo failed.
        key: String,
        /// Why it failed, rendered, because the operation carries on past it.
        why: String,
    },
}

impl Action {
    /// A word for the report.
    #[must_use]
    pub const fn verb(&self) -> &'static str {
        match self {
            Self::InProgress => "in progress",
            Self::Elsewhere => "on another host",
            Self::RolledBack { .. } => "rolled back",
            Self::Resumed { .. } => "resumed",
            Self::Unknown => "unrecognised",
            Self::Unresolved { .. } => "could not be rebuilt",
            Self::Failed { .. } => "could not be undone",
        }
    }
}

/// One unfinished operation and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Which run.
    pub id: OperationId,
    /// Which operation it was.
    pub kind: String,
    /// What it acted on.
    pub subject: String,
    /// What was done about it.
    pub action: Action,
}

impl core::fmt::Display for Resolution {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} ({}) was interrupted and {}", self.kind, self.subject, self.action.verb())?;
        match &self.action {
            Action::RolledBack { undone } if !undone.is_empty() => {
                write!(f, ": {}", undone.join(", "))
            }
            Action::Resumed { applied } if !applied.is_empty() => {
                write!(f, ": {}", applied.join(", "))
            }
            Action::Failed { key, why } => write!(f, ": {key}: {why}"),
            Action::Unresolved { why } => write!(f, ": {why}"),
            _ => Ok(()),
        }
    }
}

/// Run a plan: apply every step, then write the registry rows in one transaction.
///
/// A step that fails takes the operation with it: the steps already applied are undone,
/// in reverse, and the failure is returned. The registry is untouched in that case,
/// because the only write to it is the last thing that happens.
///
/// # Errors
/// [`Error::OperationStep`] when a step failed and the earlier steps were undone,
/// [`Error::OperationUndo`] when that undo also failed, and [`Error::Store`] when the
/// journal or the final write could not be made.
pub fn run(store: &mut Store, plan: &Plan) -> Result<OperationId> {
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let owner = Owner::current();
    journal::start(store.conn(), id, plan, &owner, Timestamp::now())?;
    for (position, step) in plan.steps.iter().enumerate() {
        let position = position_of(position)?;
        mark(store, id, position, &step.key(), StepState::Applying)?;
        match step.apply() {
            Ok(()) => {
                mark(store, id, position, &step.key(), StepState::Applied)?;
            }
            Err(source) => {
                undo_applied(store, id, plan)?;
                return Err(Error::OperationStep {
                    operation: id,
                    kind: plan.kind,
                    key: step.key(),
                    source: Box::new(source),
                });
            }
        }
    }
    match commit(store, id, plan) {
        Ok(()) => Ok(id),
        Err(source) => {
            undo_applied(store, id, plan)?;
            Err(source)
        }
    }
}

/// The registry rows and the record that the operation finished, written together.
fn commit(store: &mut Store, id: OperationId, plan: &Plan) -> Result<()> {
    let path = store.path().to_path_buf();
    let tx = store.transaction()?;
    (plan.commit)(&tx)?;
    close(&tx, id, plan.kind, State::Committed)?;
    tx.commit().map_err(|source| Error::Store { path, source: Box::new(source) })
}

/// Report every unfinished operation and resolve the ones that are ours to resolve.
///
/// This is what a command calls before it does anything else. Operations another
/// process or another machine is still running are reported and left alone; the rest
/// are rebuilt from the journal and either finished or taken back.
///
/// # Errors
/// [`Error::Store`] when the journal could not be read or written. A single operation
/// that could not be undone is a [`Action::Failed`] in the returned list rather than an
/// error, so one stuck operation does not stop the others being cleaned up.
pub fn resolve(store: &mut Store, rebuilders: &[&dyn Rebuild]) -> Result<Vec<Resolution>> {
    let here = Owner::current();
    let unfinished = journal::unfinished(store.conn())?;
    let mut resolutions = Vec::with_capacity(unfinished.len());
    for record in unfinished {
        let action = match record.owner().state(&here) {
            Liveness::Running => Action::InProgress,
            Liveness::Elsewhere => Action::Elsewhere,
            Liveness::Gone => take_over(store, &record, rebuilders)?,
        };
        resolutions.push(Resolution {
            id: record.id,
            kind: record.kind,
            subject: record.subject,
            action,
        });
    }
    Ok(resolutions)
}

/// Finish or undo one operation whose process is gone.
fn take_over(store: &mut Store, record: &Operation, rebuilders: &[&dyn Rebuild]) -> Result<Action> {
    let Some(rebuilder) = rebuilders.iter().find(|entry| entry.kind() == record.kind) else {
        return Ok(Action::Unknown);
    };
    let plan = match rebuilder.rebuild(record) {
        Ok(plan) => plan,
        Err(why) => return Ok(Action::Unresolved { why: why.to_string() }),
    };
    match record.recovery {
        Recovery::RollBack => roll_back(store, record.id, &plan),
        Recovery::Resume => resume(store, record.id, &plan),
    }
}

/// Undo every step of a rebuilt plan that the journal says may have acted.
fn roll_back(store: &mut Store, id: OperationId, plan: &Plan) -> Result<Action> {
    match undo_steps(store, id, plan)? {
        Undone::All(undone) => {
            close(store.conn(), id, plan.kind, State::RolledBack)?;
            Ok(Action::RolledBack { undone })
        }
        Undone::Stopped { key, why } => {
            close(store.conn(), id, plan.kind, State::Failed)?;
            Ok(Action::Failed { key, why })
        }
    }
}

/// Apply the steps a rebuilt plan has left, then commit it.
///
/// Every step is idempotent, so the one that was in flight when the process died is
/// simply applied again rather than reasoned about.
fn resume(store: &mut Store, id: OperationId, plan: &Plan) -> Result<Action> {
    let done = applied_keys(store, id)?;
    let mut applied = Vec::new();
    for (position, step) in plan.steps.iter().enumerate() {
        let position = position_of(position)?;
        if done.contains(&(position, step.key())) {
            continue;
        }
        mark(store, id, position, &step.key(), StepState::Applying)?;
        if let Err(source) = step.apply() {
            return Err(Error::OperationStep {
                operation: id,
                kind: plan.kind,
                key: step.key(),
                source: Box::new(source),
            });
        }
        mark(store, id, position, &step.key(), StepState::Applied)?;
        applied.push(step.key());
    }
    commit(store, id, plan)?;
    Ok(Action::Resumed { applied })
}

/// Which steps of a run are already done, by position and key.
fn applied_keys(store: &Store, id: OperationId) -> Result<Vec<(u32, String)>> {
    Ok(journal::steps(store.conn(), id)?
        .into_iter()
        .filter(|step| step.state == StepState::Applied)
        .map(|step| (step.position, step.key))
        .collect())
}

/// How far [`undo_steps`] got.
enum Undone {
    /// Every step that may have acted was undone; here are their keys.
    All(Vec<String>),
    /// One undo failed, and the ones before it in the reverse order were done.
    Stopped {
        /// The step that would not come back.
        key: String,
        /// Why, rendered.
        why: String,
    },
}

/// Undo a run's steps in reverse, stopping at the first undo that fails.
///
/// A step the journal never saw is not undone: the plan is rebuilt from `params`, so it
/// has steps this run never reached, and calling their undo would be undoing work that
/// was never done. A step recorded as `applying` *is* undone, because the process died
/// somewhere inside it and the undo is idempotent.
fn undo_steps(store: &mut Store, id: OperationId, plan: &Plan) -> Result<Undone> {
    let acted = acted_positions(store, id)?;
    let mut undone = Vec::new();
    for (position, step) in plan.steps.iter().enumerate().rev() {
        let position = position_of(position)?;
        if !acted.iter().any(|record| record.position == position) {
            continue;
        }
        if let Err(source) = step.undo() {
            return Ok(Undone::Stopped { key: step.key(), why: source.to_string() });
        }
        mark(store, id, position, &step.key(), StepState::Undone)?;
        undone.push(step.key());
    }
    Ok(Undone::All(undone))
}

/// The steps of a run that may have changed something.
fn acted_positions(store: &Store, id: OperationId) -> Result<Vec<StepRecord>> {
    Ok(journal::steps(store.conn(), id)?
        .into_iter()
        .filter(|step| step.state.may_have_acted())
        .collect())
}

/// Undo what a failing run applied, and mark it rolled back.
fn undo_applied(store: &mut Store, id: OperationId, plan: &Plan) -> Result<()> {
    match undo_steps(store, id, plan)? {
        Undone::All(_) => {
            close(store.conn(), id, plan.kind, State::RolledBack)?;
            Ok(())
        }
        Undone::Stopped { key, why } => {
            close(store.conn(), id, plan.kind, State::Failed)?;
            Err(Error::OperationUndo { operation: id, kind: plan.kind, key, why })
        }
    }
}

/// Move a run out of `running`, and insist that there was one to move.
///
/// The journal is what the next command acts on, so a registry write that lands beside
/// a run still marked `running` would have the next command undo work that in fact
/// committed. Refusing here is the difference between an impossible state and a
/// destructive one.
fn close(
    conn: &rusqlite::Connection,
    id: OperationId,
    kind: &'static str,
    state: State,
) -> Result<()> {
    if journal::finish(conn, id, state, Timestamp::now())? {
        return Ok(());
    }
    Err(Error::OperationVanished { operation: id, kind })
}

/// Write down where a step has got to, stamped now.
fn mark(store: &Store, id: OperationId, position: u32, key: &str, state: StepState) -> Result<()> {
    let record = StepRecord { position, key: key.to_owned(), state, updated_at: Timestamp::now() };
    journal::mark_step(store.conn(), id, &record)
}

/// A step's index as the journal keeps it. A plan longer than four billion steps is not
/// a plan, but the conversion is still not allowed to be a panic.
fn position_of(index: usize) -> Result<u32> {
    u32::try_from(index).map_err(|_| Error::StoreEncode { kind: "step position" })
}
