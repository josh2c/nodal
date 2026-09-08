//! Integration tests for the registry: every repository function against a real SQLite
//! file, plus the rules the schema itself enforces.
//!
//! Each test opens its own database under a `tempfile` directory, so nothing here can
//! see or disturb another test's rows.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use nodal_core::error::Error;
use nodal_core::model::{
    Actor, ActorKind, ActorName, Base, BranchName, CommitId, DbName, DbTemplate, Digest, EnvState,
    Environment, Epistemic, Event, EventId, EventKind, HostName, Lease, Lock, Objective, Platform,
    PortAllocation, PortBlock, PortName, Ports, Project, ProjectName, RawRef, RefName, ResourceKey,
    SchemaFp, Session, Slug, Timestamp, Unit, UnitId, UnitStatus, WorkspaceFp,
};
use nodal_core::store::{
    SCHEMA_VERSION, Store, bases, environments, events, leases, locks, port_allocations,
    port_blocks, projects, sessions, templates, units,
};
use tempfile::TempDir;

/// A registry in a directory that is removed when the test ends.
struct Registry {
    dir: TempDir,
    store: Store,
}

impl Registry {
    /// An empty, migrated registry.
    fn open() -> Self {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path().join("registry.db")).unwrap();
        Self { dir, store }
    }

    /// A registry holding one project and one open unit.
    fn seeded() -> Self {
        let registry = Self::open();
        projects::insert(registry.store.conn(), &project()).unwrap();
        units::insert(registry.store.conn(), &unit()).unwrap();
        registry
    }

    /// Where the database file lives.
    fn path(&self) -> PathBuf {
        self.dir.path().join("registry.db")
    }
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn id<T: std::str::FromStr<Err = Error>>(last: char) -> T {
    let text: String = format!("01J8Z6H000000000000000000{last}");
    text.parse().unwrap()
}

fn project() -> Project {
    Project {
        id: id('1'),
        root: PathBuf::from("/home/dev/acme"),
        name: ProjectName::parse("acme").unwrap(),
        recipe_hash: Digest::parse("0f1e2d").unwrap(),
        created_at: at("2026-09-06T10:00:00Z"),
    }
}

fn unit() -> Unit {
    Unit {
        id: id('2'),
        project_id: id('1'),
        slug: Slug::parse("fix-worker-import").unwrap(),
        objective: Some(Objective::parse("fix the worker import").unwrap()),
        branch: BranchName::parse("nodal/fix-worker-import").unwrap(),
        parent_branch: Some(BranchName::parse("main").unwrap()),
        status: UnitStatus::Open,
        created_at: at("2026-09-06T10:01:00Z"),
        updated_at: at("2026-09-06T10:01:00Z"),
    }
}

fn base() -> Base {
    Base {
        id: id('3'),
        project_id: id('1'),
        ws_fingerprint: WorkspaceFp(Digest::parse("aabbcc").unwrap()),
        platform: Platform::parse("x86_64-unknown-linux-gnu").unwrap(),
        commit: CommitId::parse("a".repeat(40)).unwrap(),
        path: PathBuf::from("/home/dev/.nodal/acme/base/aabbcc"),
        built_at: at("2026-09-06T10:02:00Z"),
        last_used: at("2026-09-06T10:02:00Z"),
    }
}

fn template() -> DbTemplate {
    DbTemplate {
        id: id('4'),
        project_id: id('1'),
        schema_fingerprint: SchemaFp(Digest::parse("ddeeff").unwrap()),
        db_name: DbName::parse("acme_tpl_ddeeff").unwrap(),
        parent_template_id: None,
        built_at: at("2026-09-06T10:03:00Z"),
    }
}

fn environment() -> Environment {
    let mut ports = BTreeMap::new();
    ports.insert(PortName::parse("app").unwrap(), 4001_u16);
    Environment {
        id: id('5'),
        unit_id: id('2'),
        attempt: 1,
        home: PathBuf::from("/home/dev/.nodal/acme/e/01J8Z6H0"),
        managed: true,
        base_id: Some(id('3')),
        ws_fp_materialized: Some(WorkspaceFp(Digest::parse("aabbcc").unwrap())),
        schema_fp_materialized: None,
        host: HostName::parse("laptop").unwrap(),
        db_name: Some(DbName::parse("acme_01j8z6h0").unwrap()),
        ports: Ports(ports),
        fixed_port: Some(5432),
        state: EnvState::Stopped,
        created_at: at("2026-09-06T10:04:00Z"),
        last_active: at("2026-09-06T10:04:00Z"),
    }
}

