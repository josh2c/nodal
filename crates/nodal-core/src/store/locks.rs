//! Rows of the `lock` table: who may write a unit.
//!
//! One row per unit, and the row is the whole record of the hold: the host, the actor,
//! the process that took it, when it began and when an entry last touched it.
//!
//! Two rules live here rather than in the callers. A take succeeds only when the lock is
//! free, has lapsed, or is already this actor's on this host, which is one statement and
//! therefore one write with no read-then-write between two processes. And a hand-off is
//! a separate call from a take, because moving a hold away from somebody is a thing a
//! person asks for and never a thing a retry does.

use rusqlite::{Connection, Row, params};

use crate::Result;
use crate::model::{Actor, ActorKind, ActorName, Holding, HostName, Lock, Timestamp, UnitId};
use crate::store::row;

/// The table these functions read and write.
const TABLE: &str = "lock";

/// Every column [`decode`] reads.
const COLUMNS: &str = "unit_id, host, actor_kind, actor_name, pid, pid_started_at, session, \
     taken_at, refreshed_at, expires_at";

/// Take the write on a unit, or refresh a hold this actor already has.
///
/// `false` when somebody else holds it and the hold has not lapsed. The caller decides
/// what to say about that, because what a refusal should print is a question for the
/// verb and not for the table.
///
/// A row that has lapsed is taken over in the same statement that would have inserted
/// one, so two processes racing for a lapsed lock produce one winner and one `false`
/// rather than two holders.
///
/// A row with a null `actor_name` is taken over too. It was written before locks carried
/// an actor, so it names a host and holds nobody, and the rule here is the one
/// [`crate::model::Lock::holds_anyone`] states: a row that holds nobody refuses nobody.
///
/// An actor matches only where the lineage matches as well, so that two processes of one
/// name racing for a free lock make one holder. A row with a null `session` matches any
/// lineage, for the same reason a null `actor_name` matches anybody: it was written
/// before locks carried a lineage, or by a host that would not say, and a record that
/// states nothing refuses nobody. Which of two live lineages may write is decided by
/// [`crate::runtime::lock::enter`]; this statement only keeps a race from making two.
///
/// `idle_deadline` is the instant the current holder's idle window runs out, computed
/// by the caller from the recipe. It is passed in rather than computed here because the
/// window is a project's setting and this module reads no recipe.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn take(conn: &Connection, lock: &Lock, now: Timestamp, idle_deadline: i64) -> Result<bool> {
    let taken = row::write(
        conn,
        "INSERT INTO lock (unit_id, host, actor_kind, actor_name, pid, pid_started_at, session, \
         taken_at, refreshed_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (unit_id) DO UPDATE SET host = excluded.host, \
         actor_kind = excluded.actor_kind, actor_name = excluded.actor_name, \
         pid = excluded.pid, pid_started_at = excluded.pid_started_at, \
         session = excluded.session, \
         refreshed_at = excluded.refreshed_at, \
         expires_at = excluded.expires_at, \
         taken_at = CASE WHEN lock.host = excluded.host \
         AND lock.actor_name IS excluded.actor_name \
         AND lock.actor_kind IS excluded.actor_kind \
         AND (lock.session IS NULL OR lock.session IS excluded.session) \
         THEN lock.taken_at ELSE excluded.taken_at END \
         WHERE lock.expires_at <= ? OR ? >= ? OR lock.actor_name IS NULL \
         OR (lock.host = excluded.host AND lock.actor_name IS excluded.actor_name \
         AND lock.actor_kind IS excluded.actor_kind \
         AND (lock.session IS NULL OR lock.session IS excluded.session))",
        params![
            lock.unit_id.to_string(),
            lock.host.as_str(),
            kind_of(lock)?,
            name_of(lock),
            pid_of(lock),
            started_of(lock),
            lock.session,
            lock.taken_at.unix_seconds(),
            lock.refreshed_at.unix_seconds(),
            lock.expires_at.unix_seconds(),
            now.unix_seconds(),
            now.unix_seconds(),
            idle_deadline,
        ],
    )?;
    Ok(taken == 1)
}

/// Move the hold to `lock`, whoever held it and whatever the clock says.
///
/// This is what `--take` runs, and it is the only call that takes a hold away from an
/// actor who still has it. It is a separate function from [`take`] so that no retry, no
/// refresh and no ordinary entry can reach the behaviour by accident.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn hand_over(conn: &Connection, lock: &Lock) -> Result<()> {
    row::write(
        conn,
        "INSERT INTO lock (unit_id, host, actor_kind, actor_name, pid, pid_started_at, session, \
         taken_at, refreshed_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (unit_id) DO UPDATE SET host = excluded.host, \
         actor_kind = excluded.actor_kind, actor_name = excluded.actor_name, \
         pid = excluded.pid, pid_started_at = excluded.pid_started_at, \
         session = excluded.session, taken_at = excluded.taken_at, \
         refreshed_at = excluded.refreshed_at, expires_at = excluded.expires_at",
        params![
            lock.unit_id.to_string(),
            lock.host.as_str(),
            kind_of(lock)?,
            name_of(lock),
            pid_of(lock),
            started_of(lock),
            lock.session,
            lock.taken_at.unix_seconds(),
            lock.refreshed_at.unix_seconds(),
            lock.expires_at.unix_seconds(),
        ],
    )?;
    Ok(())
}

