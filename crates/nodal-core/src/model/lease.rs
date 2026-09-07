//! Leases: a resource only one environment may hold at a time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::ids::EnvId;
use crate::model::scalar::{TOKEN_PATTERN, is_token, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// What is being held, as `kind:value`, for example `port:5432` or `device:usb0`.
    ResourceKey, kind = "resource key", pattern = TOKEN_PATTERN, validate = is_token
}

/// A claim on a singular resource. It expires so that a crashed session cannot hold a
/// fixed port forever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Lease {
    /// The resource being held.
    pub resource: ResourceKey,
    /// The environment holding it.
    pub environment_id: EnvId,
    /// When the claim lapses unless renewed.
    pub expires_at: Timestamp,
}