fn session() -> Session {
    Session {
        id: id('6'),
        environment_id: id('5'),
        actor: Actor { kind: ActorKind::Agent, name: ActorName::parse("claude-code").unwrap() },
        pid: Some(4242),
        pgid: None,
        started_at: at("2026-09-06T10:05:00Z"),
        ended_at: None,
    }
}

fn event() -> Event {
    let mut refs = BTreeMap::new();
    refs.insert(RefName::parse("commit").unwrap(), "a".repeat(40));
    Event {
        id: id('7'),
        unit: id('2'),
        environment: Some(id('5')),
        ts: at("2026-09-06T10:06:00Z"),
        actor: Actor { kind: ActorKind::Human, name: ActorName::parse("dev").unwrap() },
        kind: EventKind::Commit,
        epistemic: Epistemic::Observed,
        body: "wired the worker import".to_owned(),
        refs,
        raw_ref: Some(RawRef::parse("logs/01J8Z6H0.log").unwrap()),
    }
}

#[test]
fn opening_creates_the_file_its_parent_and_the_schema() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("nested/registry.db");
    let store = Store::open(&path).unwrap();
    assert!(path.exists(), "the database file is created");
    let version: u32 = store.conn().query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn the_journal_is_write_ahead_logging() {
    let registry = Registry::open();
    let mode: String =
        registry.store.conn().query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn opening_an_existing_registry_again_changes_nothing() {
    let registry = Registry::seeded();
    let reopened = Store::open(registry.path()).unwrap();
    let listed = units::list(reopened.conn(), id('1')).unwrap();
    assert_eq!(listed, vec![unit()]);
}

#[test]
fn a_registry_from_a_later_version_is_refused() {
    let registry = Registry::open();
    registry
        .store
        .conn()
        .execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
        .unwrap();
    let error = Store::open(registry.path()).unwrap_err();
    assert!(
        matches!(error, Error::StoreTooNew { found, supported, .. }
            if found == SCHEMA_VERSION + 1 && supported == SCHEMA_VERSION),
        "{error}"
    );
}

#[test]
fn projects_round_trip_and_are_found_by_root() {
    let registry = Registry::open();
    let conn = registry.store.conn();
    projects::insert(conn, &project()).unwrap();
    assert_eq!(projects::get(conn, id('1')).unwrap(), Some(project()));
    assert_eq!(
        projects::find_by_root(conn, &PathBuf::from("/home/dev/acme")).unwrap(),
        Some(project())
    );
    assert_eq!(projects::find_by_root(conn, &PathBuf::from("/elsewhere")).unwrap(), None);
    assert_eq!(projects::list(conn).unwrap(), vec![project()]);

    let moved = Digest::parse("999999").unwrap();
    assert!(projects::update_recipe_hash(conn, id('1'), &moved).unwrap());
    assert_eq!(projects::get(conn, id('1')).unwrap().unwrap().recipe_hash, moved);
}

#[test]
fn two_projects_cannot_share_a_root() {
    let registry = Registry::open();
    let conn = registry.store.conn();
    projects::insert(conn, &project()).unwrap();
    let mut second = project();
    second.id = id('9');
    let error = projects::insert(conn, &second).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "{error}");
}

#[test]
fn units_round_trip_and_are_found_by_slug_and_branch() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    assert_eq!(units::get(conn, id('2')).unwrap(), Some(unit()));
    assert_eq!(
        units::find_by_slug(conn, id('1'), &Slug::parse("fix-worker-import").unwrap()).unwrap(),
        Some(unit())
    );
    assert_eq!(units::find_open_by_branch(conn, id('1'), &unit().branch).unwrap(), Some(unit()));
    assert_eq!(units::list_by_status(conn, id('1'), UnitStatus::Open).unwrap(), vec![unit()]);
    assert!(units::list_by_status(conn, id('1'), UnitStatus::Merged).unwrap().is_empty());
}

