//! Frozen database templates: the state a per-unit database is created from.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::fingerprint::SchemaFp;
use crate::model::ids::{ProjectId, TemplateId};
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// A database name Nodal created and may therefore drop.
    DbName, kind = "database name", shape = scalar::SQL_IDENTIFIER
}

/// A migrated and seeded database, frozen so that a new unit's database is a copy
/// rather than a migrate-and-seed run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DbTemplate {
    /// Identity of the template.
    pub id: TemplateId,
    /// The project this template belongs to.
    pub project_id: ProjectId,
    /// The schema fingerprint this template is current for.
    pub schema_fingerprint: SchemaFp,
    /// The database that holds the frozen state.
    pub db_name: DbName,
    /// The template this one was built from by forward migrations, when it was not
    /// built from empty.
    pub parent_template_id: Option<TemplateId>,
    /// When the freeze completed.
    pub built_at: Timestamp,
}
