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

use rusqlite::Connection;

pub use crate::store::migrations::SCHEMA_VERSION;
use crate::{Error, Result};

/// Settings of this connection, applied in order.
///
/// The busy timeout comes first on purpose: it is what makes every later statement wait
/// for the write lock rather than fail. `NORMAL` synchronisation is the documented safe
/// pairing with WAL — a crash cannot corrupt the database, only lose the last commits,
/// and those are events the unit's own `events.jsonl` still holds.
const PRAGMAS: &[(&str, &str)] =
    &[("busy_timeout", BUSY_TIMEOUT_MS), ("synchronous", "NORMAL"), ("foreign_keys", "ON")];

/// How long a writer waits for the write lock before giving up.
const BUSY_TIMEOUT_MS: &str = "5000";

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
    /// The journal mode is a property of the database file rather than of the
    /// connection, so it is read before it is written: switching it takes an exclusive
    /// lock, and a registry that several processes open at once would serialise on that
    /// lock every time for a change that is almost never needed.
    fn configure(&self) -> Result<()> {
        for (name, value) in PRAGMAS {
            self.conn
                .execute_batch(&format!("PRAGMA {name} = {value};"))
                .map_err(row::store_error(&self.conn))?;
        }
        if self.journal_mode()?.eq_ignore_ascii_case(WAL) {
            return Ok(());
        }
        self.conn
            .execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(row::store_error(&self.conn))?;
        let found = self.journal_mode()?;
        if found.eq_ignore_ascii_case(WAL) {
            Ok(())
        } else {
            Err(Error::StoreJournalMode { path: self.path.clone(), found })
        }
    }

    /// How this database journals writes.
    fn journal_mode(&self) -> Result<String> {
        self.conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(row::store_error(&self.conn))
    }
}
