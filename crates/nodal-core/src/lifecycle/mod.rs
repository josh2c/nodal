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
//! A step that learns something the registry write needs — what a relocation removed,
//! which process groups a teardown could not stop — returns it, and [`run`] writes it
//! into the step's own journal row before it moves on. The commit is handed the whole
//! set ([`Outputs`]). That is what makes a rebuilt run the same run: the process that
//! finishes an interrupted one never saw its steps happen, and reads what they produced
//! out of the journal rather than out of a channel that died with the first process.
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
pub mod hooks;
pub mod identity;
pub mod idle;
pub mod journal;
pub mod marker;
pub mod ops;
pub mod owner;
pub mod states;
pub mod step;
pub mod template;
pub mod uniqueness;

use crate::lifecycle::journal::{Operation, State, StepRecord, StepState};
use crate::lifecycle::owner::{Liveness, Owner};
use crate::model::{OperationId, Timestamp};
use crate::store::Store;
use crate::{Error, Result};

pub use crate::lifecycle::step::{Commit, Output, Outputs, Plan, Recovery, Step, nothing};

/// What a finished run of a plan left behind, for the caller that started it.
///
/// [`run`] returns this rather than an identifier alone because a step's answer is not
/// only the commit's business: an operation reports to a person as well as to the
/// registry, and what it reports is what its steps found. Reading that here rather
/// than out of a channel the operation carried through the plan is what lets a plan be
/// a plain value.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// Which run this was, as the journal names it.
    pub id: OperationId,
    /// What every step of it produced.
    pub outputs: Outputs,
    /// What the registry write itself produced, which is [`nothing`] for the
    /// operations whose report needs nothing of it.
    pub committed: Output,
}

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
/// A step that fails takes the operation with it, and what happens to the work already
/// done is the plan's [`Recovery`] to say. A [`Recovery::RollBack`] plan has its
/// applied steps undone, in reverse, and leaves nothing. A [`Recovery::Resume`] plan
/// keeps them: it declares that its half-done state is harmless where it stands and
/// expensive to make again, and undoing a base build's clone because the install after
/// it failed is exactly the cost that declaration is about. The run is left in the
/// journal as `failed`, with the failing step's own row holding what the step reported,
/// so the attempt that comes next knows where to start and why.
///
/// The registry is untouched either way, because the only write to it is the last thing
/// that happens.
///
/// # Errors
/// [`Error::OperationStep`] when a step failed, [`Error::OperationUndo`] when the undo
/// that followed it also failed, and [`Error::Store`] when the journal or the final
/// write could not be made.
pub fn run(store: &mut Store, plan: &Plan) -> Result<Done> {
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let owner = Owner::current();
    journal::start(store.conn(), id, plan, &owner, Timestamp::now())?;
    let mut outputs = Outputs::new();
    for (position, step) in plan.steps.iter().enumerate() {
        let position = position_of(position)?;
        mark(store, id, (position, &step.key()), StepState::Applying, None)?;
        match step.apply() {
            Ok(output) => {
                mark(store, id, (position, &step.key()), StepState::Applied, Some(&output))?;
                outputs.record(step.key(), output);
            }
            Err(source) => {
                let failure = Error::OperationStep {
                    operation: id,
                    kind: plan.kind,
                    key: step.key(),
                    source: Box::new(source),
                };
                if plan.recovery == Recovery::Resume {
                    keep(store, id, (position, &step.key()), &failure)?;
                    return Err(failure);
                }
                undo_applied(store, id, plan)?;
                return Err(failure);
            }
        }
    }
    match commit(store, id, plan, &outputs) {
        Ok(committed) => Ok(Done { id, outputs, committed }),
        Err(source) => {
            undo_applied(store, id, plan)?;
            Err(source)
        }
    }
}

/// Write down what a failed step reported, and close the run as `failed`.
///
/// The step's row keeps its `applying` state, which is honest: the step was attempted
/// and did not finish. What is added is its output — the rendered failure, which for a
/// tool carries the tail of both of its streams. That is what a person is shown when
/// the next attempt offers to carry on, and it is the only account of the failure that
/// survives the process that saw it.
fn keep(store: &mut Store, id: OperationId, step: (u32, &str), why: &Error) -> Result<()> {
    let reported = serde_json::json!({ "failed": why.to_string() });
    mark(store, id, step, StepState::Applying, Some(&reported))?;
    close(store.conn(), id, KEPT_KIND, State::Failed)
}

/// What [`close`] is told an operation is when it is being kept rather than undone.
/// The kind is only used to name an operation that has vanished from under us.
const KEPT_KIND: &str = "operation";

