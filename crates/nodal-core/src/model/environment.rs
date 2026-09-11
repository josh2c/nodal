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

/// The name used when the machine will not say what it is called. A registry that only
/// ever sees one host still needs the column filled in.
const UNKNOWN_HOST: &str = "localhost";

/// The longest host name accepted from the operating system, including the terminator.
const HOST_NAME_MAX: usize = 256;

impl HostName {
    /// What this machine is called, or [`UNKNOWN_HOST`] when it will not say.
    ///
    /// A name the model rejects — one that is not visible ASCII — is treated the same as
    /// no name at all. The value is a label in a report and a guard against acting on
    /// another machine's rows, not something to fail an operation over.
    ///
    /// It sits on the type rather than in any one module because four unrelated things
    /// need it: an operation's owner, a session row, a lock row and `nodal ps`. The
    /// precedent is [`Timestamp::now`], which is the same shape — a machine reading that
    /// belongs to the value it produces.
    #[must_use]
    pub fn current() -> Self {
        let fallback =
            || Self::parse(UNKNOWN_HOST).unwrap_or_else(|_| unreachable!("localhost is a token"));
        read_host_name().and_then(|name| Self::parse(name).ok()).unwrap_or_else(fallback)
    }
}

/// The host name as the operating system gives it, if it gives one.
#[cfg(unix)]
fn read_host_name() -> Option<String> {
    let mut buffer = [0_u8; HOST_NAME_MAX];
    // SAFETY: `buffer` is a live array of `HOST_NAME_MAX` bytes and the length passed
    // is one less than that, so the terminator the call writes stays inside it and the
    // last byte, already zero, is never overwritten.
    let code = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len() - 1) };
    if code != 0 {
        return None;
    }
    let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
    let name = core::str::from_utf8(&buffer[..end]).ok()?;
    Some(name.to_owned())
}

/// Windows and anything else: the environment is the only portable source here, and it
/// is allowed to be silent.
#[cfg(not(unix))]
fn read_host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok()
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
