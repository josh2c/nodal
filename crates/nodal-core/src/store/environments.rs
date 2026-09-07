//! Rows of the `environment` table: the materialisations of a unit on a host.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{
    BaseId, DbName, EnvId, EnvState, Environment, HostName, Ports, SchemaFp, Timestamp, UnitId,
    WorkspaceFp,
};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "environment";

/// Every column [`decode`] reads.
const COLUMNS: &str = "id, unit_id, attempt, home, managed, base_id, ws_fp_materialized, \
     schema_fp_materialized, host, db_name, ports, fixed_port, state, created_at, last_active";

/// Record a materialisation.
///
/// # Errors
/// [`crate::Error::StoreConflict`] when the unit already has that attempt,
/// [`crate::Error::Store`] on any other failure.
pub fn insert(conn: &Connection, environment: &Environment) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO environment (id, unit_id, attempt, home, managed, base_id, \
         ws_fp_materialized, schema_fp_materialized, host, db_name, ports, fixed_port, state, \
         created_at, last_active) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            environment.id.to_string(),
            environment.unit_id.to_string(),
            environment.attempt,
            row::path_of(&environment.home)?,
            environment.managed,
            environment.base_id.map(|base| base.to_string()),
            environment.ws_fp_materialized.as_ref().map(|fp| fp.0.as_str()),
            environment.schema_fp_materialized.as_ref().map(|fp| fp.0.as_str()),
            environment.host.as_str(),
            environment.db_name.as_ref().map(DbName::as_str),
            row::json_of(&environment.ports, "port allocation")?,
            environment.fixed_port,
            row::name_of(&environment.state, "environment state")?,
            environment.created_at.unix_seconds(),
            environment.last_active.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// One environment by identity, `None` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, id: EnvId) -> Result<Option<Environment>> {
    let sql = format!("SELECT {COLUMNS} FROM environment WHERE id = ?");
    row::one(conn, &sql, params![id.to_string()], decode)
}

/// Every materialisation of a unit, oldest attempt first.
///
/// # Errors
/// As [`get`].
pub fn list_for_unit(conn: &Connection, unit_id: UnitId) -> Result<Vec<Environment>> {
    let sql = format!("SELECT {COLUMNS} FROM environment WHERE unit_id = ? ORDER BY attempt");
    row::many(conn, &sql, params![unit_id.to_string()], decode)
}

/// The current materialisation of a unit: its highest attempt.
///
/// # Errors
/// As [`get`].
pub fn latest_for_unit(conn: &Connection, unit_id: UnitId) -> Result<Option<Environment>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM environment WHERE unit_id = ? ORDER BY attempt DESC LIMIT 1"
    );
    row::one(conn, &sql, params![unit_id.to_string()], decode)
}

/// Every environment on this machine, oldest first.
///
/// What the placement guard reads: a home may not be created inside one that is already
/// there, whatever state it is in.
///
/// # Errors
/// As [`get`].
pub fn list_all(conn: &Connection) -> Result<Vec<Environment>> {
    let sql = format!("SELECT {COLUMNS} FROM environment ORDER BY id");
    row::many(conn, &sql, [], decode)
}

/// Every environment in one state, oldest first. What `nodal ls` and reclaim read.
///
/// # Errors
/// As [`get`].
pub fn list_by_state(conn: &Connection, state: EnvState) -> Result<Vec<Environment>> {
    let sql = format!("SELECT {COLUMNS} FROM environment WHERE state = ? ORDER BY id");
    let key = params![row::name_of(&state, "environment state")?];
    row::many(conn, &sql, key, decode)
}

/// Move an environment to another state; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn update_state(conn: &Connection, id: EnvId, state: EnvState, at: Timestamp) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE environment SET state = ?, last_active = ? WHERE id = ?",
        params![row::name_of(&state, "environment state")?, at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Record what is now installed here. Staleness is the difference between this and what
/// the tree asks for, so it is written only when a sync actually changed something.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn set_materialized(
    conn: &Connection,
    id: EnvId,
    workspace: Option<&WorkspaceFp>,
    schema: Option<&SchemaFp>,
) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE environment SET ws_fp_materialized = ?, schema_fp_materialized = ? WHERE id = ?",
        params![workspace.map(|fp| fp.0.as_str()), schema.map(|fp| fp.0.as_str()), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Record the ports an environment holds; `false` when there is no such row.
///
/// The grants themselves are rows of `port_allocation`, written when the environment
/// exists to hold them; this column is the same set in the form a reader wants, so both
/// are written in the one transaction that finishes a create.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreEncode`] when the
/// ports cannot be encoded.
pub fn set_ports(conn: &Connection, id: EnvId, ports: &Ports) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE environment SET ports = ? WHERE id = ?",
        params![row::json_of(ports, "port allocation")?, id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Record activity in an environment; `false` when there is no such row.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn touch(conn: &Connection, id: EnvId, at: Timestamp) -> Result<bool> {
    let changed = row::write(
        conn,
        "UPDATE environment SET last_active = ? WHERE id = ?",
        params![at.unix_seconds(), id.to_string()],
    )?;
    Ok(changed == 1)
}

/// Turn a row into an environment.
fn decode(row: &Row<'_>) -> Result<Environment> {
    Ok(Environment {
        id: row::scalar::<EnvId>(row, TABLE, "id")?,
        unit_id: row::scalar::<UnitId>(row, TABLE, "unit_id")?,
        attempt: row::number::<u32>(row, TABLE, "attempt")?,
        home: row::path(row, TABLE, "home")?,
        managed: row::plain::<bool>(row, TABLE, "managed")?,
        base_id: row::scalar_opt::<BaseId>(row, TABLE, "base_id")?,
        ws_fp_materialized: row::scalar_opt(row, TABLE, "ws_fp_materialized")?.map(WorkspaceFp),
        schema_fp_materialized: row::scalar_opt(row, TABLE, "schema_fp_materialized")?
            .map(SchemaFp),
        host: row::scalar::<HostName>(row, TABLE, "host")?,
        db_name: row::scalar_opt::<DbName>(row, TABLE, "db_name")?,
        ports: row::json::<Ports>(row, TABLE, "ports")?,
        fixed_port: row::number_opt::<u16>(row, TABLE, "fixed_port")?,
        state: row::name::<EnvState>(row, TABLE, "state")?,
        created_at: row::stamp(row, TABLE, "created_at")?,
        last_active: row::stamp(row, TABLE, "last_active")?,
    })
}

/// How many environments hold a base against eviction: its pin count.
///
/// The registry keeps no pin column. A pin *is* an environment row pointing at the
/// base, so counting them is the whole answer and there is no second number that can
/// disagree with the foreign key the database already enforces.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn count_for_base(conn: &Connection, base_id: BaseId) -> Result<u32> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM environment WHERE base_id = ?",
            params![base_id.to_string()],
            |row| row.get(0),
        )
        .map_err(row::store_error(conn))?;
    u32::try_from(count).map_err(|_| crate::Error::StoreEncode { kind: "pin count" })
}
