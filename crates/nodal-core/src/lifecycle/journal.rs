//! The journal: the rows an operation writes about itself while it runs.
//!
//! Two tables, described in `store/migrations/0002_journal.sql`. `operation` is the run
//! — what it was, whose it was, and whether it finished — and `operation_step` is one
//! row per step, written before the step is attempted and again once it is done.
//!
//! Every function here is one statement. Deciding what to write is
//! [`crate::lifecycle`]'s job; keeping the writes durable one at a time is this file's,
//! because durability between steps is the whole point: a process killed in the gap
//! between two steps has to have left behind an accurate account of what it did.

use rusqlite::{Connection, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;
use crate::lifecycle::owner::Owner;
use crate::lifecycle::step::{Output, Outputs, Plan, Recovery};
use crate::model::{HostName, OperationId, Timestamp};
use crate::store::row;

/// The tables these functions read and write.
const TABLE: &str = "operation";
const STEP_TABLE: &str = "operation_step";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, kind, subject, params, recovery, state, host, pid, started_at, ended_at";

/// Every column [`decode_step`] reads.
const STEP_COLUMNS: &str = "position, key, state, output, updated_at";

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

/// How far one step got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    /// Written down, and being attempted. A step found in this state was interrupted
    /// part-way, so it counts as applied for the purpose of undoing it.
    Applying,
    /// Done.
    Applied,
    /// Taken back.
    Undone,
}

impl StepState {
    /// Whether a step in this state may have changed something that needs undoing.
    #[must_use]
    pub const fn may_have_acted(self) -> bool {
        matches!(self, Self::Applying | Self::Applied)
    }
}

/// One run of an operation, as the journal has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// Which run this is.
    pub id: OperationId,
    /// Which operation it is: `new`, `reclaim`, and so on.
    pub kind: String,
    /// What it acts on, for the report.
    pub subject: String,
    /// The plan's input, for rebuilding it.
    pub params: Value,
    /// What to do with this run if it was interrupted.
    pub recovery: Recovery,
    /// How far it got.
    pub state: State,
    /// The machine it ran on.
    pub host: HostName,
    /// Its process identifier on that machine.
    pub pid: u32,
    /// When it started.
    pub started_at: Timestamp,
    /// When it reached a state that is not `running`.
    pub ended_at: Option<Timestamp>,
}

impl Operation {
    /// The process that started this run.
    #[must_use]
    pub fn owner(&self) -> Owner {
        Owner { host: self.host.clone(), pid: self.pid }
    }
}

/// One step of a run, as the journal has it.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRecord {
    /// Its place in the plan, counting from zero.
    pub position: u32,
    /// Its key, which is how it is matched to a step of a rebuilt plan.
    pub key: String,
    /// How far it got.
    pub state: StepState,
    /// What it produced for the registry write, when it produced anything.
    ///
    /// This is the whole of what the process that started a run can tell the process
    /// that finishes it about what the run learned on the way. `None` for a step that
    /// has nothing to say and for one that has not been applied yet.
    pub output: Option<Output>,
    /// When that was last written.
    pub updated_at: Timestamp,
}

