//! Locks: which host may write a unit.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::environment::HostName;
use crate::model::ids::UnitId;
use crate::model::timestamp::Timestamp;

/// The single-writer claim on a unit. V1 runs on one host at a time; the lock exists so
/// that a unit moved to another machine cannot be written from both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Lock {
    /// The unit being held.
    pub unit_id: UnitId,
    /// The host that holds the write.
    pub host: HostName,
    /// When the claim lapses unless renewed.
    pub expires_at: Timestamp,
}
