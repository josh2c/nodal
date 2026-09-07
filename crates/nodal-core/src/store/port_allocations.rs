//! Rows of the `port_allocation` table: the ports environments hold.
//!
//! A port is claimed by writing the row, never by reading first and then writing: two
//! environments racing for the same port are separated by the primary key, and the
//! loser is told the port is taken instead of quietly taking it as well.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{EnvId, PortAllocation, PortName, ProjectId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "port_allocation";

/// Every column [`decode`] reads.
const COLUMNS: &str = "port, project_id, environment_id, name";

/// Claim a port. `false` when another environment already holds it, or when this
/// environment already holds a port under that name.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn claim(conn: &Connection, allocation: &PortAllocation) -> Result<bool> {
    let claimed = row::write(
        conn,
        "INSERT INTO port_allocation (port, project_id, environment_id, name) \
         VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
        params![
            allocation.port,
            allocation.project_id.to_string(),
            allocation.environment_id.to_string(),
            allocation.name.as_str(),
        ],
    )?;
    Ok(claimed == 1)
}

/// Who holds a port, `None` when nobody does.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, port: u16) -> Result<Option<PortAllocation>> {
    let sql = format!("SELECT {COLUMNS} FROM port_allocation WHERE port = ?");
    row::one(conn, &sql, params![port], decode)
}

/// Every port an environment holds, lowest first.
///
/// # Errors
/// As [`get`].
pub fn list_for_environment(
    conn: &Connection,
    environment_id: EnvId,
) -> Result<Vec<PortAllocation>> {
    let sql =
        format!("SELECT {COLUMNS} FROM port_allocation WHERE environment_id = ? ORDER BY port");
    row::many(conn, &sql, params![environment_id.to_string()], decode)
}

/// Every port held in a range, lowest first: what an allocator reads to skip the ports
/// of a block that are already taken.
///
/// # Errors
/// As [`get`].
pub fn list_in_range(conn: &Connection, first: u16, last: u16) -> Result<Vec<PortAllocation>> {
    let sql =
        format!("SELECT {COLUMNS} FROM port_allocation WHERE port BETWEEN ? AND ? ORDER BY port");
    row::many(conn, &sql, params![first, last], decode)
}

/// The port an environment holds under one name, `None` when it holds none.
///
/// # Errors
/// As [`get`].
pub fn find_by_name(
    conn: &Connection,
    environment_id: EnvId,
    name: &PortName,
) -> Result<Option<PortAllocation>> {
    let sql =
        format!("SELECT {COLUMNS} FROM port_allocation WHERE environment_id = ? AND name = ?");
    row::one(conn, &sql, params![environment_id.to_string(), name.as_str()], decode)
}

/// Give back every port an environment holds, and report how many rows went.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn release_all(conn: &Connection, environment_id: EnvId) -> Result<usize> {
    row::write(
        conn,
        "DELETE FROM port_allocation WHERE environment_id = ?",
        params![environment_id.to_string()],
    )
}

/// Turn a row into an allocation.
fn decode(row: &Row<'_>) -> Result<PortAllocation> {
    Ok(PortAllocation {
        port: row::number::<u16>(row, TABLE, "port")?,
        project_id: row::scalar::<ProjectId>(row, TABLE, "project_id")?,
        environment_id: row::scalar::<EnvId>(row, TABLE, "environment_id")?,
        name: row::scalar::<PortName>(row, TABLE, "name")?,
    })
}
