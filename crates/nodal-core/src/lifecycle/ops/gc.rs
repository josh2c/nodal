//! `nodal gc`: remove what reclaim only moved aside, once its retention has run out.
//!
//! Reclaiming a unit does not delete anything ([`super::reclaim`]). It moves the home
//! to the project's trash directory and writes down the first instant it may go. This
//! is the operation that acts on that instant, and it is the only thing in Nodal that
//! deletes a directory a person worked in.
//!
//! Three things happen, in this order, and the order is the point.
//!
//! **Idle runtime is stopped first.** A tether whose unit was reclaimed, a process still
//! standing in a home that was reclaimed last week, a container still labelled for a
//! unit that no longer exists — these are what "idle" means here, and they are stopped
//! before the directory under them is removed rather than after, so that nothing is
//! deleting a tree a running process is reading. Runtime belonging to a unit that is
//! *still live* is never touched: a person's development server is not garbage, whatever
//! the clock says about it, and never removing what was not asked for is worth more than
//! the disk.
//!
//! The tethers come first, for the reason they come first in a reclaim
//! ([`super::reclaim`]): the registry recorded each group when `nodal run --tether`
//! started it, so a group is a record rather than an inference, and one signal reaches
//! every process in it. A group is read from the *environments* that have been
//! reclaimed, not from the units, because an environment in [`EnvState::Absent`] is one
//! whose home has been taken away and nothing of it should still be running.
//!
//! A tether's row is closed once its group is empty. That is what keeps the sweep from
//! signalling a group identifier the system has since given to something else: a row is
//! acted on only while it is open, and it stays open only while the group is there.
//!
//! **A merged unit's home is given back before any of that.** A unit the list found
//! merged keeps its home, because the day after a merge is exactly when somebody wants
//! to look at what they did. It keeps it for the retention the project asked for
//! (`reclaim.trash_retention`), measured from the moment the merge was recorded, and
//! then this sweep reclaims it — by the ordinary path, so the uniqueness check applies
//! in full. A merged unit somebody has since put new work in is refused and named in
//! the report rather than removed, which is the whole reason the reclaim is reused
//! instead of the directory being taken directly.
//!
//! Reclaiming is not removing. The home goes to the trash with a retention of its own,
//! and a later sweep is what finally takes it.
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
//! **Idle live units are reported, on request, and never touched.** `nodal gc --idle`
//! adds one section to the report: the units nobody has been in for longer than the
//! threshold, read from the session rows ([`crate::lifecycle::idle`]). It stops nothing
//! of theirs and asks nothing about them. A person's development server is not garbage
//! whatever the clock says, so the answer to "this has been quiet for a month" is a
//! line in a report and a person's own decision.
//!
//! The two signals it reads to find that idle runtime — the process table and the
//! container daemon — are not available on every host. A reading that cannot be made is
//! a [`Note`] in the answer and never a failure, which is the contract every attribution
//! signal already has, and it keeps "nothing was running" apart from "I could not look".

use std::path::Path;

use rusqlite::Connection;

use crate::lifecycle::idle;
use crate::lifecycle::uniqueness::Finding;
use crate::model::{
    EnvState, Project, SessionId, Timestamp, Trashed, Unit, UnitId, UnitStatus, trash as retention,
};
use crate::output::view::{Idle, Leftover, Retired, Swept};
use crate::runtime::attribute::{Note, Source};
use crate::runtime::processes::{Processes, Running};
use crate::runtime::stop::{self, Signals as _, Stopped, Target};
use crate::services::docker;
use crate::store::{Store, environments, leases, projects, sessions, trash, units};

use super::reclaim;
use crate::{Error, Result};

/// The label a unit's containers carry.
const UNIT_LABEL: &str = "nodal.unit";

/// What one sweep was asked to do beyond its ordinary work.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Report the live units nothing has touched for this many days. `None` asks for no
    /// such report, which is not the same as a report that found none.
    pub idle: Option<u32>,
    /// Whether the project's own reclaim hooks run for a merged unit whose home is
    /// given back. `false` is `--no-hooks`.
    pub hooks: bool,
}

