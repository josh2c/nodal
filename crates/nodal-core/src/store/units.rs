//! Rows of the `unit` table: the branches with a home directory and a memory.
//!
//! The rule that two open units cannot hold the same branch is a partial unique index
//! in the schema, not a check here, so a race between two `nodal new` runs is decided by
//! the database rather than by whichever process read first.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{
    BranchName, CommitId, Epistemic, Objective, ProjectId, Slug, Timestamp, Unit, UnitId,
    UnitStatus,
};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "unit";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, project_id, slug, objective, objective_epistemic, branch, \
     parent_branch, base_commit, status, created_at, updated_at";

/// Record a new unit.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the slug is taken or another open unit already
/// holds the branch, [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, unit: &Unit) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO unit (id, project_id, slug, objective, objective_epistemic, branch, \
         parent_branch, base_commit, status, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            unit.id.to_string(),
            unit.project_id.to_string(),
            unit.slug.as_str(),
            unit.objective.as_ref().map(Objective::as_str),
            epistemic_name(unit.objective_epistemic)?,
            unit.branch.as_str(),
            unit.parent_branch.as_ref().map(BranchName::as_str),
            unit.base_commit.as_ref().map(CommitId::as_str),
            row::name_of(&unit.status, "unit status")?,
            unit.created_at.unix_seconds(),
            unit.updated_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// One unit by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: UnitId) -> Result<Option<Unit>> {
    let sql = format!("SELECT {COLUMNS} FROM unit WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// The unit a slug names, `None` when there is none.
///
/// # Errors
/// As [`get`].
pub fn find_by_slug(conn: &Connection, project_id: ProjectId, slug: &Slug) -> Result<Option<Unit>> {
    let sql = format!("SELECT {COLUMNS} FROM unit WHERE project_id = ? AND slug = ?");
    row::one(conn, &sql, params![project_id.to_string(), slug.as_str()], decode)
}

/// The open unit holding a branch, `None` when the branch is free.
///
/// # Errors
/// As [`get`].
pub fn find_open_by_branch(
    conn: &Connection,
    project_id: ProjectId,
    branch: &BranchName,
) -> Result<Option<Unit>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM unit WHERE project_id = ? AND branch = ? AND status = 'open'"
    );
    row::one(conn, &sql, params![project_id.to_string(), branch.as_str()], decode)
}

/// Every unit of a project, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list(conn: &Connection, project_id: ProjectId) -> Result<Vec<Unit>> {
    let sql = format!("SELECT {COLUMNS} FROM unit WHERE project_id = ? ORDER BY id");
    row::many(conn, &sql, params![project_id.to_string()], decode)
}

/// Every unit of a project in one state, oldest first.
///
/// # Errors
/// As [`get`].
pub fn list_by_status(
    conn: &Connection,
    project_id: ProjectId,
    status: UnitStatus,
) -> Result<Vec<Unit>> {
    let sql = format!("SELECT {COLUMNS} FROM unit WHERE project_id = ? AND status = ? ORDER BY id");
    let key = params![project_id.to_string(), row::name_of(&status, "unit status")?];
    row::many(conn, &sql, key, decode)
}

/// Move a unit to another state; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when reopening a unit whose branch another open unit
/// now holds, [`crate::Error::Store`] on any other failure.
pub fn update_status(
    conn: &Connection,
    id: UnitId,
    status: UnitStatus,
    at: Timestamp,
) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE unit SET status = ?, updated_at = ? WHERE id = ?",
        params![row::name_of(&status, "unit status")?, at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Point a unit at another branch; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when another open unit holds the branch,
/// [`crate::Error::Store`] on any other failure.
pub fn update_branch(
    conn: &Connection,
    id: UnitId,
    branch: &BranchName,
    at: Timestamp,
) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE unit SET branch = ?, updated_at = ? WHERE id = ?",
        params![branch.as_str(), at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Record what a unit is for; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn update_objective(
    conn: &Connection,
    id: UnitId,
    objective: Option<&Objective>,
    epistemic: Option<Epistemic>,
    at: Timestamp,
) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE unit SET objective = ?, objective_epistemic = ?, updated_at = ? WHERE id = ?",
        params![
            objective.map(Objective::as_str),
            epistemic_name(epistemic)?,
            at.unix_seconds(),
            id.to_string(),
        ],
    )?;
    Ok(changed == 1)
}

/// How an objective is known, as the one word the column holds.
fn epistemic_name(epistemic: Option<Epistemic>) -> Result<Option<String>> {
    epistemic.map(|known| row::name_of(&known, "objective epistemic")).transpose()
}

/// Turn a row into a unit.
fn decode(row: &Row<'_>) -> Result<Unit> {
    Ok(Unit {
        id: row::scalar::<UnitId>(row, TABLE, "id")?,
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        slug: row::scalar::<Slug>(row, TABLE, "slug")?,
        objective: row::scalar_opt::<Objective>(row, TABLE, "objective")?,
        objective_epistemic: row::name_opt::<Epistemic>(row, TABLE, "objective_epistemic")?,
        branch: row::scalar::<BranchName>(row, TABLE, "branch")?,
        parent_branch: row::scalar_opt::<BranchName>(row, TABLE, "parent_branch")?,
        base_commit: row::scalar_opt::<CommitId>(row, TABLE, "base_commit")?,
        status: row::name::<UnitStatus>(row, TABLE, "status")?,
        created_at: row::stamp(row, TABLE, "created_at")?,
        updated_at: row::stamp(row, TABLE, "updated_at")?,
    })
}
