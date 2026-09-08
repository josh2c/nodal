//! Acceptance test for T1.7: the port allocator, the fixed-port lease, and the scan
//! that says which granted ports are really bound.
//!
//! What this suite defends is that a port is granted rather than found. Two units of
//! one project take ports from one block and never the same one, whether they ask one
//! after the other or at the same moment from different processes; a port the project
//! pins is held by one unit at a time and the unit holding it is named; reclaim gives
//! everything back; and the scan reports a listener that is really there.
//!
//! The concurrency test opens one [`Store`] per thread, as separate processes would,
//! and holds every thread at a barrier so the claims genuinely overlap. Its assertion
//! is set equality: as many distinct ports as there were callers, every one inside the
//! block, is what "no double grants" means.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Barrier;

use nodal_core::error::Error;
use nodal_core::model::{
    BranchName, Digest, EnvId, EnvState, Environment, HostName, PortName, Ports, Project,
    ProjectId, ProjectName, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::services::{listeners, ports};
use nodal_core::store::{
    Store, environments, leases, port_allocations, port_blocks, projects, units,
};
use tempfile::TempDir;

/// How many callers race for a port in the concurrency test. Well past the number of
/// units a person opens at once, and still a small part of one block.
const RACERS: u32 = 16;

/// The alphabet a canonical ULID is written in.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The `n`th identifier of a run: a canonical ULID whose low bits count.
fn ulid_at(n: u32) -> String {
    let mut text = String::from("01J8Z6H0000000000000");
    for shift in (0..6).rev() {
        let digit = usize::try_from((u64::from(n) >> (shift * 5)) & 31).unwrap();
        text.push(char::from(CROCKFORD[digit]));
    }
    text
}

fn id_at<T: std::str::FromStr<Err = Error>>(n: u32) -> T {
    ulid_at(n).parse().unwrap()
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

/// The instant every test reads as the present.
fn now() -> Timestamp {
    at("2026-09-08T09:00:00Z")
}

fn project_id(n: u32) -> ProjectId {
    id_at(1000 + n)
}

fn unit_id(n: u32) -> UnitId {
    id_at(2000 + n)
}

fn env_id(n: u32) -> EnvId {
    id_at(3000 + n)
}

fn name(text: &str) -> PortName {
    PortName::parse(text).unwrap()
}

/// A registry in a directory that is removed when the test ends.
struct Registry {
    dir: TempDir,
    store: Store,
}

impl Registry {
    fn open() -> Self {
        let dir = TempDir::new().unwrap();
        let store = Store::open(dir.path().join("registry.db")).unwrap();
        Self { dir, store }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("registry.db")
    }

    /// Record a project.
    fn project(&self, n: u32) -> ProjectId {
        let id = project_id(n);
        projects::insert(
            self.store.conn(),
            &Project {
                id,
                root: PathBuf::from(format!("/home/dev/project-{n}")),
                name: ProjectName::parse(format!("project-{n}")).unwrap(),
                recipe_hash: Digest::parse("0f1e2d").unwrap(),
                created_at: now(),
            },
        )
        .unwrap();
        id
    }

    /// Record a unit of a project, and one environment of that unit.
    fn unit(&self, project: ProjectId, n: u32) -> (UnitId, EnvId) {
        let unit = unit_id(n);
        units::insert(
            self.store.conn(),
            &Unit {
                id: unit,
                project_id: project,
                slug: Slug::parse(format!("unit-{n}")).unwrap(),
                objective: None,
                objective_epistemic: None,
                branch: BranchName::parse(format!("nodal/unit-{n}")).unwrap(),
                parent_branch: None,
                status: UnitStatus::Open,
                created_at: now(),
                updated_at: now(),
            },
        )
        .unwrap();
        (unit, self.environment(unit, n))
    }

    /// Record one more materialisation of a unit.
    fn environment(&self, unit: UnitId, n: u32) -> EnvId {
        let id = env_id(n);
        environments::insert(
            self.store.conn(),
            &Environment {
                id,
                unit_id: unit,
                attempt: n + 1,
                home: PathBuf::from(format!("/home/dev/.nodal/e/{n}")),
                managed: true,
                base_id: None,
                ws_fp_materialized: None,
                schema_fp_materialized: None,
                host: HostName::parse("workstation").unwrap(),
                db_name: None,
                ports: Ports::default(),
                fixed_port: None,
                state: EnvState::Stopped,
                created_at: now(),
                last_active: now(),
            },
        )
        .unwrap();
        id
    }
}

/// The recipe's pinned ports, as `db.fixed_ports` holds them.
fn fixed(port: u16) -> BTreeMap<PortName, u16> {
    BTreeMap::from([(name("db"), port)])
}

/// When a lease taken in a test lapses. Every test reads [`now`] as the present, so a
/// claim that expires here is live throughout.
fn until() -> Timestamp {
    at("2026-09-08T10:00:00Z")
}

#[test]
fn two_units_of_one_project_get_distinct_ports() {
    let mut registry = Registry::open();
    let project = registry.project(1);
    let (_, first) = registry.unit(project, 1);
    let (_, second) = registry.unit(project, 2);
    let block = ports::ensure_block(&mut registry.store, project).unwrap();
    let conn = registry.store.conn();

    let wanted = [name("app"), name("api")];
    let one = ports::allocate(conn, block, first, &wanted).unwrap();
    let two = ports::allocate(conn, block, second, &wanted).unwrap();

    let granted: BTreeSet<u16> = one.0.values().chain(two.0.values()).copied().collect();
    assert_eq!(granted.len(), 4, "four names, four ports: {one:?} {two:?}");
    assert!(granted.iter().all(|port| block.contains(*port)), "{granted:?} is outside {block:?}");
    assert_eq!(one.0.keys().cloned().collect::<BTreeSet<_>>(), wanted.iter().cloned().collect());
}

#[test]
fn a_project_keeps_its_block_and_the_next_project_gets_another() {
    let mut registry = Registry::open();
    let first = registry.project(1);
    let second = registry.project(2);

    let block = ports::ensure_block(&mut registry.store, first).unwrap();
    assert_eq!(ports::ensure_block(&mut registry.store, first).unwrap(), block);
    assert_eq!(block.first, ports::RANGE_FIRST);

    let other = ports::ensure_block(&mut registry.store, second).unwrap();
    assert_eq!(other.first, block.last + 1);
    assert!(!other.contains(block.last), "{other:?} overlaps {block:?}");
    assert_eq!(port_blocks::list(registry.store.conn()).unwrap().len(), 2);
}

#[test]
fn asking_twice_returns_the_ports_already_held() {
    let mut registry = Registry::open();
    let project = registry.project(1);
    let (_, environment) = registry.unit(project, 1);
    let block = ports::ensure_block(&mut registry.store, project).unwrap();
    let conn = registry.store.conn();

    let wanted = [name("app")];
    let once = ports::allocate(conn, block, environment, &wanted).unwrap();
    let again = ports::allocate(conn, block, environment, &wanted).unwrap();

    assert_eq!(once, again, "a repeated step must not grant a second port");
    assert_eq!(port_allocations::list_for_environment(conn, environment).unwrap().len(), 1);
}

#[test]
fn a_fixed_port_conflict_names_the_holding_unit() {
    let registry = Registry::open();
    let project = registry.project(1);
    let (holder, holding_env) = registry.unit(project, 1);
    let (_, claimant) = registry.unit(project, 2);
    let conn = registry.store.conn();
    let pinned = fixed(54_322);

    assert_eq!(ports::hold_fixed(conn, holding_env, &pinned, now(), until()).unwrap(), None);
    assert!(ports::check_fixed(conn, holding_env, &pinned, now()).unwrap().is_empty());

    let reported = ports::check_fixed(conn, claimant, &pinned, now()).unwrap();
    assert_eq!(reported.len(), 1, "{reported:?}");
    let conflict = &reported[0];
    assert_eq!(conflict.port, 54_322);
    assert_eq!(conflict.name, name("db"));
    assert_eq!(conflict.unit, holder);
    assert_eq!(conflict.slug, Slug::parse("unit-1").unwrap());
    assert_eq!(conflict.environment, holding_env);
    assert_eq!(conflict.expires_at, until());

    let refused = ports::hold_fixed(conn, claimant, &pinned, now(), until()).unwrap();
    assert_eq!(refused.as_ref(), Some(conflict), "the claim is refused, and by the same words");
    assert!(leases::list_for_environment(conn, claimant).unwrap().is_empty());
}

#[test]
fn a_refused_claim_leaves_none_of_the_ports_it_had_taken() {
    let registry = Registry::open();
    let project = registry.project(1);
    let (_, holding_env) = registry.unit(project, 1);
    let (_, claimant) = registry.unit(project, 2);
    let conn = registry.store.conn();

    // The holder pins the second of the two ports the claimant wants.
    ports::hold_fixed(conn, holding_env, &fixed(54_323), now(), until()).unwrap();
    let wanted = BTreeMap::from([(name("api"), 54_321_u16), (name("db"), 54_323_u16)]);

    let refused = ports::hold_fixed(conn, claimant, &wanted, now(), until()).unwrap();
    assert_eq!(refused.map(|conflict| conflict.port), Some(54_323));
    assert!(
        leases::list_for_environment(conn, claimant).unwrap().is_empty(),
        "the port taken before the refusal must be given back"
    );
}

#[test]
fn a_lapsed_claim_is_not_a_conflict() {
    let registry = Registry::open();
    let project = registry.project(1);
    let (_, holding_env) = registry.unit(project, 1);
    let (_, claimant) = registry.unit(project, 2);
    let conn = registry.store.conn();
    let pinned = fixed(54_322);

    ports::hold_fixed(conn, holding_env, &pinned, now(), until()).unwrap();
    let later = at("2026-09-08T11:00:00Z");
    assert!(ports::check_fixed(conn, claimant, &pinned, later).unwrap().is_empty());
    assert_eq!(ports::hold_fixed(conn, claimant, &pinned, later, later).unwrap(), None);
    assert!(leases::list_for_environment(conn, holding_env).unwrap().is_empty());
}

#[test]
fn reclaim_gives_back_every_port_and_lease() {
    let mut registry = Registry::open();
    let project = registry.project(1);
    let (_, environment) = registry.unit(project, 1);
    let (_, other) = registry.unit(project, 2);
    let block = ports::ensure_block(&mut registry.store, project).unwrap();
    let conn = registry.store.conn();

    let granted = ports::allocate(conn, block, environment, &[name("app"), name("api")]).unwrap();
    ports::hold_fixed(conn, environment, &fixed(54_322), now(), until()).unwrap();

    let released = ports::release(conn, environment).unwrap();
    assert_eq!(
        released.allocated,
        granted.0.values().copied().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>()
    );
    assert_eq!(released.fixed, vec![54_322]);
    assert!(port_allocations::list_for_environment(conn, environment).unwrap().is_empty());
    assert!(leases::list_for_environment(conn, environment).unwrap().is_empty());

    // What was given back is available again, to this environment or to another.
    let reused = ports::allocate(conn, block, other, &[name("app")]).unwrap();
    assert_eq!(reused.0[&name("app")], block.first);
    assert_eq!(ports::release(conn, environment).unwrap(), ports::Released::default());
}

#[test]
fn concurrent_callers_never_get_the_same_port() {
    let registry = Registry::open();
    let project = registry.project(1);
    let (unit, _) = registry.unit(project, 1);
    let environments: Vec<EnvId> =
        (1..RACERS).map(|n| registry.environment(unit, 100 + n)).collect();
    let path = registry.path();
    let mut all: Vec<EnvId> = vec![env_id(1)];
    all.extend(environments);

    let barrier = Barrier::new(all.len());
    let granted: Vec<Ports> = std::thread::scope(|scope| {
        let handles: Vec<_> = all
            .iter()
            .map(|environment| {
                let (barrier, path, environment) = (&barrier, path.clone(), *environment);
                scope.spawn(move || {
                    // Each caller opens the registry for itself, as its own process would.
                    let mut store = Store::open(path).unwrap();
                    barrier.wait();
                    let block = ports::ensure_block(&mut store, project).unwrap();
                    ports::allocate(store.conn(), block, environment, &[name("app"), name("api")])
                        .unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect()
    });

    let block = port_blocks::get(registry.store.conn(), project).unwrap().unwrap();
    let ports: Vec<u16> = granted.iter().flat_map(|granted| granted.0.values().copied()).collect();
    let distinct: BTreeSet<u16> = ports.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        ports.len(),
        "{} callers came away with {} ports and {} distinct ones",
        all.len(),
        ports.len(),
        distinct.len()
    );
    assert_eq!(ports.len(), all.len() * 2);
    assert!(distinct.iter().all(|port| block.contains(*port)), "{distinct:?} is outside {block:?}");
    assert_eq!(
        port_blocks::list(registry.store.conn()).unwrap().len(),
        1,
        "one project, one block"
    );
    assert_eq!(
        port_allocations::list_in_range(registry.store.conn(), block.first, block.last)
            .unwrap()
            .len(),
        ports.len(),
        "the registry holds one row per port granted"
    );
}

#[test]
fn concurrent_claims_on_one_fixed_port_leave_one_holder_everybody_names() {
    let registry = Registry::open();
    let project = registry.project(1);
    let mut environments = Vec::new();
    for n in 1..=RACERS {
        environments.push(registry.unit(project, n).1);
    }
    let path = registry.path();
    let pinned = fixed(54_322);

    let barrier = Barrier::new(environments.len());
    let answers: Vec<(EnvId, Option<ports::FixedPortConflict>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = environments
            .iter()
            .map(|environment| {
                let (barrier, path, pinned, environment) =
                    (&barrier, path.clone(), pinned.clone(), *environment);
                scope.spawn(move || {
                    let store = Store::open(path).unwrap();
                    barrier.wait();
                    let answer =
                        ports::hold_fixed(store.conn(), environment, &pinned, now(), until())
                            .unwrap();
                    (environment, answer)
                })
            })
            .collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect()
    });

    let winners: Vec<EnvId> = answers
        .iter()
        .filter(|(_, answer)| answer.is_none())
        .map(|(environment, _)| *environment)
        .collect();
    assert_eq!(winners.len(), 1, "one holder at a time: {winners:?}");
    let conn = registry.store.conn();
    let held = leases::get(conn, &ports::resource(54_322).unwrap()).unwrap().unwrap();
    assert_eq!(held.environment_id, winners[0]);
    for (environment, answer) in answers.iter().filter(|(_, answer)| answer.is_some()) {
        let conflict = answer.as_ref().unwrap();
        assert_eq!(conflict.environment, winners[0], "{environment} was told the wrong holder");
        assert_eq!(conflict.port, 54_322);
        assert!(conflict.slug.as_str().starts_with("unit-"), "{:?}", conflict.slug);
        assert!(leases::list_for_environment(conn, *environment).unwrap().is_empty());
    }
}

/// The scan reports a listener that is really there, and only that one.
///
/// The two ports come from the operating system, not from the block: one socket is kept
/// open for the length of the test, and one is closed as soon as its port is known, so
/// the pair is a port that is bound and a port that is not.
#[cfg(target_os = "linux")]
#[test]
fn the_scan_finds_a_test_listener_and_not_a_closed_one() {
    let bound = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listening = bound.local_addr().unwrap().port();
    let quiet = closed.local_addr().unwrap().port();
    drop(closed);

    let granted = Ports(BTreeMap::from([(name("app"), listening), (name("api"), quiet)]));
    let scanned = listeners::scan(&granted).unwrap();

    assert_eq!(scanned.len(), 2, "{scanned:?}");
    let app = scanned.iter().find(|report| report.name == name("app")).unwrap();
    let api = scanned.iter().find(|report| report.name == name("api")).unwrap();
    assert_eq!((app.port, app.listening), (listening, true), "the test listener is bound");
    assert_eq!((api.port, api.listening), (quiet, false), "a closed socket is not bound");
    assert!(listeners::listening_ports().unwrap().contains(&listening));
}

/// Off Linux the scan says it cannot answer rather than answering nothing found.
#[cfg(not(target_os = "linux"))]
#[test]
fn the_scan_refuses_a_host_without_the_table_it_reads() {
    let granted = Ports(BTreeMap::from([(name("app"), 20_000)]));
    let error = listeners::scan(&granted).unwrap_err();
    assert!(matches!(error, Error::ListenerScanUnsupported { .. }), "{error}");
}