/// Carry on with a run that a failed step left behind.
///
/// The counterpart of [`resolve`] for a failure rather than a death. [`resolve`] acts
/// on runs still marked `running`, whose process is gone; this acts on a run marked
/// `failed`, whose process handled the failure and stopped. A person asked for this
/// one: the step that failed is applied again, the steps after it follow, and the
/// registry write ends it, exactly as a first run would have.
///
/// # Errors
/// [`Error::OperationVanished`] when the run is no longer there to be reopened, and
/// whatever the steps and the commit report.
pub fn retry(store: &mut Store, id: OperationId, plan: &Plan) -> Result<Done> {
    reopen(store.conn(), id, plan.kind)?;
    let done = applied_keys(store, id)?;
    let mut outputs = journal::outputs(store.conn(), id)?;
    for (position, step) in plan.steps.iter().enumerate() {
        let position = position_of(position)?;
        if done.contains(&(position, step.key())) {
            continue;
        }
        mark(store, id, (position, &step.key()), StepState::Applying, None)?;
        match step.apply() {
            Ok(output) => {
                mark(store, id, (position, &step.key()), StepState::Applied, Some(&output))?;
                outputs.record(step.key(), output);
            }
            Err(source) => {
                let failure = Error::OperationStep {
                    operation: id,
                    kind: plan.kind,
                    key: step.key(),
                    source: Box::new(source),
                };
                keep(store, id, (position, &step.key()), &failure)?;
                return Err(failure);
            }
        }
    }
    let committed = commit(store, id, plan, &outputs)?;
    Ok(Done { id, outputs, committed })
}

/// Put a `failed` run back into `running`, under this process.
///
/// The owner is rewritten as well as the state. A run this process is now applying
/// steps of must not read as one whose owner is gone, or [`resolve`] in a second
/// `nodal` would take it over while the first is inside a step of it.
fn reopen(conn: &rusqlite::Connection, id: OperationId, kind: &'static str) -> Result<()> {
    let owner = Owner::current();
    let changed = crate::store::row::write(
        conn,
        "UPDATE operation SET state = 'running', ended_at = NULL, host = ?, pid = ? \
         WHERE id = ? AND state = 'failed'",
        rusqlite::params![owner.host.as_str(), owner.pid, id.to_string()],
    )?;
    if changed == 1 {
        return Ok(());
    }
    Err(Error::OperationVanished { operation: id, kind })
}

/// The registry rows and the record that the operation finished, written together.
fn commit(store: &mut Store, id: OperationId, plan: &Plan, outputs: &Outputs) -> Result<Output> {
    let path = store.path().to_path_buf();
    let tx = store.transaction()?;
    let committed = (plan.commit)(&tx, outputs)?;
    close(&tx, id, plan.kind, State::Committed)?;
    tx.commit().map_err(|source| Error::Store { path, source: Box::new(source) })?;
    Ok(committed)
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
    // What the first run's steps learned, which this process never saw happen. Without
    // it the commit below would write the registry a first run would not have written,
    // which is the one difference between the two runs the journal exists to remove.
    let mut outputs = journal::outputs(store.conn(), id)?;
    let mut applied = Vec::new();
    for (position, step) in plan.steps.iter().enumerate() {
        let position = position_of(position)?;
        if done.contains(&(position, step.key())) {
            continue;
        }
        mark(store, id, (position, &step.key()), StepState::Applying, None)?;
        let output = match step.apply() {
            Ok(output) => output,
            Err(source) => {
                return Err(Error::OperationStep {
                    operation: id,
                    kind: plan.kind,
                    key: step.key(),
                    source: Box::new(source),
                });
            }
        };
        mark(store, id, (position, &step.key()), StepState::Applied, Some(&output))?;
        outputs.record(step.key(), output);
        applied.push(step.key());
    }
    commit(store, id, plan, &outputs)?;
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
        mark(store, id, (position, &step.key()), StepState::Undone, None)?;
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

/// Write down where a step has got to and what it produced, stamped now.
///
/// The output is written in the same statement as the state, so there is no moment in
/// which the journal says a step is applied without saying what it produced.
fn mark(
    store: &Store,
    id: OperationId,
    step: (u32, &str),
    state: StepState,
    output: Option<&Output>,
) -> Result<()> {
    let (position, key) = step;
    let record = StepRecord {
        position,
        key: key.to_owned(),
        state,
        output: output.filter(|output| !output.is_null()).cloned(),
        updated_at: Timestamp::now(),
    };
    journal::mark_step(store.conn(), id, &record)
}

/// A step's index as the journal keeps it. A plan longer than four billion steps is not
/// a plan, but the conversion is still not allowed to be a panic.
fn position_of(index: usize) -> Result<u32> {
    u32::try_from(index).map_err(|_| Error::StoreEncode { kind: "step position" })
}
