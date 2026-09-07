//! The project: one Git repository Nodal manages units for.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::fingerprint::Digest;
use crate::model::ids::ProjectId;
use crate::model::scalar::{LINE_PATTERN, is_line, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// A project's display name, taken from its directory or its remote.
    ProjectName, kind = "project name", pattern = LINE_PATTERN, validate = is_line
}

/// A repository Nodal knows about, and the recipe it was last read with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    /// Identity of the project.
    pub id: ProjectId,
    /// Absolute path of the repository root on this host.
    pub root: PathBuf,
    /// Display name; the handle a user sees in `nodal ls`.
    pub name: ProjectName,
    /// Digest of the effective recipe, so a changed `nodal.toml` is visible without
    /// re-reading the file.
    pub recipe_hash: Digest,
    /// When Nodal first saw this project.
    pub created_at: Timestamp,
}