/// Who holds the write on a unit, `None` when nobody does.
///
/// The row is answered as it stands, lapsed or not. Whether a lapsed hold counts is a
/// question that needs the project's idle window, and this module reads no recipe.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement, [`crate::Error::StoreRow`] when a
/// column does not hold a value the model accepts.
pub fn get(conn: &Connection, unit_id: UnitId) -> Result<Option<Lock>> {
    let sql = format!("SELECT {COLUMNS} FROM lock WHERE unit_id = ?");
    row::one(conn, &sql, params![unit_id.to_string()], decode)
}

/// Every lock the registry holds, by unit.
///
/// The list reads this once and shows a holder per row, so it asks for all of them
/// rather than one query per unit.
///
/// # Errors
/// As [`get`].
pub fn list_all(conn: &Connection) -> Result<Vec<Lock>> {
    let sql = format!("SELECT {COLUMNS} FROM lock ORDER BY unit_id");
    row::many(conn, &sql, params![], decode)
}

/// Every lock a host holds.
///
/// # Errors
/// As [`get`].
pub fn list_for_host(conn: &Connection, host: &HostName) -> Result<Vec<Lock>> {
    let sql = format!("SELECT {COLUMNS} FROM lock WHERE host = ? ORDER BY unit_id");
    row::many(conn, &sql, params![host.as_str()], decode)
}

/// Give up the write on a unit. Only the holder releases it. `false` when it held none.
///
/// A reclaim releases the unit's lock with this, so a home that has gone leaves no row
/// claiming a writer.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn release(conn: &Connection, unit_id: UnitId, holder: &HostName) -> Result<bool> {
    let released = row::write(
        conn,
        "DELETE FROM lock WHERE unit_id = ? AND host = ?",
        params![unit_id.to_string(), holder.as_str()],
    )?;
    Ok(released == 1)
}

/// The identifier of the process a hold was taken by, `None` when it records none.
const fn pid_of(lock: &Lock) -> Option<u32> {
    match &lock.process {
        Some(held) => Some(held.pid),
        None => None,
    }
}

/// When that process started, in seconds since the epoch, `None` where it is undated.
///
/// A row with a process and no instant is the one a reading cannot resolve, and it is
/// written as it was read: this host would not date the process, so nothing is claimed
/// about when it began.
fn started_of(lock: &Lock) -> Option<i64> {
    lock.process.as_ref()?.started_at.map(Timestamp::unix_seconds)
}

/// The process a row records, pinned to when it started.
///
/// The instant is read only where an identifier is there. A `pid_started_at` beside a
/// null `pid` would be a pin on nothing, which no write here produces and which reading
/// as a holder would name a process the registry never recorded.
fn process_of(row: &Row<'_>) -> Result<Option<Holding>> {
    let Some(pid) = row::number_opt::<u32>(row, TABLE, "pid")? else { return Ok(None) };
    Ok(Some(Holding { pid, started_at: row::stamp_opt(row, TABLE, "pid_started_at")? }))
}

/// The stored spelling of a lock's actor kind, `None` when it holds no actor.
fn kind_of(lock: &Lock) -> Result<Option<String>> {
    lock.actor.as_ref().map(|actor| row::name_of(&actor.kind, "actor kind")).transpose()
}

/// The stored spelling of a lock's actor name, `None` when it holds no actor.
fn name_of(lock: &Lock) -> Option<String> {
    lock.actor.as_ref().map(|actor| actor.name.to_string())
}

/// Turn a row into a lock.
///
/// The actor is present only when both of its columns are. A half-written actor is not a
/// state any write here produces, and reading one as a holder would name somebody the
/// registry never recorded.
fn decode(row: &Row<'_>) -> Result<Lock> {
    let kind = row::name_opt::<ActorKind>(row, TABLE, "actor_kind")?;
    let name = row::scalar_opt::<ActorName>(row, TABLE, "actor_name")?;
    Ok(Lock {
        unit_id: row::scalar::<UnitId>(row, TABLE, "unit_id")?,
        host: row::scalar::<HostName>(row, TABLE, "host")?,
        actor: kind.zip(name).map(|(kind, name)| Actor { kind, name }),
        process: process_of(row)?,
        session: row::number_opt::<u32>(row, TABLE, "session")?,
        taken_at: row::stamp(row, TABLE, "taken_at")?,
        refreshed_at: row::stamp(row, TABLE, "refreshed_at")?,
        expires_at: row::stamp(row, TABLE, "expires_at")?,
    })
}
