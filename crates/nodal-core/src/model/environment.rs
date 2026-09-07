//! The environment: one materialisation of a unit on one host.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::db_template::DbName;
use crate::model::fingerprint::{SchemaFp, WorkspaceFp};
use crate::model::ids::{BaseId, EnvId, UnitId};
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// The name of a host a unit can live on.
    HostName, kind = "host name", shape = scalar::TOKEN
}

string_newtype! {
    /// The name a recipe gives a port, for example `app` or `api`.
    PortName, kind = "port name", shape = scalar::TOKEN
}

/// Whether an environment exists on disk and whether anything is running in it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EnvState {
    /// Recorded but not materialised, or reclaimed.
    Absent,
    /// On disk, nothing running.
    Stopped,
    /// On disk with attributed processes.
    Running,
}

/// The ports allocated to an environment, by the name the recipe gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct Ports(pub BTreeMap<PortName, u16>);

/// One home directory, its services and how current it is.
///
/// `ws_fp_materialized` and `schema_fp_materialized` are what was installed here, not
/// what the tree now asks for: staleness is the difference between the two, computed
/// when it matters. Whether the tree is dirty is never stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Environment {
    /// Identity of the environment.
    pub id: EnvId,
    /// The unit this is a materialisation of.
    pub unit_id: UnitId,
    /// Which materialisation of that unit this is, counting from one.
    pub attempt: u32,
    /// The home directory.
    pub home: PathBuf,
    /// False for a directory adopted in place, which Nodal did not create and will
    /// never remove.
    pub managed: bool,
    /// The base the home was cloned from, when it was cloned from one.
    pub base_id: Option<BaseId>,
    /// The workspace fingerprint actually installed here.
    pub ws_fp_materialized: Option<WorkspaceFp>,
    /// The schema fingerprint actually present in the database here.
    pub schema_fp_materialized: Option<SchemaFp>,
    /// The host that holds the writable copy.
    pub host: HostName,
    /// The per-unit database, when the stack has one.
    pub db_name: Option<DbName>,
    /// Ports allocated from the project's block.
    pub ports: Ports,
    /// A port the project pins and only one environment may hold at a time.
    pub fixed_port: Option<u16>,
    /// Whether the home exists and whether anything runs in it.
    pub state: EnvState,
    /// When the environment was created.
    pub created_at: Timestamp,
    /// Last observed activity in it.
    pub last_active: Timestamp,
}
