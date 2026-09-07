//! Rows of the `project` table: the repositories Nodal manages units for.

use std::path::Path;

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{Digest, Project, ProjectId, ProjectName};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "project";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, root, name, recipe_hash, created_at";

/// Record a project. Two projects cannot share a root.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the root is already recorded,
/// [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, project: &Project) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO project (id, root, name, recipe_hash, created_at) VALUES (?, ?, ?, ?, ?)",
        params![
            project.id.to_string(),
            row::path_of(&project.root)?,
            project.name.as_str(),
            project.recipe_hash.as_str(),
            project.created_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// One project by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: ProjectId) -> Result<Option<Project>> {
    row::one(
        conn,
        &format!("SELECT {COLUMNS} FROM project WHERE id = ?"),
        params![id.to_string()],
        decode,
    )
}

/// The project rooted at `root`, `None` when it is not recorded.
///
/// # Errors
/// As [`get`].
pub fn find_by_root(conn: &Connection, root: &Path) -> Result<Option<Project>> {
    let sql = format!("SELECT {COLUMNS} FROM project WHERE root = ?");
    row::one(conn, &sql, params![row::path_of(root)?], decode)
}

/// Every project, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list(conn: &Connection) -> Result<Vec<Project>> {
    row::many(conn, &format!("SELECT {COLUMNS} FROM project ORDER BY id"), [], decode)
}

/// Record that a project's effective recipe changed; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn update_recipe_hash(conn: &Connection, id: ProjectId, hash: &Digest) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE project SET recipe_hash = ? WHERE id = ?",
        params![hash.as_str(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Turn a row into a project.
fn decode(row: &Row<'_>) -> Result<Project> {
    Ok(Project {
        id: row::scalar::<ProjectId>(row, TABLE, "id")?,
        root: row::path(row, TABLE, "root")?,
        name: row::scalar::<ProjectName>(row, TABLE, "name")?,
        recipe_hash: row::scalar::<Digest>(row, TABLE, "recipe_hash")?,
        created_at: row::stamp(row, TABLE, "created_at")?,
    })
}
