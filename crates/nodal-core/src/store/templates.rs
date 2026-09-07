//! Rows of the `db_template` table: the frozen databases a unit's database is copied
//! from.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{DbName, DbTemplate, ProjectId, SchemaFp, TemplateId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "db_template";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, project_id, schema_fingerprint, db_name, parent_template_id, built_at";

/// Record a frozen template. One template exists per schema fingerprint.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when a template for that fingerprint or database
/// name is already recorded, [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, template: &DbTemplate) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO db_template (id, project_id, schema_fingerprint, db_name, \
         parent_template_id, built_at) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            template.id.to_string(),
            template.project_id.to_string(),
            template.schema_fingerprint.0.as_str(),
            template.db_name.as_str(),
            template.parent_template_id.map(|parent| parent.to_string()),
            template.built_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// One template by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: TemplateId) -> Result<Option<DbTemplate>> {
    let sql = format!("SELECT {COLUMNS} FROM db_template WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// The template current for a schema fingerprint, `None` when there is none.
///
/// # Errors
/// As [`get`].
pub fn find(
    conn: &Connection,
    project_id: ProjectId,
    fingerprint: &SchemaFp,
) -> Result<Option<DbTemplate>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM db_template WHERE project_id = ? AND schema_fingerprint = ?"
    );
    row::one(conn, &sql, params![project_id.to_string(), fingerprint.0.as_str()], decode)
}

/// Every template of a project, oldest first, which is also parent-before-child order.
///
/// # Errors
/// As [`get`].
pub fn list_for_project(conn: &Connection, project_id: ProjectId) -> Result<Vec<DbTemplate>> {
    let sql = format!("SELECT {COLUMNS} FROM db_template WHERE project_id = ? ORDER BY id");
    row::many(conn, &sql, params![project_id.to_string()], decode)
}

/// Forget a dropped template; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when another template was built from this one,
/// [`crate::Error::Store`] on any other failure.
pub fn delete(conn: &Connection, id: TemplateId) -> Result<bool> {
    let changed =
        row::write(conn, "DELETE FROM db_template WHERE id = ?", params![id.to_string()])?;
    Ok(changed == 1)
}

/// Turn a row into a template.
fn decode(row: &Row<'_>) -> Result<DbTemplate> {
    Ok(DbTemplate {
        id: row::scalar::<TemplateId>(row, TABLE, "id")?,
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        schema_fingerprint: SchemaFp(row::scalar(row, TABLE, "schema_fingerprint")?),
        db_name: row::scalar::<DbName>(row, TABLE, "db_name")?,
        parent_template_id: row::scalar_opt::<TemplateId>(row, TABLE, "parent_template_id")?,
        built_at: row::stamp(row, TABLE, "built_at")?,
    })
}