/// Write down that a run has started, before anything is done.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreEncode`] when
/// the plan's parameters are not JSON.
pub fn start(
    conn: &Connection,
    id: OperationId,
    plan: &Plan,
    owner: &Owner,
    at: Timestamp,
) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO operation (id, kind, subject, params, recovery, state, host, pid, \
         started_at, ended_at) VALUES (?, ?, ?, ?, ?, 'running', ?, ?, ?, NULL)",
        params![
            id.to_string(),
            plan.kind,
            plan.subject.as_str(),
            row::json_of(&plan.params, "operation parameters")?,
            row::name_of(&plan.recovery, "recovery policy")?,
            owner.host.as_str(),
            owner.pid,
            at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// Record where one step has got to, creating its row the first time.
///
/// The record is the whole of what is written, so a caller states the step's place, its
/// key and its state together and there is no argument order to get wrong.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn mark_step(conn: &Connection, id: OperationId, step: &StepRecord) -> Result<()> {
    let output = step.output.as_ref().map(|o| row::json_of(o, "step output")).transpose()?;
    row::write(
        conn,
        "INSERT INTO operation_step (operation_id, position, key, state, output, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT (operation_id, position) DO UPDATE SET \
         key = excluded.key, state = excluded.state, \
         output = COALESCE(excluded.output, operation_step.output), \
         updated_at = excluded.updated_at",
        params![
            id.to_string(),
            step.position,
            step.key.as_str(),
            row::name_of(&step.state, "step state")?,
            output,
            step.updated_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// Move a run out of `running`. Runs on the caller's connection, which for a committed
/// operation is the transaction that also writes the registry rows.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn finish(conn: &Connection, id: OperationId, state: State, at: Timestamp) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE operation SET state = ?, ended_at = ? WHERE id = ? AND state = 'running'",
        params![row::name_of(&state, "operation state")?, at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// One run by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: OperationId) -> Result<Option<Operation>> {
    let sql = format!("SELECT {COLUMNS} FROM operation WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// Every run that never finished, oldest first. The read every command makes.
///
/// # Errors
/// As [`get`].
pub fn unfinished(conn: &Connection) -> Result<Vec<Operation>> {
    let sql = format!("SELECT {COLUMNS} FROM operation WHERE state = 'running' ORDER BY id");
    row::many(conn, &sql, [], decode)
}

/// Every run of one kind that a failed step stopped, newest first.
///
/// The read that lets a build offer to carry on. A `failed` row is not `running`, so
/// [`unfinished`] does not see it and no resolver will touch it: it waits for a person
/// to ask for it, and this is how the command that asks finds it.
///
/// # Errors
/// As [`get`].
pub fn failed(conn: &Connection, kind: &str) -> Result<Vec<Operation>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM operation WHERE kind = ? AND state = 'failed' ORDER BY id DESC"
    );
    row::many(conn, &sql, params![kind], decode)
}

/// Every run in a terminal state, newest first, at most `limit` of them.
///
/// # Errors
/// As [`get`].
pub fn recent(conn: &Connection, limit: u32) -> Result<Vec<Operation>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM operation WHERE state <> 'running' ORDER BY id DESC LIMIT ?"
    );
    row::many(conn, &sql, params![limit], decode)
}

/// The steps of a run, in plan order.
///
/// # Errors
/// As [`get`].
pub fn steps(conn: &Connection, id: OperationId) -> Result<Vec<StepRecord>> {
    let sql = format!(
        "SELECT {STEP_COLUMNS} FROM operation_step WHERE operation_id = ? ORDER BY position"
    );
    row::many(conn, &sql, params![id.to_string()], decode_step)
}

/// Turn a row into a run.
fn decode(row: &Row<'_>) -> Result<Operation> {
    Ok(Operation {
        id: row::scalar::<OperationId>(row, TABLE, "id")?,
        kind: row::plain(row, TABLE, "kind")?,
        subject: row::plain(row, TABLE, "subject")?,
        params: row::json(row, TABLE, "params")?,
        recovery: row::name::<Recovery>(row, TABLE, "recovery")?,
        state: row::name::<State>(row, TABLE, "state")?,
        host: row::scalar::<HostName>(row, TABLE, "host")?,
        pid: row::number(row, TABLE, "pid")?,
        started_at: row::stamp(row, TABLE, "started_at")?,
        ended_at: row::stamp_opt(row, TABLE, "ended_at")?,
    })
}

/// What the applied steps of a run produced, ready for the run's commit.
///
/// The read that makes a rebuilt plan's commit see what the first run's steps saw. Only
/// applied steps count: a step recorded `applying` was interrupted inside itself, so
/// whatever it had learned was never written down, and a resumed run applies it again
/// and gets the answer afresh.
///
/// # Errors
/// As [`get`].
pub fn outputs(conn: &Connection, id: OperationId) -> Result<Outputs> {
    let mut outputs = Outputs::new();
    for step in steps(conn, id)?.into_iter().filter(|step| step.state == StepState::Applied) {
        if let Some(output) = step.output {
            outputs.record(step.key, output);
        }
    }
    Ok(outputs)
}

/// Turn a row into a step.
fn decode_step(row: &Row<'_>) -> Result<StepRecord> {
    Ok(StepRecord {
        position: row::number(row, STEP_TABLE, "position")?,
        key: row::plain(row, STEP_TABLE, "key")?,
        state: row::name::<StepState>(row, STEP_TABLE, "state")?,
        output: row::json_opt(row, STEP_TABLE, "output")?,
        updated_at: row::stamp(row, STEP_TABLE, "updated_at")?,
    })
}
