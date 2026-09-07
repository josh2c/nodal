//! The registry: one SQLite database holding every row of the domain model.
//!
//! Nodal's whole claim is one list of every unit across every tool, so the registry is
//! the only shared state and it has to survive several processes writing at once — a
//! shell hook, an agent's `nodal note`, and a `nodal ls` in another terminal. That is
//! why the database runs in WAL mode: readers never block the writer, and a writer that
//! finds the lock taken waits for it instead of failing.
//!
//! What lives here and what does not: this module opens the database, runs migrations,
//! and offers one small function per table for reading and writing rows. It holds no
//! business rules. Which rows an operation may write, and in what order, is a lifecycle
//! decision (`docs/code-structure.md`).
//!
//! Instants are stored as whole seconds since the Unix epoch, the form
//! [`Timestamp::unix_seconds`](crate::model::Timestamp::unix_seconds) names. A value
//! read back is therefore truncated to the second; the portable record in a unit's
//! `events.jsonl` keeps the RFC 3339 text, and ordering within the log comes from the
//! identifier, not the timestamp.

pub mod bases;
pub mod environments;
pub mod events;
pub mod leases;
pub mod locks;
pub mod migrations;
pub mod projects;
pub mod row;
pub mod sessions;
pub mod templates;
pub mod units;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;

pub use crate::store::migrations::SCHEMA_VERSION;
use crate::{Error, Result};

/// Settings of this connection, applied after the busy timeout is in place.
///
/// `NORMAL` synchronisation is the documented safe pairing with WAL — a crash cannot
/// corrupt the database, only lose the last commits, and those are events the unit's
/// own `events.jsonl` still holds.
const PRAGMAS: &[(&str, &str)] = &[("synchronous", "NORMAL"), ("foreign_keys", "ON")];

/// How long an open waits for a lock another connection is holding.
///
/// One budget covers both waits an open can make: the busy handler's, which every
/// statement inherits, and [`Store::engage_wal`]'s own, which stands in for the busy
/// handler on the one path SQLite does not consult it.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long [`Store::engage_wal`] sleeps before its first retry. Each further wait
/// doubles, up to [`WAL_BACKOFF_MAX`], so a crowd of openers spreads out instead of
/// waking together to contend for the same lock.
const WAL_BACKOFF_START: Duration = Duration::from_millis(1);

/// The longest [`Store::engage_wal`] sleeps between retries.
const WAL_BACKOFF_MAX: Duration = Duration::from_millis(64);

/// The journal mode every connection must end up in.
const WAL: &str = "wal";

/// An open registry.
///
/// Each process, and each thread within a process, opens its own `Store`; SQLite, not
/// this type, is what serialises the writes.
#[derive(Debug)]
pub struct Store {
    /// The connection every statement runs on.
    conn: Connection,
    /// Where the database file lives, for error messages.
    path: PathBuf,
}

