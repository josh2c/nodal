//! The manifest: what `.nodal/manifest.toml` says about a unit's home.
//!
//! A tool that finds a home directory reads this file to learn what the directory is,
//! without a registry and without Nodal on its path. It is a record of identity and of
//! names, and it holds no value of any kind — not a secret, and not a generated port
//! either, because a bundle copies this file to another machine (`docs/contracts.md`)
//! and a value that travelled with it would be wrong there.
//!
//! What each name's value is comes from `.nodal/env`, which is written beside it and
//! never leaves the machine.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::environment::HostName;
use crate::model::ids::{EnvId, UnitId};
use crate::model::project::ProjectName;
use crate::model::recipe::EnvName;
use crate::model::timestamp::Timestamp;
use crate::model::unit::Slug;

/// Who supplied an environment variable's value.
///
/// The order of the variants is the order the resolver tries: a name a unit generated
/// for itself beats the same name in a machine-wide file, because a generated value is
/// bound to resources only this unit has.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Nodal itself: the `NODAL_*` variables that say which unit this is.
    Identity,
    /// A value the unit minted for its own services: a port, a URL, a database role.
    Generated,
    /// The per-machine file, `~/.nodal/secrets.env`.
    Machine,
}

/// Why a declared name has no value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Want {
    /// The recipe declares it under `env.generated` and no service produced it.
    Generated,
    /// The recipe declares it under `env.secrets` and no secret source has it.
    Secret,
    /// The recipe declares it under `env.required_local` and no source has it.
    RequiredLocal,
}

/// A declared name that activation could not fill.
///
/// A missing name is a line of a report, never a failure: a working copy of a project
/// whose mail credential is absent still runs everything that does not send mail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct Missing {
    /// The name nothing answered.
    pub name: EnvName,
    /// Which list of the recipe declares it.
    pub want: Want,
}

/// `.nodal/manifest.toml`: what this home is, and which names it was activated with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// The unit this home is a materialisation of.
    pub unit: UnitId,
    /// The materialisation itself.
    pub environment: EnvId,
    /// The unit's CLI handle.
    pub slug: Slug,
    /// The project the unit belongs to.
    pub project: ProjectName,
    /// The host that holds this copy.
    pub host: HostName,
    /// The home directory this file sits in.
    pub home: PathBuf,
    /// When the file was written.
    pub written_at: Timestamp,
    /// Every name `.nodal/env` assigns, and who supplied it. Names only.
    pub env: BTreeMap<EnvName, Origin>,
    /// Every declared name nothing answered.
    pub missing: Vec<Missing>,
}