#[test]
fn a_branch_is_held_by_one_open_unit_and_freed_when_it_closes() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    let mut rival = unit();
    rival.id = id('8');
    rival.slug = Slug::parse("second-go").unwrap();

    let error = units::insert(conn, &rival).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "{error}");

    let later = at("2026-09-06T11:00:00Z");
    assert!(units::update_status(conn, id('2'), UnitStatus::Merged, later).unwrap());
    units::insert(conn, &rival).unwrap();
    assert_eq!(
        units::find_open_by_branch(conn, id('1'), &unit().branch).unwrap().map(|u| u.id),
        Some(rival.id)
    );
}

#[test]
fn a_unit_records_what_changed_about_it() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    let later = at("2026-09-06T12:00:00Z");
    let moved = BranchName::parse("nodal/fix-worker-import-2").unwrap();

    assert!(units::update_branch(conn, id('2'), &moved, later).unwrap());
    assert!(units::update_objective(conn, id('2'), None, later).unwrap());
    let stored = units::get(conn, id('2')).unwrap().unwrap();
    assert_eq!(stored.branch, moved);
    assert_eq!(stored.objective, None);
    assert_eq!(stored.updated_at, later);
    assert!(!units::update_status(conn, id('9'), UnitStatus::Merged, later).unwrap());
}

#[test]
fn bases_round_trip_and_are_found_by_fingerprint_and_platform() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    assert_eq!(bases::get(conn, id('3')).unwrap(), Some(base()));
    assert_eq!(
        bases::find(conn, id('1'), &base().ws_fingerprint, &base().platform).unwrap(),
        Some(base())
    );
    let elsewhere = Platform::parse("aarch64-apple-darwin").unwrap();
    assert_eq!(bases::find(conn, id('1'), &base().ws_fingerprint, &elsewhere).unwrap(), None);

    let used = at("2026-09-06T13:00:00Z");
    assert!(bases::touch(conn, id('3'), used).unwrap());
    assert_eq!(bases::list_for_project(conn, id('1')).unwrap()[0].last_used, used);
    assert!(bases::delete(conn, id('3')).unwrap());
    assert_eq!(bases::get(conn, id('3')).unwrap(), None);
}

#[test]
fn templates_round_trip_and_are_found_by_schema_fingerprint() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    templates::insert(conn, &template()).unwrap();
    assert_eq!(templates::get(conn, id('4')).unwrap(), Some(template()));
    assert_eq!(
        templates::find(conn, id('1'), &template().schema_fingerprint).unwrap(),
        Some(template())
    );
    assert_eq!(templates::list_for_project(conn, id('1')).unwrap(), vec![template()]);
    assert!(templates::delete(conn, id('4')).unwrap());
    assert_eq!(templates::list_for_project(conn, id('1')).unwrap(), vec![]);
}

#[test]
fn environments_round_trip_with_their_ports_and_fingerprints() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();

    assert_eq!(environments::get(conn, id('5')).unwrap(), Some(environment()));
    assert_eq!(environments::latest_for_unit(conn, id('2')).unwrap(), Some(environment()));
    assert_eq!(environments::list_by_state(conn, EnvState::Stopped).unwrap(), vec![environment()]);

    let active = at("2026-09-06T14:00:00Z");
    assert!(environments::update_state(conn, id('5'), EnvState::Running, active).unwrap());
    let schema = SchemaFp(Digest::parse("ddeeff").unwrap());
    assert!(environments::set_materialized(conn, id('5'), None, Some(&schema)).unwrap());
    let stored = environments::get(conn, id('5')).unwrap().unwrap();
    assert_eq!(stored.state, EnvState::Running);
    assert_eq!(stored.ws_fp_materialized, None);
    assert_eq!(stored.schema_fp_materialized, Some(schema));
    assert_eq!(stored.last_active, active);
    assert!(environments::touch(conn, id('5'), active).unwrap());
}

#[test]
fn a_unit_cannot_have_two_of_the_same_attempt() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    let mut again = environment();
    again.id = id('A');
    let error = environments::insert(conn, &again).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "{error}");

    again.attempt = 2;
    environments::insert(conn, &again).unwrap();
    assert_eq!(environments::list_for_unit(conn, id('2')).unwrap().len(), 2);
    assert_eq!(environments::latest_for_unit(conn, id('2')).unwrap().unwrap().attempt, 2);
}

