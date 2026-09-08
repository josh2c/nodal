//! Upgrading a registry a v1-era Nodal wrote.
//!
//! Version drift has two directions and Nodal answers them differently. A database from
//! a *later* Nodal is refused, because this binary does not know what is in it
//! (`Error::StoreTooNew`, asserted in `store.rs`). A database from an *earlier* Nodal is
//! migrated forward, and this file is what says the forward direction keeps the rows.
//!
//! The fixture is a real v1 file, not a description of one. It is made by running the
//! first migration exactly as it is committed, writing rows through SQL rather than
//! through today's writers, and stamping `user_version` at 1. A writer that changed
//! shape since v1 therefore cannot make this test pass by accident: the bytes in the
//! file are the bytes v1 wrote.
//!
//! Three claims:
//!
//! 1. opening the file migrates it to the current schema version;
//! 2. every row that was in it is readable afterwards, through the readers of today;
//! 3. a value a later migration added is filled in by that migration rather than left
//!    for a reader to guess. `objective_epistemic` arrived in migration 6, and a v1 unit
//!    that carries an objective is `stated` afterwards, because an objective a person
//!    typed is the only kind v1 could hold.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::model::{ActorKind, EnvState, Epistemic, EventKind, Timestamp, UnitStatus};
use nodal_core::store::migrations::MIGRATIONS;
use nodal_core::store::{SCHEMA_VERSION, Store, environments, events, projects, sessions, units};
use tempfile::TempDir;

/// The identifiers the fixture uses, so that a failure names the row it is about.
const PROJECT: &str = "01J8Z6H0000000000000000001";
/// The unit of that project.
const UNIT: &str = "01J8Z6H0000000000000000002";
/// The unit's one materialisation.
const ENVIRONMENT: &str = "01J8Z6H0000000000000000005";
/// One session in that home.
const SESSION: &str = "01J8Z6H0000000000000000006";
/// One event of that unit.
const EVENT: &str = "01J8Z6H0000000000000000007";

/// The rows, as a v1 Nodal wrote them: one project, one unit, one home, one session and
/// one event.
///
/// The column lists are the v1 column lists. `session.pgid` is not here because
/// migration 5 added it, and `unit.objective_epistemic` is not here because migration 6
/// did.
const V1_ROWS: &str = "
INSERT INTO project (id, root, name, recipe_hash, created_at)
VALUES ('01J8Z6H0000000000000000001', '/home/dev/acme', 'acme', '0f1e2d', 1788688800);

INSERT INTO unit (id, project_id, slug, objective, branch, parent_branch, status,
                  created_at, updated_at)
VALUES ('01J8Z6H0000000000000000002', '01J8Z6H0000000000000000001', 'fix-worker-import',
        'fix the worker import', 'nodal/fix-worker-import', 'main', 'open',
        1788688860, 1788688860);

INSERT INTO environment (id, unit_id, attempt, home, managed, base_id,
                         ws_fp_materialized, schema_fp_materialized, host, db_name,
                         ports, fixed_port, state, created_at, last_active)
VALUES ('01J8Z6H0000000000000000005', '01J8Z6H0000000000000000002', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H0', 1, NULL, NULL, NULL, 'laptop', NULL,
        '{\"app\":4001}', 5432, 'stopped', 1788689040, 1788689040);

INSERT INTO session (id, environment_id, actor_kind, actor_name, pid, started_at, ended_at)
VALUES ('01J8Z6H0000000000000000006', '01J8Z6H0000000000000000005', 'agent',
        'claude-code', 4242, 1788689100, NULL);

INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind,
                   epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000007', '01J8Z6H0000000000000000002',
        '01J8Z6H0000000000000000005', 1788689160, 'human', 'dev', 'commit', 'observed',
        'wired the worker import', '{}', NULL);
";

/// Write a registry file exactly as a v1 Nodal left one.
fn v1_registry(directory: &Path) -> PathBuf {
    let path = directory.join("registry.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let first = MIGRATIONS.first().expect("there is a first migration");
    assert_eq!(first.version, 1, "the fixture is built from migration 1");
    conn.execute_batch(first.sql).unwrap();
    conn.execute_batch(V1_ROWS).unwrap();
    conn.pragma_update(None, "user_version", 1_u32).unwrap();
    conn.close().unwrap();
    assert_eq!(version_of(&path), 1, "the fixture is a version 1 file");
    path
}

/// The schema version a file carries, read without opening it as a store.
fn version_of(path: &Path) -> u32 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap()
}

