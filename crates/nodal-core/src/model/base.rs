//! Warm bases: the substrate a unit's home is cloned from.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::fingerprint::Digest;
use crate::model::fingerprint::{Platform, WorkspaceFp};
use crate::model::ids::{BaseId, ProjectId};
use crate::model::recipe::{ToolName, ToolVersion};
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;
use crate::model::version::Version;

string_newtype! {
    /// A Git object id in full, lowercase hexadecimal form.
    CommitId, kind = "commit id", shape = scalar::OBJECT_ID
}

/// What built a base, and with what.
///
/// A base is minutes of work that a later release may no longer produce the same way,
/// and until this record existed a base could not say which release made it, which
/// commands it ran or which tools answered them. A person looking at a base that
/// behaves unlike a fresh one had nothing to compare.
///
/// It is not part of the key. A base is keyed by its workspace fingerprint and its
/// platform, and none of this changes that: a base built by an older Nodal is still
/// warm, and this is what makes that visible rather than making it cold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// The Nodal that built it.
    pub nodal_version: Version,
    /// Every install it ran, in order, each as the argument list it was given.
    pub install: Vec<Vec<String>>,
    /// The build it ran afterwards, empty when it was not asked for one.
    pub warm: Vec<String>,
    /// What each tool answered when it was asked its version, by the name the recipe
    /// records that tool under.
    pub tools: BTreeMap<ToolName, ToolVersion>,
    /// The effective recipe the build read, by content.
    pub recipe: Digest,
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
    /// What built it. `None` for a base built before a base recorded this, which is
    /// the one thing a stale base cannot be asked about.
    pub provenance: Option<Provenance>,
}