#[test]
fn sessions_open_and_close_once() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    sessions::insert(conn, &session()).unwrap();

    assert_eq!(sessions::get(conn, id('6')).unwrap(), Some(session()));
    assert_eq!(sessions::list_open(conn, id('5')).unwrap(), vec![session()]);

    let ended = at("2026-09-06T15:00:00Z");
    assert!(sessions::end(conn, id('6'), ended).unwrap());
    assert!(!sessions::end(conn, id('6'), at("2026-09-06T16:00:00Z")).unwrap());
    assert_eq!(sessions::get(conn, id('6')).unwrap().unwrap().ended_at, Some(ended));
    assert!(sessions::list_open(conn, id('5')).unwrap().is_empty());
    assert_eq!(sessions::list_for_environment(conn, id('5')).unwrap().len(), 1);
}

#[test]
fn events_round_trip_and_read_back_in_order() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    events::append(conn, &event()).unwrap();

    let mut later = event();
    later.id = id('8');
    later.environment = None;
    later.raw_ref = None;
    later.kind = EventKind::Note;
    later.epistemic = Epistemic::Stated;
    events::append(conn, &later).unwrap();

    assert_eq!(events::get(conn, id('7')).unwrap(), Some(event()));
    assert_eq!(events::list_for_unit(conn, id('2')).unwrap(), vec![event(), later.clone()]);
    assert_eq!(events::list_since(conn, id('2'), id('7')).unwrap(), vec![later.clone()]);
    assert_eq!(events::list_recent(conn, id('2'), 1).unwrap(), vec![later]);
    assert_eq!(events::count_for_unit(conn, id('2')).unwrap(), 2);
}

#[test]
fn an_event_kind_the_contract_does_not_name_cannot_be_written() {
    let registry = Registry::seeded();
    let error = registry
        .store
        .conn()
        .execute(
            "INSERT INTO event (id, unit_id, ts, actor_kind, actor_name, kind, epistemic, body, \
             refs) VALUES (?, ?, 0, 'human', 'dev', 'gossip', 'stated', '', '{}')",
            rusqlite_params(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("CHECK"), "{error}");
}

/// The two identifiers the raw insert above needs, as text.
fn rusqlite_params() -> [String; 2] {
    [id::<EventId>('B').to_string(), id::<UnitId>('2').to_string()]
}

#[test]
fn a_lease_is_held_by_one_environment_until_it_lapses() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    let mut second = environment();
    second.id = id('A');
    second.attempt = 2;
    environments::insert(conn, &second).unwrap();

    let resource = ResourceKey::parse("port:5432").unwrap();
    let now = at("2026-09-06T10:00:00Z");
    let held = Lease {
        resource: resource.clone(),
        environment_id: id('5'),
        expires_at: at("2026-09-06T11:00:00Z"),
    };
    assert!(leases::acquire(conn, &held, now).unwrap());

    let rival = Lease { environment_id: id('A'), ..held.clone() };
    assert!(!leases::acquire(conn, &rival, now).unwrap(), "a live claim is not taken over");
    assert_eq!(leases::get(conn, &resource).unwrap(), Some(held.clone()));

    let renewed = Lease { expires_at: at("2026-09-06T12:00:00Z"), ..held.clone() };
    assert!(leases::acquire(conn, &renewed, now).unwrap(), "the holder renews its own claim");

    let after = at("2026-09-06T13:00:00Z");
    assert_eq!(leases::list_expired(conn, after).unwrap(), vec![renewed]);
    let taken_over = Lease { environment_id: id('A'), ..held.clone() };
    assert!(leases::acquire(conn, &taken_over, after).unwrap(), "a lapsed claim is taken over");

    assert!(!leases::release(conn, &resource, id('5')).unwrap(), "only the holder releases");
    assert!(leases::release(conn, &resource, id('A')).unwrap());
    assert_eq!(leases::get(conn, &resource).unwrap(), None);
    assert!(leases::list_for_environment(conn, id('A')).unwrap().is_empty());
}

