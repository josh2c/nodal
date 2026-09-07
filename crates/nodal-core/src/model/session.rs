//! Sessions: one attachment of an actor to an environment.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::actor::Actor;
use crate::model::ids::{EnvId, SessionId};
use crate::model::timestamp::Timestamp;

/// One shell, agent or editor attached to a home directory. Sessions are what makes
/// "who is in this unit right now" answerable without guessing from processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Session {
    /// Identity of the session.
    pub id: SessionId,
    /// The environment the actor attached to.
    pub environment_id: EnvId,
    /// Who attached.
    pub actor: Actor,
    /// Process id on the session's host, for attribution and idle detection.
    pub pid: Option<u32>,
    /// When the actor attached.
    pub started_at: Timestamp,
    /// When it detached; absent while the session is open.
    pub ended_at: Option<Timestamp>,
}
