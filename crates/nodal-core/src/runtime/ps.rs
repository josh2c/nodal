//! `nodal ps`: every signal read once, merged into one answer.
//!
//! This is the composition and nothing else. The registry is read here, once, into a
//! [`Scope`]; the process table is read here, once, and handed to the two signals that
//! need it; each signal is asked what it sees; and the rows are merged.
//!
//! Merging is one rule. Two signals can see one thing — a container that carries a label
//! and mounts a home, a process seen twice — and the better answer wins: certain over
//! probable. Nothing else is dropped. A row per process, a row per container and a row
//! per bound port is what a person asked for when they asked what is running.
//!
//! The answer is ordered by unit, then by kind, then by name, so two runs a second apart
//! print the same rows in the same order and a person can read the difference.
//!
//! Nothing here writes. `nodal ps` is a read command: it opens no operation, changes no
//! row, and a machine whose signals all fail still answers, with notes instead of rows.

use rusqlite::Connection;

use crate::Result;
use crate::model::{EnvState, HostName, Timestamp};
use crate::output::view::Ps;
use crate::runtime::attribute::{
    Attributed, Attributor, Home, Note, Reading, Scope, Source, cwd, docker, listeners, process_env,
};
use crate::runtime::processes::{Processes, Running};
use crate::services::docker::Docker;
use crate::store::{environments, units};

/// The homes on this host, as the registry has them.
///
/// An environment that has been reclaimed ([`EnvState::Absent`]) is left out: there is
/// no directory to stand in and no service to belong to it. A row whose unit is gone is
/// left out too, which no sequence of operations produces and a partially restored
/// registry can.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement.
pub fn scope(conn: &Connection, host: &HostName) -> Result<Scope> {
    let mut homes = Vec::new();
    for environment in environments::list_all(conn)? {
        if &environment.host != host || environment.state == EnvState::Absent {
            continue;
        }
        let Some(unit) = units::get(conn, environment.unit_id)? else { continue };
        homes.push(Home {
            unit: unit.id,
            slug: unit.slug,
            environment: environment.id,
            root: environment.home,
            ports: environment.ports,
        });
    }
    Ok(Scope::new(homes))
}

/// Read every signal and merge what they saw.
///
/// # Errors
/// [`crate::Error::Store`] on a failed statement while the scope is read. A signal that
/// cannot run is a note, not an error.
pub fn observe(
    conn: &Connection,
    processes: &dyn Processes,
    docker: &dyn Docker,
    host: &HostName,
    now: Timestamp,
) -> Result<Ps> {
    let scope = scope(conn, host)?;
    let (running, mut notes) = table(processes);
    let signals: [&dyn Attributor; 4] = [
        &process_env::FromEnvironment::new(&running),
        &cwd::FromDirectory::new(&running),
        &docker::FromContainers::new(docker),
        &listeners::FromPorts,
    ];
    let mut rows = Vec::new();
    for signal in signals {
        let Reading { rows: seen, note } = signal.read(&scope);
        rows.extend(seen);
        notes.extend(note);
    }
    Ok(Ps { now, host: host.clone(), rows: merge(rows), notes })
}

/// The process table, or an empty one and a note for each signal that needed it.
///
/// The two process signals are told apart in the notes rather than sharing one, because
/// a person reading `nodal ps` on a host with no `/proc` needs to see that both the
/// certain answer and the probable one are missing, not that "something" is.
fn table(processes: &dyn Processes) -> (Vec<Running>, Vec<Note>) {
    match processes.scan() {
        Ok(running) => (running, Vec::new()),
        Err(error) => {
            let why = error.to_string();
            (
                Vec::new(),
                vec![Note::new(Source::Environment, why.clone()), Note::new(Source::Cwd, why)],
            )
        }
    }
}

/// One row per thing, ordered, with the better answer kept where two signals saw one
/// thing.
#[must_use]
pub fn merge(mut rows: Vec<Attributed>) -> Vec<Attributed> {
    rows.sort_by(|left, right| {
        (&left.slug, left.kind, &left.what, left.pid, left.port, left.confidence).cmp(&(
            &right.slug,
            right.kind,
            &right.what,
            right.pid,
            right.port,
            right.confidence,
        ))
    });
    rows.dedup_by(|later, kept| {
        // The sort put the better confidence first, so the one already kept wins.
        (kept.unit, kept.kind, kept.pid, kept.port, &kept.what)
            == (later.unit, later.kind, later.pid, later.port, &later.what)
    });
    rows
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{merge, table};
    use crate::runtime::attribute::fixture::{UNIT, home};
    use crate::runtime::attribute::{Attributed, Confidence, Kind, Source};
    use crate::runtime::processes::{Processes, Running};
    use crate::{Error, Result};

    /// A machine whose process table cannot be read.
    struct NoTable;

    impl Processes for NoTable {
        fn scan(&self) -> Result<Vec<Running>> {
            Err(Error::ProcessScanUnsupported { host: "macos" })
        }
    }

    fn row(pid: u32, confidence: Confidence, signal: Source) -> Attributed {
        let home = home(UNIT, "worker-import", "/homes/worker-import", 41_230);
        Attributed {
            unit: home.unit,
            slug: home.slug,
            environment: home.environment,
            kind: Kind::Process,
            what: String::from("node dev"),
            pid: Some(pid),
            port: None,
            confidence,
            signal,
        }
    }

    #[test]
    fn one_thing_seen_twice_keeps_the_better_answer() {
        let rows = merge(vec![
            row(11, Confidence::Probable, Source::Cwd),
            row(11, Confidence::Certain, Source::Environment),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].confidence, Confidence::Certain);
        assert_eq!(rows[0].signal, Source::Environment);
    }

    #[test]
    fn two_processes_are_two_rows() {
        let rows = merge(vec![
            row(12, Confidence::Certain, Source::Environment),
            row(11, Confidence::Certain, Source::Environment),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].pid, Some(11), "rows are ordered so two runs read alike");
    }

    #[test]
    fn a_host_with_no_process_table_says_so_for_both_signals_that_need_one() {
        let (running, notes) = table(&NoTable);
        assert!(running.is_empty());
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].signal, Source::Environment);
        assert_eq!(notes[1].signal, Source::Cwd);
        assert!(notes[0].why.contains("/proc"), "{}", notes[0].why);
    }
}
