//! `nodal gc`: remove what reclaim only moved aside, once its retention has run out.
//!
//! Reclaiming a unit does not delete anything ([`super::reclaim`]). It moves the home
//! to the project's trash directory and writes down the first instant it may go. This
//! is the operation that acts on that instant, and it is the only thing in Nodal that
//! deletes a directory a person worked in.
//!
//! Three things happen, in this order, and the order is the point.
//!
//! **Idle runtime is stopped first.** A process still standing in a home that was
//! reclaimed last week, a container still labelled for a unit that no longer exists —
//! these are what "idle" means here, and they are stopped before the directory under
//! them is removed rather than after, so that nothing is deleting a tree a running
//! process is reading. Runtime belonging to a unit that is *still live* is never
//! touched: a person's development server is not garbage, whatever the clock says
//! about it, and never removing what was not asked for is worth more than the disk.
//!
//! **Then the expired homes go**, one at a time: the directory first, then the row.
//! That order is the same one `nodal base gc` uses and for the same reason. A process
//! killed between them leaves a row pointing at nothing, which the next sweep clears; the
//! other order would leave a directory nothing knows about, which nothing would ever
//! clean up.
//!
//! **Then the lapsed leases are given back.** A lease outlives the environment that
//! took it only when something went wrong, so this is a repair rather than a routine.
//!
//! A sweep is not a [`crate::lifecycle::Plan`], because removing a directory has no
//! undo and a step is required to have one. What it has instead is an order in which
//! being interrupted is safe at every point.
//!
//! The two signals it reads to find that idle runtime — the process table and the
//! container daemon — are not available on every host. A reading that cannot be made is
//! a [`Note`] in the answer and never a failure, which is the contract every attribution
//! signal already has, and it keeps "nothing was running" apart from "I could not look".

use std::path::Path;

use rusqlite::Connection;

use crate::model::{EnvState, Timestamp, Trashed, UnitId};
use crate::output::view::{Leftover, Swept};
use crate::runtime::attribute::{Note, Source};
use crate::runtime::processes::{Processes, Running};
use crate::runtime::stop::{self, Stopped};
use crate::services::docker;
use crate::store::{Store, environments, leases, trash, units};
use crate::{Error, Result};

/// The label a unit's containers carry.
const UNIT_LABEL: &str = "nodal.unit";

/// Remove every expired home, stop the runtime of units that are gone, and give back
/// lapsed leases.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read or written. A directory that
/// will not go is a [`Leftover`] in the answer rather than an error: one home nobody
/// can remove must not stop the rest of the sweep.
pub fn collect(store: &Store, now: Timestamp) -> Result<Swept> {
    let expired = trash::list_expired(store.conn(), now)?;
    let idle = stop_absent(store.conn())?;
    let (removed, freed, mut leftovers) = sweep(store, &expired)?;
    let released = release_lapsed(store.conn(), now, &mut leftovers)?;
    Ok(Swept {
        now,
        removed,
        kept: kept(store.conn(), now)?,
        freed_bytes: Some(freed),
        stopped: idle.stopped,
        containers: idle.containers,
        leases: released,
        notes: idle.notes,
        leftovers,
    })
}

/// What the sweep did about runtime that outlived its unit.
#[derive(Debug, Default)]
struct Idle {
    /// What became of the processes.
    stopped: Stopped,
    /// The containers that went.
    containers: Vec<String>,
    /// The signals that could not be read.
    notes: Vec<Note>,
}

/// The entries whose retention has not run out yet.
fn kept(conn: &Connection, now: Timestamp) -> Result<Vec<Trashed>> {
    Ok(trash::list(conn)?.into_iter().filter(|entry| !entry.has_expired(now)).collect())
}

/// Remove each expired home and forget it, reporting what would not go.
fn sweep(store: &Store, expired: &[Trashed]) -> Result<(Vec<Trashed>, u64, Vec<Leftover>)> {
    let mut removed = Vec::new();
    let mut freed = 0;
    let mut leftovers = Vec::new();
    for entry in expired {
        let size = size_of(&entry.path);
        match remove(&entry.path) {
            Ok(()) => {
                trash::remove(store.conn(), entry.environment_id)?;
                freed += size;
                removed.push(entry.clone());
            }
            Err(why) => leftovers.push(Leftover::new("directory", why.to_string())),
        }
    }
    Ok((removed, freed, leftovers))
}

