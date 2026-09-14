//! Rows of the `base` table: the warm substrates a unit's home is cloned from.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::base::Provenance;
use crate::model::{Base, BaseId, CommitId, Platform, ProjectId, Timestamp, WorkspaceFp};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "base";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, project_id, ws_fingerprint, platform, commit_id, path, built_at, \
                       last_used, nodal_version, install_argv, warm_argv, tool_versions, \
                       recipe_digest";

/// What a provenance column is called when the store refuses to encode one.
const PROVENANCE: &str = "base provenance";

/// Record a built base. One base exists per fingerprint and platform.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when a base for that key is already recorded,
/// [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, base: &Base) -> Result<()> {
    let built = base.provenance.as_ref();
    row::write(
        conn,
        "INSERT INTO base (id, project_id, ws_fingerprint, platform, commit_id, path, built_at, \
         last_used, nodal_version, install_argv, warm_argv, tool_versions, recipe_digest) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            base.id.to_string(),
            base.project_id.to_string(),
            base.ws_fingerprint.0.as_str(),
            base.platform.as_str(),
            base.commit.as_str(),
            row::path_of(&base.path)?,
            base.built_at.unix_seconds(),
            base.last_used.unix_seconds(),
            built.map(|p| p.nodal_version.to_string()),
            built.map(|p| row::json_of(&p.install, PROVENANCE)).transpose()?,
            built.map(|p| row::json_of(&p.warm, PROVENANCE)).transpose()?,
            built.map(|p| row::json_of(&p.tools, PROVENANCE)).transpose()?,
            built.map(|p| p.recipe.as_str().to_owned()),
        ],
    )?;
    Ok(())
}

/// One base by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: BaseId) -> Result<Option<Base>> {
    let sql = format!("SELECT {COLUMNS} FROM base WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// The base a unit would be cloned from, `None` when none is warm for that key.
///
/// # Errors
/// As [`get`].
pub fn find(
    conn: &Connection,
    project_id: ProjectId,
    fingerprint: &WorkspaceFp,
    platform: &Platform,
) -> Result<Option<Base>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM base WHERE project_id = ? AND ws_fingerprint = ? AND platform = ?"
    );
    let key = params![project_id.to_string(), fingerprint.0.as_str(), platform.as_str()];
    row::one(conn, &sql, key, decode)
}

/// Every base of a project, least recently used first, which is eviction order.
///
/// # Errors
/// As [`get`].
pub fn list_for_project(conn: &Connection, project_id: ProjectId) -> Result<Vec<Base>> {
    let sql = format!("SELECT {COLUMNS} FROM base WHERE project_id = ? ORDER BY last_used, id");
    row::many(conn, &sql, params![project_id.to_string()], decode)
}

/// Record that a base was cloned from; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn touch(conn: &Connection, id: BaseId, at: Timestamp) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE base SET last_used = ? WHERE id = ?",
        params![at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Forget an evicted base; `false` when there is no such row.
///
/// Removing the directory is the caller's step, and is what makes this the second half
/// of an eviction rather than the whole of it.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when an environment still points at the base,
/// [`crate::Error::Store`] on any other failure.
pub fn delete(conn: &Connection, id: BaseId) -> Result<bool> {
    let changed = row::write(conn, "DELETE FROM base WHERE id = ?", params![id.to_string()])?;
    Ok(changed == 1)
}

/// Turn a row into a base.
fn decode(row: &Row<'_>) -> Result<Base> {
    Ok(Base {
        id: row::scalar::<BaseId>(row, TABLE, "id")?,
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        ws_fingerprint: WorkspaceFp(row::scalar(row, TABLE, "ws_fingerprint")?),
        platform: row::scalar::<Platform>(row, TABLE, "platform")?,
        commit: row::scalar::<CommitId>(row, TABLE, "commit_id")?,
        path: row::path(row, TABLE, "path")?,
        built_at: row::stamp(row, TABLE, "built_at")?,
        last_used: row::stamp(row, TABLE, "last_used")?,
        provenance: provenance(row)?,
    })
}

/// What built this base, or `None` for a row written before a base recorded it.
///
/// The version column is what says whether there is a record at all: the four beside it
/// are written in the same statement, so a row with a version has them and a row
/// without one has none of them.
fn provenance(row: &Row<'_>) -> Result<Option<Provenance>> {
    let Some(nodal_version) = row::scalar_opt(row, TABLE, "nodal_version")? else {
        return Ok(None);
    };
    Ok(Some(Provenance {
        nodal_version,
        install: row::json_opt(row, TABLE, "install_argv")?.unwrap_or_default(),
        warm: row::json_opt(row, TABLE, "warm_argv")?.unwrap_or_default(),
        tools: row::json_opt(row, TABLE, "tool_versions")?.unwrap_or_default(),
        recipe: row::scalar(row, TABLE, "recipe_digest")?,
    }))
}