/// Reclaim the merged units whose retention has run out, remove every expired home,
/// stop the runtime of units that are gone, and give back lapsed leases.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read or written. A directory that
/// will not go, and a merged unit the uniqueness check refuses, are [`Leftover`]s in
/// the answer rather than errors: one home nobody can remove must not stop the rest of
/// the sweep.
pub fn collect(store: &mut Store, now: Timestamp, options: &Options) -> Result<Swept> {
    let mut leftovers = Vec::new();
    let retired = retire(store, now, options.hooks, &mut leftovers)?;
    let expired = trash::list_expired(store.conn(), now)?;
    let stopped = stop_absent(store.conn())?;
    let (removed, freed, mut swept_leftovers) = sweep(store, &expired)?;
    leftovers.append(&mut swept_leftovers);
    let released = release_lapsed(store.conn(), now, &mut leftovers)?;
    Ok(Swept {
        now,
        removed,
        kept: kept(store.conn(), now)?,
        freed_bytes: Some(freed),
        stopped: stopped.stopped,
        containers: stopped.containers,
        leases: released,
        retired,
        idle: match options.idle {
            Some(days) => quiet(store.conn(), now, days)?,
            None => Vec::new(),
        },
        idle_asked: options.idle.is_some(),
        notes: stopped.notes,
        leftovers,
    })
}

// ---------------------------------------------------------------------------
// The merged units whose retention has run out.
// ---------------------------------------------------------------------------

/// Give back the home of every merged unit that has kept one long enough.
///
/// Through [`super::reclaim`], not around it. That is the point of this whole path: the
/// uniqueness check, the teardown, the hooks and the trash entry are the ones a person
/// gets when they type `nodal reclaim`, so a merged unit somebody has since put work in
/// is refused here exactly as it would be there.
///
/// A refusal is a line of the report. So is any other failure of one unit, because a
/// sweep that stopped at the first unit it could not reclaim would leave the rest of the
/// machine untouched for a reason that has nothing to do with them. A registry that
/// cannot be read is not one of those and is raised.
fn retire(
    store: &mut Store,
    now: Timestamp,
    hooks: bool,
    leftovers: &mut Vec<Leftover>,
) -> Result<Vec<Retired>> {
    let mut retired = Vec::new();
    for (project, unit) in due(store.conn(), now)? {
        let request = reclaim::Request {
            target: Some(unit.slug.to_string()),
            force: false,
            hooks,
            cwd: project.root.clone(),
        };
        match reclaim::reclaim(store, &request) {
            Ok(report) => retired.push(Retired {
                slug: unit.slug.clone(),
                trashed: report.trashed.map(|entry| entry.path),
            }),
            Err(Error::Store { path, source }) => return Err(Error::Store { path, source }),
            Err(refused) => leftovers.push(refusal(&unit, &refused)),
        }
    }
    Ok(retired)
}

/// The merged units whose home has been kept for as long as the project asked.
///
/// The clock runs from the unit's own `updated_at`, which for a merged unit is the
/// instant the merge was recorded ([`crate::lifecycle::states`]). A unit whose home has
/// already gone is not one of these: there is nothing left to give back.
fn due(conn: &Connection, now: Timestamp) -> Result<Vec<(Project, Unit)>> {
    let mut found = Vec::new();
    for project in projects::list(conn)? {
        let days = super::reclaim::recipe_of(&project.root).trash_retention_days();
        for unit in units::list_by_status(conn, project.id, UnitStatus::Merged)? {
            let expires = retention::expiry(unit.updated_at, days);
            if expires.unix_seconds() <= now.unix_seconds() && has_home(conn, &unit)? {
                found.push((project.clone(), unit));
            }
        }
    }
    Ok(found)
}

/// Whether the unit still has a materialisation a reclaim would act on.
fn has_home(conn: &Connection, unit: &Unit) -> Result<bool> {
    Ok(environments::latest_for_unit(conn, unit.id)?
        .is_some_and(|environment| environment.state != EnvState::Absent))
}