impl Store {
    /// Open the registry at `path`, creating and migrating it if needed.
    ///
    /// Opening is idempotent and safe to do concurrently: two processes that both find
    /// an out-of-date database will not both migrate it.
    ///
    /// # Errors
    /// [`Error::Io`] when the parent directory could not be created, [`Error::Store`]
    /// when the file could not be opened or configured, [`Error::StoreJournalMode`]
    /// when WAL could not be engaged, [`Error::StoreMigration`] when a migration
    /// failed, [`Error::StoreTooNew`] when the file was written by a later version.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        let conn = Connection::open(&path)
            .map_err(|source| Error::Store { path: path.clone(), source: Box::new(source) })?;
        let mut store = Self { conn, path };
        store.configure()?;
        migrations::run(&mut store)?;
        Ok(store)
    }

    /// Where this registry lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The connection the repository functions run on.
    #[must_use]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Begin a write transaction that takes the write lock immediately.
    ///
    /// Deferred transactions are wrong here: one that starts by reading and later
    /// writes can fail to upgrade with no chance to wait, so every transaction that may
    /// write starts as `IMMEDIATE` and the busy timeout does its job.
    ///
    /// # Errors
    /// [`Error::Store`] when the write lock could not be taken within the busy timeout.
    pub fn transaction(&mut self) -> Result<rusqlite::Transaction<'_>> {
        let path = self.path.clone();
        self.conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(move |source| Error::Store { path, source: Box::new(source) })
    }

    /// Apply this connection's settings, then make sure the file is in WAL mode.
    ///
    /// The busy timeout is installed first on purpose: it is what makes every later
    /// statement wait for a lock rather than fail.
    fn configure(&self) -> Result<()> {
        self.conn.busy_timeout(BUSY_TIMEOUT).map_err(row::store_error(&self.conn))?;
        for (name, value) in PRAGMAS {
            self.conn
                .execute_batch(&format!("PRAGMA {name} = {value};"))
                .map_err(row::store_error(&self.conn))?;
        }
        self.engage_wal()
    }

    /// Bring the file into WAL mode, waiting out whoever else is holding it.
    ///
    /// The journal mode is a property of the database file rather than of the
    /// connection, so it is read before it is written: switching it takes an exclusive
    /// lock, and a registry that several processes open at once would serialise on that
    /// lock every time for a change that is almost never needed.
    ///
    /// When the switch is needed, the wait for that exclusive lock has to be ours. The
    /// busy handler covers most of the ways this statement can find the file taken — a
    /// reader's shared lock, a writer's exclusive one — and against those the switch
    /// does block for [`BUSY_TIMEOUT`] like any other statement. It does not cover a
    /// *reserved* lock: against that one SQLite answers `SQLITE_BUSY` immediately,
    /// without consulting the handler, no matter what the busy timeout is set to.
    ///
    /// A reserved lock is exactly what the losing side of this race meets, because it is
    /// the lock the switch itself takes on its way to the exclusive one. Two openers of
    /// a fresh registry that reach this line together are one converting the file and
    /// one told, in no time at all, that it may not — and with fifty openers and two
    /// cores that overlap is a matter of scheduling rather than luck. Waiting is
    /// therefore ours to do: retry until the file is in WAL, which it will be shortly,
    /// put there by whichever opener won.
    ///
    /// A busy answer is not a failure until [`BUSY_TIMEOUT`] has passed, and neither is
    /// a switch that quietly leaves the mode alone: SQLite reports the mode it settled
    /// on, and the losing side of the race is told the old one.
    fn engage_wal(&self) -> Result<()> {
        let deadline = Instant::now() + BUSY_TIMEOUT;
        let mut backoff = WAL_BACKOFF_START;
        let mut found = self.journal_mode()?;
        loop {
            if found.eq_ignore_ascii_case(WAL) {
                return Ok(());
            }
            match self.set_journal_mode_wal() {
                Ok(mode) => found = mode,
                // Busy leaves the mode as it was, which is what the error would report.
                Err(error) if is_busy(&error) => {}
                Err(source) => return Err(row::store_error(&self.conn)(source)),
            }
            if found.eq_ignore_ascii_case(WAL) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::StoreJournalMode { path: self.path.clone(), found });
            }
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(WAL_BACKOFF_MAX);
            found = self.journal_mode()?;
        }
    }

    /// Ask for WAL, and report the mode the file is in afterwards.
    fn set_journal_mode_wal(&self) -> std::result::Result<String, rusqlite::Error> {
        self.conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
    }

    /// How this database journals writes.
    fn journal_mode(&self) -> Result<String> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(row::store_error(&self.conn))
    }
}

/// Whether a failure is another connection holding a lock, and so worth waiting out.
fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::sync::Barrier;
    use std::time::Duration;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{Store, WAL};

    /// How long the writer in the test below holds its reserved lock. Long enough that
    /// an open which does not wait cannot pass by luck, short against the
    /// [`super::BUSY_TIMEOUT`] the waiting one is allowed to spend.
    const HELD: Duration = Duration::from_millis(300);

    /// Opening a registry that is not yet in WAL mode, while another connection holds a
    /// reserved lock on it. This is the standoff behind the fifty-way race on a first
    /// open, held still: the reserved lock is the one the WAL switch takes on its way to
    /// the exclusive lock, so a second opener arriving mid-conversion meets it — and
    /// meeting it is the one case SQLite refuses without consulting the busy handler,
    /// instantly, however long the busy timeout is. Waiting is the open's own job, and
    /// this test is what says so.
    #[test]
    fn opening_waits_out_a_reserved_lock_the_busy_handler_does_not_cover() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("registry.db");
        let holder = Connection::open(&path).unwrap();
        holder.execute_batch("PRAGMA journal_mode = DELETE; CREATE TABLE held (a);").unwrap();

        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            let barrier = &barrier;
            scope.spawn(move || {
                holder.execute_batch("BEGIN; INSERT INTO held VALUES (1);").unwrap();
                barrier.wait();
                std::thread::sleep(HELD);
                holder.execute_batch("COMMIT;").unwrap();
            });
            barrier.wait();
            let store = Store::open(&path).unwrap();
            assert_eq!(store.journal_mode().unwrap(), WAL);
        });
    }
}