/// Stop what is still running for a unit whose home is not there any more.
///
/// The scope is deliberately narrow. An environment in [`EnvState::Absent`] has been
/// reclaimed: nothing of it should be running, and anything that is, is left over from
/// before the reclaim rather than work somebody is doing. Every other environment is
/// left alone, however long it has been since anybody touched it.
fn stop_absent(conn: &Connection) -> Result<Idle> {
    let gone = absent_units(conn)?;
    if gone.is_empty() {
        return Ok(Idle::default());
    }
    let mut idle = Idle::default();
    let (pids, containers) = seen(&gone, &mut idle.notes);
    idle.stopped = stop::processes(&stop::Live, &pids, stop::GRACE);
    match docker::remove(&docker::Cli, &containers) {
        Ok(removed) => {
            idle.notes.extend(removed.why.map(|why| Note::new(Source::Docker, why)));
            idle.containers = removed.containers;
        }
        Err(error) => idle.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    Ok(idle)
}

/// What this machine can see of the units that are gone, noting what it cannot look at.
fn seen(gone: &[UnitId], notes: &mut Vec<Note>) -> (Vec<u32>, Vec<String>) {
    let pids = match Processes::scan(&crate::runtime::processes::Live) {
        Ok(running) => running
            .iter()
            .filter(|process| names_a_gone_unit(process, gone))
            .map(|p| p.pid)
            .collect(),
        Err(error) => {
            notes.push(Note::new(Source::Environment, error.to_string()));
            Vec::new()
        }
    };
    let containers = match docker::survey(&docker::Cli) {
        Ok(docker::Survey::Ran(containers)) => labelled(containers, gone),
        Ok(docker::Survey::Unavailable { why }) => {
            notes.push(Note::new(Source::Docker, why));
            Vec::new()
        }
        Err(error) => {
            notes.push(Note::new(Source::Docker, error.to_string()));
            Vec::new()
        }
    };
    (pids, containers)
}

/// The units every one of whose materialisations has been reclaimed.
fn absent_units(conn: &Connection) -> Result<Vec<UnitId>> {
    let mut gone = Vec::new();
    for unit in environments::list_by_state(conn, EnvState::Absent)? {
        let live = environments::list_for_unit(conn, unit.unit_id)?
            .iter()
            .any(|row| row.state != EnvState::Absent);
        if !live && units::get(conn, unit.unit_id)?.is_some() && !gone.contains(&unit.unit_id) {
            gone.push(unit.unit_id);
        }
    }
    Ok(gone)
}

/// Whether a process says it belongs to one of these units.
fn names_a_gone_unit(process: &Running, gone: &[UnitId]) -> bool {
    process
        .var(crate::env::vars::ID)
        .and_then(|id| UnitId::parse(id).ok())
        .is_some_and(|id| gone.contains(&id))
}

/// The containers among these that are labelled for a unit that is gone.
fn labelled(containers: Vec<docker::Container>, gone: &[UnitId]) -> Vec<String> {
    containers
        .into_iter()
        .filter(|container| {
            container
                .label(UNIT_LABEL)
                .and_then(|id| UnitId::parse(id).ok())
                .is_some_and(|id| gone.contains(&id))
        })
        .map(|container| container.name)
        .collect()
}

/// Give back every lease whose claim has run out.
fn release_lapsed(
    conn: &Connection,
    now: Timestamp,
    leftovers: &mut Vec<Leftover>,
) -> Result<Vec<String>> {
    let mut released = Vec::new();
    for lease in leases::list_expired(conn, now)? {
        if leases::release(conn, &lease.resource, lease.environment_id)? {
            released.push(lease.resource.to_string());
        } else {
            leftovers.push(Leftover::new("lease", lease.resource.to_string()));
        }
    }
    Ok(released)
}

/// Remove a directory and everything under it. One that is not there is already gone.
fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// What a directory occupies, as the sum of the sizes of the files under it.
///
/// An approximation, and named as one: it counts a file once whatever the filesystem
/// did about sharing its blocks, so a home cloned copy-on-write reads as the size of
/// its content rather than the space it actually took. Reporting the larger of the two
/// is the safe direction for a number a person reads after a deletion. Anything that
/// cannot be read contributes nothing rather than failing the sweep.
fn size_of(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            total += size_of(&entry.path());
        } else if kind.is_file() {
            total += entry.metadata().map_or(0, |data| data.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{remove, size_of};

    #[test]
    fn a_directory_reads_as_the_sum_of_what_is_under_it() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("a"), [0_u8; 100]).unwrap();
        std::fs::create_dir(root.path().join("d")).unwrap();
        std::fs::write(root.path().join("d").join("b"), [0_u8; 23]).unwrap();
        assert_eq!(size_of(root.path()), 123);
    }

    #[test]
    fn what_is_not_there_reads_as_nothing_and_removes_without_complaint() {
        let root = tempfile::TempDir::new().unwrap();
        let missing = root.path().join("never");
        assert_eq!(size_of(&missing), 0);
        remove(&missing).unwrap();
    }
}
