//! Sessions, derived rather than declared.
//!
//! A session row says an actor is attached to a unit's environment. Nothing reports
//! that: it is read from the process table, because a process that carries `NODAL_ID`
//! is in the unit that variable names, whether it is a shell a person opened, an IDE
//! terminal, or an agent. There is no attach command to forget to run and no exit hook
//! to fire twice; a process that ends stops being seen and the row is closed on the
//! next scan.
//!
//! Deriving is pure ([`derive`]) and reconciling is the part that writes
//! ([`reconcile`]). A prompt hook is an optional accelerant and nothing more: it is
//! what puts `NODAL_ID` into a shell with no direnv, so the same scan sees it.
//!
//! One kind of session is declared rather than derived, and this module leaves it
//! alone. A tether — a session carrying a process group, written by `nodal run
//! --tether` — is opened and closed by the group's liveness, not by a scan of who
//! carries which variable ([`crate::runtime::run`]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::model::{Actor, EnvId, Environment, HostName, Session, SessionId, Timestamp, UnitId};
use crate::runtime::actor;
use crate::runtime::processes::{Processes, Running};
use crate::store::{environments, sessions};
use crate::{Result, env};

/// One process seen inside a unit's home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// The process identifier.
    pub pid: u32,
    /// The unit the process is in.
    pub unit: UnitId,
    /// The home it names, which is what says which materialisation it is in.
    pub root: PathBuf,
    /// Who the process belongs to.
    pub actor: Actor,
}

/// What one reconciliation changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Sessions opened, because a process was seen that no open row covers.
    pub opened: usize,
    /// Sessions ended, because an open row's process is gone.
    pub ended: usize,
}

/// The processes that are inside a unit, from a scan of the process table.
///
/// A process with no `NODAL_ID`, or one whose value is not a unit identifier, is not in
/// a unit and is left out. This function reads no registry and touches no disk.
///
/// # Errors
/// [`crate::Error::InvalidValue`] only through [`actor::from_vars`], which falls back
/// rather than failing.
pub fn derive(running: &[Running]) -> Result<Vec<Attached>> {
    let mut attached = Vec::new();
    for process in running {
        let Some(unit) = process.var(env::vars::ID).and_then(|id| UnitId::parse(id).ok()) else {
            continue;
        };
        let root = process.var(env::vars::ROOT).unwrap_or_default();
        attached.push(Attached {
            pid: process.pid,
            unit,
            root: PathBuf::from(root),
            actor: actor::from_vars(&process.vars)?,
        });
    }
    Ok(attached)
}

/// Scan this machine and bring the registry's session rows up to date.
///
/// # Errors
/// Whatever the scan reports, and [`crate::Error::Store`] on a failed statement.
pub fn observe(
    conn: &Connection,
    processes: &dyn Processes,
    host: &HostName,
    now: Timestamp,
) -> Result<Reconciled> {
    let running = processes.scan()?;
    let attached = derive(&running)?;
    reconcile(conn, host, &attached, now)
}

/// Bring this machine's session rows up to date, and say nothing when it cannot.
///
/// This is what a command calls on its way past: a scan is a nicety, and a host whose
/// process table Nodal cannot read is not a reason to fail the work the person asked
/// for. What happened is a debug line.
pub fn observe_quietly(conn: &Connection) {
    let host = crate::model::HostName::current();
    match observe(conn, &crate::runtime::processes::Live, &host, Timestamp::now()) {
        Ok(change) => tracing::debug!(opened = change.opened, ended = change.ended, "sessions"),
        Err(error) => tracing::debug!(%error, "the session scan did not run"),
    }
}

/// Open a row for each process that has none, and end each row whose process is gone.
///
/// Only rows of environments on `host` are ended: a session on another machine is not
/// this machine's to close, and its absence from this process table says nothing.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn reconcile(
    conn: &Connection,
    host: &HostName,
    attached: &[Attached],
    now: Timestamp,
) -> Result<Reconciled> {
    let mut homes = Homes::default();
    let live = resolve(conn, attached, &mut homes)?;
    let open = sessions::list_open_all(conn)?;
    let mut change = Reconciled::default();
    for (environment, process) in &live {
        if open.iter().any(|row| row.environment_id == *environment && row.pid == Some(process.pid))
        {
            continue;
        }
        sessions::insert(conn, &row_for(*environment, process, now))?;
        change.opened += 1;
    }
    for row in &open {
        if !ends_here(conn, row, host, &live)? {
            continue;
        }
        if sessions::end(conn, row.id, now)? {
            change.ended += 1;
        }
    }
    Ok(change)
}

/// The environments already looked up, so a scan of many processes in one unit reads
/// the registry once for it.
#[derive(Debug, Default)]
struct Homes(BTreeMap<UnitId, Vec<Environment>>);

impl Homes {
    /// Every materialisation of a unit.
    fn of(&mut self, conn: &Connection, unit: UnitId) -> Result<&[Environment]> {
        if let std::collections::btree_map::Entry::Vacant(slot) = self.0.entry(unit) {
            slot.insert(environments::list_for_unit(conn, unit)?);
        }
        Ok(self.0.get(&unit).map_or(&[][..], Vec::as_slice))
    }
}

/// Which environment each seen process is in.
///
/// A process whose `NODAL_ROOT` matches no materialisation of its unit is left out: the
/// home it names has been reclaimed, or the registry is another machine's.
fn resolve<'a>(
    conn: &Connection,
    attached: &'a [Attached],
    homes: &mut Homes,
) -> Result<Vec<(EnvId, &'a Attached)>> {
    let mut live = Vec::new();
    for process in attached {
        let found = homes
            .of(conn, process.unit)?
            .iter()
            .find(|environment| environment.home == process.root)
            .map(|environment| environment.id);
        if let Some(environment) = found {
            live.push((environment, process));
        }
    }
    Ok(live)
}

/// Whether this machine is the one that may close `row`.
///
/// A tether is never closed here. Its row records a process group, and the group
/// outlives the process this row's `pid` names: a development server replaces its own
/// leader, and `nodal run --tether` may have been killed long ago. Whether that group
/// has gone is a question for the one thing that can ask it, which is a signal
/// ([`crate::runtime::stop`]), not for a scan of who is carrying which variable.
fn ends_here(
    conn: &Connection,
    row: &Session,
    host: &HostName,
    live: &[(EnvId, &Attached)],
) -> Result<bool> {
    if row.pgid.is_some() {
        return Ok(false);
    }
    if live.iter().any(|(environment, process)| {
        *environment == row.environment_id && Some(process.pid) == row.pid
    }) {
        return Ok(false);
    }
    let Some(environment) = environments::get(conn, row.environment_id)? else {
        return Ok(false);
    };
    Ok(&environment.host == host)
}

/// The row one seen process becomes.
fn row_for(environment_id: EnvId, process: &Attached, now: Timestamp) -> Session {
    Session {
        id: SessionId::from_ulid(ulid::Ulid::new()),
        environment_id,
        actor: process.actor.clone(),
        pid: Some(process.pid),
        pgid: None,
        started_at: now,
        ended_at: None,
    }
}