/// The line a refused unit gets, naming what was found rather than a policy.
fn refusal(unit: &Unit, why: &Error) -> Leftover {
    let detail = match why {
        Error::NotUnique { findings, .. } => {
            format!("{}: {}", unit.slug, Finding::summarise(findings))
        }
        other => format!("{}: {other}", unit.slug),
    };
    Leftover::new("unit", detail)
}

// ---------------------------------------------------------------------------
// The live units that have gone quiet.
// ---------------------------------------------------------------------------

/// The live units nothing has touched for `days`, longest quiet first.
///
/// Reported and nothing else. Every environment here is one a person can still open,
/// and this function reads timestamps ([`idle`]) rather than the machine: no process is
/// signalled, no container is asked about, and nothing is written.
fn quiet(conn: &Connection, now: Timestamp, days: u32) -> Result<Vec<Idle>> {
    let mut found = Vec::new();
    for environment in environments::list_all(conn)? {
        if environment.state == EnvState::Absent {
            continue;
        }
        let Some(unit) = units::get(conn, environment.unit_id)? else { continue };
        if !matches!(unit.status, UnitStatus::Open | UnitStatus::Review) {
            continue;
        }
        let sessions = sessions::list_for_environment(conn, environment.id)?;
        if idle::attached(&sessions) {
            continue;
        }
        let since = idle::last_seen(&sessions, environment.last_active, now);
        if idle::is_idle(since, now, days) {
            found.push(Idle { slug: unit.slug, home: environment.home, since });
        }
    }
    found.sort_by_key(|unit| (unit.since.unix_seconds(), unit.slug.to_string()));
    Ok(found)
}

/// What the sweep did about runtime that outlived its unit.
///
/// Named for the units it is about — the ones whose homes are gone — rather than for
/// the word "idle", which in this command means the live units that are only reported.
#[derive(Debug, Default)]
struct Outlived {
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
fn stop_absent(conn: &Connection) -> Result<Outlived> {
    let tethers = absent_tethers(conn)?;
    let gone = absent_units(conn)?;
    if gone.is_empty() && tethers.is_empty() {
        return Ok(Outlived::default());
    }
    let mut outlived = Outlived::default();
    let (pids, containers) = seen(&gone, &mut outlived.notes);
    let mut targets: Vec<Target> = tethers.iter().map(|(_, pgid)| Target::Group(*pgid)).collect();
    targets.extend(pids.into_iter().map(Target::Process));
    outlived.stopped = stop::processes(&stop::Live, &targets, stop::GRACE);
    close_empty(conn, &tethers)?;
    match docker::remove(&docker::Cli, &containers) {
        Ok(removed) => {
            outlived.notes.extend(removed.why.map(|why| Note::new(Source::Docker, why)));
            outlived.containers = removed.containers;
        }
        Err(error) => outlived.notes.push(Note::new(Source::Docker, error.to_string())),
    }
    Ok(outlived)
}

/// The tethers of every environment that has been reclaimed, as row and group.
///
/// Only open rows, and only reclaimed environments. A live unit's tether is its
/// person's development server, and this sweep never touches one.
fn absent_tethers(conn: &Connection) -> Result<Vec<(SessionId, u32)>> {
    let mut found = Vec::new();
    for environment in environments::list_by_state(conn, EnvState::Absent)? {
        for session in sessions::list_open_tethers(conn, environment.id)? {
            if let Some(pgid) = session.pgid {
                found.push((session.id, pgid));
            }
        }
    }
    Ok(found)
}

/// Close the row of every tether whose group is now empty, and leave the rest open.
///
/// A group that would not stop keeps its row, so the next sweep tries again. A group
/// that has gone gives its row up, so no later sweep signals an identifier the system
/// has handed to something else.
fn close_empty(conn: &Connection, tethers: &[(SessionId, u32)]) -> Result<()> {
    let now = Timestamp::now();
    for (session, pgid) in tethers {
        if stop::Live.alive(Target::Group(*pgid)) {
            continue;
        }
        sessions::end(conn, *session, now)?;
    }
    Ok(())
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