#[test]
fn a_v1_registry_is_migrated_to_the_current_schema() {
    let directory = TempDir::new().unwrap();
    let path = v1_registry(directory.path());

    Store::open(&path).unwrap();

    assert_eq!(version_of(&path), SCHEMA_VERSION, "the file is at the current version");
}

#[test]
fn the_project_and_the_unit_a_v1_registry_held_are_readable_afterwards() {
    let directory = TempDir::new().unwrap();
    let store = Store::open(v1_registry(directory.path())).unwrap();
    let conn = store.conn();

    let project = projects::get(conn, PROJECT.parse().unwrap()).unwrap().expect("the project");
    assert_eq!(project.root, PathBuf::from("/home/dev/acme"));
    assert_eq!(project.name.as_str(), "acme");
    assert_eq!(project.created_at, Timestamp::parse("2026-09-06T10:00:00Z").unwrap());

    let unit = units::get(conn, UNIT.parse().unwrap()).unwrap().expect("the unit");
    assert_eq!(unit.slug.as_str(), "fix-worker-import");
    assert_eq!(unit.branch.as_str(), "nodal/fix-worker-import");
    assert_eq!(unit.status, UnitStatus::Open);
    assert_eq!(
        unit.objective.as_ref().map(nodal_core::model::Objective::as_str),
        Some("fix the worker import")
    );
}

#[test]
fn the_home_a_v1_registry_held_is_readable_afterwards() {
    let directory = TempDir::new().unwrap();
    let store = Store::open(v1_registry(directory.path())).unwrap();

    let home = environments::get(store.conn(), ENVIRONMENT.parse().unwrap())
        .unwrap()
        .expect("the environment");

    assert_eq!(home.home, PathBuf::from("/home/dev/.nodal/acme/e/01J8Z6H0"));
    assert!(home.managed);
    assert_eq!(home.state, EnvState::Stopped);
    assert_eq!(home.fixed_port, Some(5432));
    assert_eq!(home.ports.0.values().copied().collect::<Vec<u16>>(), vec![4001]);
}

#[test]
fn the_session_and_the_event_a_v1_registry_held_are_readable_afterwards() {
    let directory = TempDir::new().unwrap();
    let store = Store::open(v1_registry(directory.path())).unwrap();
    let conn = store.conn();

    let open = sessions::list_open(conn, ENVIRONMENT.parse().unwrap()).unwrap();
    assert_eq!(open.len(), 1, "the session is still open");
    assert_eq!(open[0].id.to_string(), SESSION);
    assert_eq!(open[0].actor.kind, ActorKind::Agent);
    assert_eq!(open[0].pid, Some(4242));
    assert_eq!(open[0].pgid, None, "a column migration 5 added is empty, not wrong");

    let log = events::list_for_unit(conn, UNIT.parse().unwrap()).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].id.to_string(), EVENT);
    assert_eq!(log[0].kind, EventKind::Commit);
    assert_eq!(log[0].body, "wired the worker import");
}

#[test]
fn a_value_a_later_migration_added_is_filled_in_by_that_migration() {
    let directory = TempDir::new().unwrap();
    let store = Store::open(v1_registry(directory.path())).unwrap();

    let unit = units::get(store.conn(), UNIT.parse().unwrap()).unwrap().expect("the unit");

    assert_eq!(
        unit.objective_epistemic,
        Some(Epistemic::Stated),
        "an objective a v1 Nodal held is one a person typed"
    );
}

#[test]
fn migrating_a_registry_twice_is_migrating_it_once() {
    let directory = TempDir::new().unwrap();
    let path = v1_registry(directory.path());

    Store::open(&path).unwrap();
    let after_first =
        events::list_for_unit(Store::open(&path).unwrap().conn(), UNIT.parse().unwrap()).unwrap();

    assert_eq!(after_first.len(), 1, "a second open did not repeat a migration");
    assert_eq!(version_of(&path), SCHEMA_VERSION);
}