#[test]
fn a_unit_is_written_from_one_host_until_the_claim_lapses() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    let now = at("2026-09-06T10:00:00Z");
    let laptop = HostName::parse("laptop").unwrap();
    let desktop = HostName::parse("desktop").unwrap();
    let held =
        Lock { unit_id: id('2'), host: laptop.clone(), expires_at: at("2026-09-06T11:00:00Z") };

    assert!(locks::acquire(conn, &held, now).unwrap());
    let rival = Lock { host: desktop.clone(), ..held.clone() };
    assert!(!locks::acquire(conn, &rival, now).unwrap());
    assert_eq!(locks::get(conn, id('2')).unwrap(), Some(held.clone()));
    assert_eq!(locks::list_for_host(conn, &laptop).unwrap(), vec![held.clone()]);

    let after = at("2026-09-06T12:00:00Z");
    assert!(locks::acquire(conn, &rival, after).unwrap());
    assert!(!locks::release(conn, id('2'), &laptop).unwrap());
    assert!(locks::release(conn, id('2'), &desktop).unwrap());
    assert_eq!(locks::get(conn, id('2')).unwrap(), None);
}

#[test]
fn a_row_another_row_points_at_is_not_removed_by_accident() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    let error = bases::delete(conn, id('3')).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "{error}");
}

#[test]
fn a_project_has_one_port_block_and_a_range_belongs_to_one_project() {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    let block = PortBlock { project_id: id('1'), first: 20_000, last: 20_099 };
    port_blocks::insert(conn, &block).unwrap();
    assert_eq!(port_blocks::get(conn, id('1')).unwrap(), Some(block));
    assert_eq!(port_blocks::list(conn).unwrap(), vec![block]);

    let again = PortBlock { first: 20_100, last: 20_199, ..block };
    let error = port_blocks::insert(conn, &again).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "one project, one block: {error}");

    let mut neighbour = project();
    neighbour.id = id('9');
    neighbour.root = PathBuf::from("/home/dev/other");
    projects::insert(conn, &neighbour).unwrap();
    let other = PortBlock { project_id: neighbour.id, ..block };
    let error = port_blocks::insert(conn, &other).unwrap_err();
    assert!(matches!(error, Error::StoreConflict { .. }), "one range, one project: {error}");
}

/// A registry holding a project, a unit, a base and two environments of that unit.
fn two_environments() -> Registry {
    let registry = Registry::seeded();
    let conn = registry.store.conn();
    bases::insert(conn, &base()).unwrap();
    environments::insert(conn, &environment()).unwrap();
    let mut second = environment();
    second.id = id('A');
    second.attempt = 2;
    environments::insert(conn, &second).unwrap();
    registry
}

/// The port `app` is claimed under, in the tests below.
fn allocation() -> PortAllocation {
    PortAllocation {
        port: 20_000,
        project_id: id('1'),
        environment_id: id('5'),
        name: PortName::parse("app").unwrap(),
    }
}

#[test]
fn a_port_is_granted_to_one_environment_under_one_name() {
    let registry = two_environments();
    let conn = registry.store.conn();
    let held = allocation();
    assert!(port_allocations::claim(conn, &held).unwrap());
    assert_eq!(port_allocations::get(conn, 20_000).unwrap(), Some(held.clone()));

    let rival = PortAllocation { environment_id: id('A'), ..held.clone() };
    assert!(!port_allocations::claim(conn, &rival).unwrap(), "a held port is not granted twice");
    let renamed = PortAllocation { port: 20_001, ..held };
    assert!(!port_allocations::claim(conn, &renamed).unwrap(), "one port per name");
}

#[test]
fn ports_are_read_back_by_range_and_by_name_and_are_returned_together() {
    let registry = two_environments();
    let conn = registry.store.conn();
    let app = allocation();
    let api = PortAllocation { port: 20_001, name: PortName::parse("api").unwrap(), ..app.clone() };
    assert!(port_allocations::claim(conn, &app).unwrap());
    assert!(port_allocations::claim(conn, &api).unwrap());

    assert_eq!(
        port_allocations::list_in_range(conn, 20_000, 20_099).unwrap(),
        vec![app.clone(), api]
    );
    assert_eq!(port_allocations::list_in_range(conn, 20_002, 20_099).unwrap(), vec![]);
    assert_eq!(port_allocations::find_by_name(conn, id('5'), &app.name).unwrap(), Some(app));

    assert_eq!(port_allocations::release_all(conn, id('A')).unwrap(), 0);
    assert_eq!(port_allocations::release_all(conn, id('5')).unwrap(), 2);
    assert!(port_allocations::list_for_environment(conn, id('5')).unwrap().is_empty());
    assert_eq!(port_allocations::get(conn, 20_000).unwrap(), None);
}
