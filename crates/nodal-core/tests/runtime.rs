//! Acceptance for sessions, which are derived rather than declared (T1.9).
//!
//! Three claims:
//!
//! 1. A process that carries `NODAL_ID` is in that unit, and one that does not is not.
//! 2. A real process started with a home's environment opens a session row, and the row
//!    is ended once that process is gone. Nothing reports either event.
//! 3. A session of an environment on another host is left alone by this machine.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nodal_core::model::{
    Actor, ActorKind, ActorName, EnvId, HostName, ProjectId, Session, SessionId, Timestamp, UnitId,
};
use nodal_core::runtime::processes::{Processes, Running};
use nodal_core::runtime::sessions::{self, Attached};
use nodal_core::store::{Store, environments, projects, sessions as session_rows, units};
use nodal_safety::rows;

/// The unit every row in this file belongs to.
const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The environment it is materialised as.
const ENVIRONMENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

/// A process table a test supplies instead of a machine.
struct Table(Vec<Running>);

impl Processes for Table {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Ok(self.0.clone())
    }
}

fn running(pid: u32, pairs: &[(&str, &str)]) -> Running {
    let vars: BTreeMap<String, String> =
        pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
    Running::new(pid, vars)
}

fn host() -> HostName {
    HostName::parse("workstation").unwrap()
}

/// A registry that holds one unit materialised at `home`, on `host`.
fn registry(root: &Path, home: &Path, host: &HostName) -> Store {
    let now = Timestamp::now();
    let project_id = ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap();
    let unit = rows::unit(
        UnitId::parse(UNIT).unwrap(),
        project_id,
        "fix-worker-import",
        "nodal/fix-worker-import",
        now,
    );
    let mut environment = rows::environment(EnvId::parse(ENVIRONMENT).unwrap(), unit.id, home, now);
    environment.host = host.clone();
    let project = rows::project(unit.project_id, root.join("checkout"), "fixture", now);
    let store = Store::open(root.join("registry.db")).unwrap();
    projects::insert(store.conn(), &project).unwrap();
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(store.conn(), &environment).unwrap();
    store
}

#[test]
fn a_process_carrying_the_unit_is_in_it_and_one_that_does_not_is_not() {
    let table = [
        running(11, &[("NODAL_ID", UNIT), ("NODAL_ROOT", "/homes/one"), ("USER", "josh")]),
        running(12, &[("CLAUDECODE", "1"), ("NODAL_ID", UNIT), ("NODAL_ROOT", "/homes/one")]),
        running(13, &[("USER", "josh")]),
        running(14, &[("NODAL_ID", "not-an-identifier")]),
    ];
    let attached = sessions::derive(&table).unwrap();

    assert_eq!(attached.len(), 2, "{attached:?}");
    assert_eq!(attached[0].pid, 11);
    assert_eq!(attached[0].actor.kind, ActorKind::Human);
    assert_eq!(attached[0].root, PathBuf::from("/homes/one"));
    assert_eq!(attached[1].actor.kind, ActorKind::Agent);
    assert_eq!(attached[1].actor.name.as_str(), "claude-code");
}

#[test]
fn a_session_opens_because_a_process_is_there_and_ends_because_it_is_gone() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let host = host();
    let store = registry(directory.path(), &home, &host);
    let environment = EnvId::parse(ENVIRONMENT).unwrap();

    let table = Table(vec![running(
        4242,
        &[("NODAL_ID", UNIT), ("NODAL_ROOT", home.to_str().unwrap()), ("USER", "josh")],
    )]);
    let opened = sessions::observe(store.conn(), &table, &host, Timestamp::now()).unwrap();
    assert_eq!(opened.opened, 1);
    let open = session_rows::list_open(store.conn(), environment).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].pid, Some(4242));

    // Seeing the same process again changes nothing: the row is already open.
    let again = sessions::observe(store.conn(), &table, &host, Timestamp::now()).unwrap();
    assert_eq!((again.opened, again.ended), (0, 0));

    // The process is gone, so the row is closed. Nothing said so.
    let ended =
        sessions::observe(store.conn(), &Table(Vec::new()), &host, Timestamp::now()).unwrap();
    assert_eq!(ended.ended, 1);
    assert!(session_rows::list_open(store.conn(), environment).unwrap().is_empty());
}

#[test]
fn a_session_on_another_host_is_not_this_machines_to_close() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let elsewhere = HostName::parse("laptop").unwrap();
    let store = registry(directory.path(), &home, &elsewhere);
    let environment = EnvId::parse(ENVIRONMENT).unwrap();
    session_rows::insert(
        store.conn(),
        &Session {
            id: SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap(),
            environment_id: environment,
            actor: Actor { kind: ActorKind::Human, name: ActorName::parse("josh").unwrap() },
            pid: Some(99),
            pgid: None,
            started_at: Timestamp::now(),
            ended_at: None,
        },
    )
    .unwrap();

    let change = sessions::reconcile(store.conn(), &host(), &[], Timestamp::now()).unwrap();
    assert_eq!(change.ended, 0);
    assert_eq!(session_rows::list_open(store.conn(), environment).unwrap().len(), 1);
}

#[test]
fn a_process_started_with_a_homes_environment_is_seen_on_this_machine() {
    if !cfg!(target_os = "linux") {
        eprintln!("skipped: a process scan reads /proc, which this host does not have");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let host = nodal_core::lifecycle::owner::current_host();
    let store = registry(directory.path(), &home, &host);
    let environment = EnvId::parse(ENVIRONMENT).unwrap();

    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .env("NODAL_ID", UNIT)
        .env("NODAL_ROOT", &home)
        .spawn()
        .unwrap();

    let live = nodal_core::runtime::processes::Live;
    let opened = sessions::observe(store.conn(), &live, &host, Timestamp::now()).unwrap();
    assert_eq!(opened.opened, 1, "the process table did not show the process");
    let open = session_rows::list_open(store.conn(), environment).unwrap();
    assert_eq!(open[0].pid, Some(child.id()));

    child.kill().unwrap();
    child.wait().unwrap();
    let ended = sessions::observe(store.conn(), &live, &host, Timestamp::now()).unwrap();
    assert_eq!(ended.ended, 1);
}

/// A dropped attachment is one nothing resolves to, which reconciling must not open.
#[test]
fn a_process_in_a_home_the_registry_does_not_know_opens_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let host = host();
    let store = registry(directory.path(), &home, &host);
    let attached = [Attached {
        pid: 7,
        unit: UnitId::parse(UNIT).unwrap(),
        root: PathBuf::from("/somewhere/else"),
        actor: Actor { kind: ActorKind::Human, name: ActorName::parse("josh").unwrap() },
    }];

    let change = sessions::reconcile(store.conn(), &host, &attached, Timestamp::now()).unwrap();
    assert_eq!((change.opened, change.ended), (0, 0));
}
