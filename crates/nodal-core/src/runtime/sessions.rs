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
//! One kind of session is declared rather than derived, and a scan leaves it alone. A
//! recorded group — a session carrying a `pgid`, written by `nodal run --tether` or by a
//! recipe hook that backgrounded work ([`crate::lifecycle::hooks`]) — is opened and
//! closed by the group's liveness, not by a scan of who carries which variable.
//!
//! Liveness is a question with an answer, though, and [`close_dead_groups`] asks it. A
//! group that has gone is not work somebody is doing, and a row that says otherwise is
//! the registry claiming an attachment that ended. The question is asked with signal
//! zero, which delivers nothing and only reports whether the group still holds a
//! process, so nothing is signalled to find out. It is asked without a process table,
//! which is why it runs on a host where a scan cannot.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::model::{Actor, EnvId, Environment, HostName, Session, SessionId, Timestamp, UnitId};
use crate::runtime::actor;
use crate::runtime::processes::{Processes, Running};
use crate::runtime::stop::{self, Signals as _, Target};
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
    let now = Timestamp::now();
    // First, and not inside the scan: a recorded group is answered for by a signal
    // rather than by a process table, so this is the half that runs on a host where the
    // other half cannot.
    match close_dead_groups(conn, &host, now) {
        Ok(closed) => tracing::debug!(closed, "recorded groups that have gone"),
        Err(error) => tracing::debug!(%error, "the recorded groups were not read"),
    }
    match observe(conn, &crate::runtime::processes::Live, &host, now) {
        Ok(change) => tracing::debug!(opened = change.opened, ended = change.ended, "sessions"),
        Err(error) => tracing::debug!(%error, "the session scan did not run"),
    }
}

/// Close the row of every recorded process group on this host whose group has gone, and
/// answer with how many were closed.
///
/// An open row carrying a `pgid` is the claim "this group is still the unit's to stop".
/// A group that no longer holds a process makes that claim false, and until something
/// closes the row the registry presents finished work as an attachment: the unit is
/// never idle, `nodal reclaim` lists a session it did not need to end, and a person
/// reading the registry is told somebody is in there. Nothing else closed one. A scan
/// cannot ([`ends_here`]), the `nodal run` that opened a tether may have been killed,
/// and a hook's shell exited long before the group did.
///
/// Three rules keep it from doing harm.
///
/// **Nothing is signalled.** Signal zero delivers nothing; it reports whether the group
/// still holds a process and nothing else. A group that answers is left exactly as it
/// is, row and all.
///
/// **A group this operation would never signal is never closed either.** Group zero
/// addresses this process's own group and one is the system, and a row holding either is
/// a row nothing here can answer for ([`stop::is_spared`]). It stays open and stays
/// visible rather than being quietly tidied away.
///
/// **Only this host's rows.** A session on another machine is not this machine's to
/// close, and a group identifier means nothing across two hosts: the number would name
/// some unrelated local process, or nothing at all.
///
/// A group identifier the system has since handed to something else answers as alive, so
/// the row stays open. That is the conservative direction and the right one: the row is
/// kept, and the operation that acts on it checks liveness again before it signals.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn close_dead_groups(conn: &Connection, host: &HostName, now: Timestamp) -> Result<usize> {
    let mut closed = 0;
    for row in sessions::list_open_all(conn)? {
        let Some(pgid) = row.pgid else { continue };
        let group = Target::Group(pgid);
        if stop::is_spared(group) || stop::Live.alive(group) {
            continue;
        }
        if !on_host(conn, row.environment_id, host)? {
            continue;
        }
        if sessions::end(conn, row.id, now)? {
            closed += 1;
        }
    }
    Ok(closed)
}

/// Whether the environment a row belongs to is one of this host's.
fn on_host(conn: &Connection, environment: EnvId, host: &HostName) -> Result<bool> {
    Ok(environments::get(conn, environment)?.is_some_and(|row| &row.host == host))
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

/// Whether this machine is the one that may close `row`, on the strength of the scan.
///
/// A recorded group is never closed here. Its row holds a process group, and the group
/// outlives the process this row's `pid` names: a development server replaces its own
/// leader, and `nodal run --tether` may have been killed long ago. Whether that group
/// has gone is a question for the one thing that can ask it, which is a signal
/// ([`close_dead_groups`]), not for a scan of who is carrying which variable.
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
    on_host(conn, row.environment_id, host)
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
