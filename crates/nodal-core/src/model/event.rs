//! The event log: what happened in a unit, and whether Nodal saw it or was told it.
//!
//! Events are dual-written, to the store for queries and to the unit's `events.jsonl`
//! as the portable record, so the schema here is also a file format.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::actor::Actor;
use crate::model::ids::{EnvId, EventId, UnitId};
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// A name for one of an event's references, for example `commit` or `file`.
    RefName, kind = "reference name", shape = scalar::TOKEN
}

string_newtype! {
    /// A pointer to raw output kept outside the event, such as a log file path.
    RawRef, kind = "raw reference", shape = scalar::LINE
}

/// What kind of thing happened.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// An actor attached to an environment.
    Attached,
    /// An actor detached.
    Detached,
    /// A command was run.
    Command,
    /// A commit was made.
    Commit,
    /// A test run reported a result.
    TestResult,
    /// Something failed.
    Failure,
    /// A file was written.
    FileTouched,
    /// Something was learned about the code.
    Finding,
    /// A choice was made and should be inherited by whoever continues.
    Decision,
    /// Something is blocking and needs an answer.
    Question,
    /// A handoff note to the next actor.
    Handoff,
    /// A sync ran, with its outcomes.
    Sync,
    /// A free note.
    Note,
}

/// How the event is known. V1 has two tiers and no more: Nodal watched it happen, or
/// somebody said so.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Epistemic {
    /// Captured by Nodal from a shim, a hook or the Git state.
    Observed,
    /// Declared by a person or an agent.
    Stated,
}

/// One entry in a unit's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    /// Identity of the event; sorting by it sorts by time.
    pub id: EventId,
    /// The unit the event belongs to.
    pub unit: UnitId,
    /// The environment it happened in, when it happened in one.
    pub environment: Option<EnvId>,
    /// When it happened.
    pub ts: Timestamp,
    /// Who or what it happened by.
    pub actor: Actor,
    /// What kind of thing happened.
    pub kind: EventKind,
    /// Whether Nodal observed it or was told.
    pub epistemic: Epistemic,
    /// The event in words, for the compiled context.
    pub body: String,
    /// Named references the event points at: commits, files, ports, exit codes.
    pub refs: BTreeMap<RefName, String>,
    /// Where the raw output lives, when it was too large to keep inline.
    pub raw_ref: Option<RawRef>,
}
