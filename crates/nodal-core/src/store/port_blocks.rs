//! Rows of the `port_block` table: the range of ports one project hands out from.
//!
//! A block is written once and never moved. Which range a new project is given is a
//! services rule ([`crate::services::ports`]), not a store one, so these functions
//! record a block and read the blocks that are taken; they do not choose.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{PortBlock, ProjectId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "port_block";

/// Every column [`decode`] reads.
const COLUMNS: &str = "project_id, first, last";

/// Give a project its block.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the project already has a block or another
/// project holds that range, [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, block: &PortBlock) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO port_block (project_id, first, last) VALUES (?, ?, ?)",
        params![block.project_id.to_string(), block.first, block.last],
    )?;
    Ok(())
}

/// A project's block, `None` when it does not have one yet.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, project_id: ProjectId) -> Result<Option<PortBlock>> {
    let sql = format!("SELECT {COLUMNS} FROM port_block WHERE project_id = ?");
    row::one(conn, &sql, params![project_id.to_string()], decode)
}

/// Every block, lowest first: what an allocator reads to find a range nobody holds.
///
/// # Errors
/// As [`get`].
pub fn list(conn: &Connection) -> Result<Vec<PortBlock>> {
    row::many(conn, &format!("SELECT {COLUMNS} FROM port_block ORDER BY first"), [], decode)
}

/// Turn a row into a block.
fn decode(row: &Row<'_>) -> Result<PortBlock> {
    Ok(PortBlock {
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        first: row::number::<u16>(row, TABLE, "first")?,
        last: row::number::<u16>(row, TABLE, "last")?,
    })
}
