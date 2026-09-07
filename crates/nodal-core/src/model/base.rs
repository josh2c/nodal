//! Warm bases: the substrate a unit's home is cloned from.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::fingerprint::{Platform, WorkspaceFp};
use crate::model::ids::{BaseId, ProjectId};
use crate::model::scalar::{OBJECT_ID_PATTERN, is_object_id, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// A Git object id in full, lowercase hexadecimal form.
    CommitId, kind = "commit id", pattern = OBJECT_ID_PATTERN, validate = is_object_id
}

/// One base: a clean checkout with dependencies installed, keyed by the workspace
/// fingerprint it was built for. A unit's home is a copy-on-write clone of one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Base {
    /// Identity of the base.
    pub id: BaseId,
    /// The project this base belongs to.
    pub project_id: ProjectId,
    /// The workspace fingerprint this base is warm for.
    pub ws_fingerprint: WorkspaceFp,
    /// The target triple it was built on; a base is not warm on another platform.
    pub platform: Platform,
    /// The commit the base was built at.
    pub commit: CommitId,
    /// Where the base lives on this host.
    pub path: PathBuf,
    /// When the build finished.
    pub built_at: Timestamp,
    /// Last time a unit was materialised from it; the input to eviction.
    pub last_used: Timestamp,
}
