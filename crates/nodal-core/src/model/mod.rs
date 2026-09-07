//! The domain model: plain data with `serde` and JSON schemas, and no IO.
//!
//! One type per row of the data model. Every value that has a shape is a validated
//! newtype rather than a `String`, so an invalid branch name or database name is
//! rejected where it enters the process instead of where it is used. The JSON schemas
//! in `schemas/` are generated from these types by [`schema::documents`] and diffed in
//! CI, which is what makes them a contract other tools can build on.

mod scalar;

pub mod actor;
pub mod base;
pub mod db_template;
pub mod environment;
pub mod event;
pub mod fingerprint;
pub mod ids;
pub mod lease;
pub mod lock;
pub mod manifest;
pub mod port;
pub mod project;
pub mod recipe;
pub mod schema;
pub mod session;
pub mod timestamp;
pub mod unit;

pub use crate::model::actor::{Actor, ActorKind, ActorName};
pub use crate::model::base::{Base, CommitId};
pub use crate::model::db_template::{DbName, DbTemplate};
pub use crate::model::environment::{EnvState, Environment, HostName, PortName, Ports};
pub use crate::model::event::{Epistemic, Event, EventKind, RawRef, RefName};
pub use crate::model::fingerprint::{
    Digest, FingerprintPart, Platform, SchemaFp, SubFp, WorkspaceFp,
};
pub use crate::model::ids::{
    BaseId, EnvId, EventId, OperationId, ProjectId, SessionId, TemplateId, UnitId,
};
pub use crate::model::lease::{Lease, ResourceKey};
pub use crate::model::lock::Lock;
pub use crate::model::manifest::{Manifest, Missing, Origin, Want};
pub use crate::model::port::{PortAllocation, PortBlock};
pub use crate::model::project::{Project, ProjectName};
pub use crate::model::recipe::{
    Backend, BaseSpec, CommandLine, Commands, DEFAULT_TRASH_RETENTION_DAYS, Db, DbKind, Env,
    EnvName, Hooks, MigrationTool, PackageManager, Recipe, Reclaim, ServiceName, Services, Sync,
    TaskCache, ToolName, ToolVersion,
};
pub use crate::model::schema::{SCHEMA_VERSION, SchemaDoc};
pub use crate::model::session::Session;
pub use crate::model::timestamp::Timestamp;
pub use crate::model::unit::{BranchName, Objective, Slug, Unit, UnitStatus};
