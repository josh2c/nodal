//! Who did something: a person or an agent.
//!
//! The same shape is used by sessions and events, because "which agent wrote this" is
//! the question a handoff has to answer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::scalar::{self, string_newtype};

string_newtype! {
    /// What the actor is called: a tool name such as `claude-code`, or a person's
    /// handle. Never a credential.
    ActorName, kind = "actor name", shape = scalar::LINE
}

/// Whether the actor is a person or a program acting on its own.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// A person at a terminal or an editor.
    Human,
    /// A coding agent.
    Agent,
}

/// The actor of a session or an event.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct Actor {
    /// Person or agent.
    pub kind: ActorKind,
    /// What it is called.
    pub name: ActorName,
}
