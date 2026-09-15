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

/// Give up a unit's handle, so the name is free for the next unit of the project.
///
/// A handle is unique among a project's units, and the schema holds that with a unique
/// index over every row rather than over the open ones. So a unit that was reclaimed
/// went on owning its name, and a person who made the unit again got `<name>-2` — a
/// second unit on the first one's branch.
///
/// The row is not deleted and nothing of it is lost. It keeps its own identifier, its
/// branch, its objective and its place in the log, and the trash entry written beside it
/// keeps the name a person typed. What it gives up is the handle, which is the one part
/// of a unit that is a claim on something another unit may want.
///
/// The released handle is the old one with the unit's own identifier after it. That is
/// unique among the project's units by construction, so the write cannot fail on the
/// index, and it reads as what it is. The handle is truncated first, where it has to be,
/// so the result is inside the length a handle may have.
///
/// Doing this twice is doing it once: a handle that already ends in the unit's own
/// identifier has already been released, and is returned as it is.
///
/// # Errors
/// [`crate::Error::InvalidValue`] when the released handle is not a handle, and
/// [`crate::Error::Store`] when the row could not be written.
pub fn release_slug(conn: &Connection, unit: &Unit, at: Timestamp) -> Result<Slug> {
    let released = released(&unit.slug, unit.id)?;
    if released == unit.slug {
        return Ok(released);
    }
    row::write(
        conn,
        "UPDATE unit SET slug = ?, updated_at = ? WHERE id = ?",
        params![released.as_str(), at.unix_seconds(), unit.id.to_string()],
    )?;
    Ok(released)
}

/// The unit that held this handle until a reclaim released it.
///
/// Asked only after no unit holds the handle. The released form is the handle with the
/// unit's own identifier after it ([`released`]), so the question is put the way it was
/// answered: for each of the project's archived units, what would this handle have
/// become in its hands, and is that the handle it has.
///
/// **More than one row can match, and the newest wins.** A name that is made, reclaimed,
/// made again and reclaimed again leaves two archived units whose released handles both
/// come from it. They are told apart by their own identifiers and neither is wrong; what
/// the name means is the last unit that held it, so that is the one answered. Unit
/// identifiers are ordered by the moment they were made, so the last match in the
/// registry's own order is that unit.
///
/// Nothing else records the released name. A reclaim of a checkout adopted in place
/// writes no trash entry, so a record kept there would answer for some reclaims and not
/// for others; the handle the unit carries is the record every reclaim leaves.
///
/// # Errors
/// As [`get`].
pub fn find_released_by_slug(
    conn: &Connection,
    project_id: ProjectId,
    slug: &Slug,
) -> Result<Option<Unit>> {
    let mut newest = None;
    for unit in list_by_status(conn, project_id, UnitStatus::Archived)? {
        if released(slug, unit.id)? == unit.slug {
            newest = Some(unit);
        }
    }
    Ok(newest)
}

/// The handle a released unit takes: its own, with its identifier after it.
///
/// The identifier is lower case because a handle is, and it is 26 characters, so the
/// handle in front of it is cut to what is left of the 64 a handle may hold. A cut that
/// ends on the separator takes the separator too, because a handle holds no empty group.
fn released(slug: &Slug, id: UnitId) -> Result<Slug> {
    let id = id.to_string().to_lowercase();
    if slug.as_str().ends_with(&id) {
        return Ok(slug.clone());
    }
    let room = (Slug::MAX_LEN as usize).saturating_sub(id.len() + 1);
    let kept = slug.as_str().get(..room).unwrap_or(slug.as_str()).trim_end_matches('-');
    Slug::parse(format!("{kept}-{id}"))
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
