//! Port isolation: a port is granted to one unit at a time, however many ask at once.
//!
//! A port is granted from the project's block and written down, rather than found by
//! looking for a free socket. The difference shows up under concurrency: two units
//! created in the same second by two agents would both find the same socket free, and
//! the second dev server to start would fail on a machine the person is not watching.
//!
//! The concurrency check has the shape `crates/nodal-core/tests/ports.rs` gave it in
//! T1.7. Every caller opens the registry for itself, as a separate process would, and
//! they are held at a barrier so the claims genuinely overlap. What is different here is
//! that the units are real: they are made by `nodal new` on the fixture project, and the
//! registry they race in is the one those commands wrote. Their ports are given back
//! first, because a repeated grant answers with the ports already held and would prove
//! nothing about a race.
//!
//! The last check reads which of the granted ports something is really listening on.
//! That reads `/proc/net/tcp*`, which macOS does not publish, so it is skipped there and
//! says so.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeSet;
use std::sync::Barrier;

use nodal_core::model::{EnvId, PortName, Ports};
use nodal_core::services::{listeners, ports};
use nodal_core::store::{Store, environments, port_allocations, port_blocks};
use nodal_safety::InState as _;
use nodal_safety::{Machine, platform};

/// How many callers race for a port. The number T1.7 chose: well past the units a person
/// opens at once, and still a small part of one block.
const RACERS: usize = 16;

/// The names every unit of the fixture is granted a port under: its dev server, and the
/// one service the recipe says each copy needs its own of.
fn wanted() -> Vec<PortName> {
    vec![PortName::parse("app").unwrap(), PortName::parse("redis").unwrap()]
}

/// Every port the registry has granted to this materialisation.
fn granted(store: &Store, environment: EnvId) -> Ports {
    environments::get(store.conn(), environment)
        .unwrap()
        .expect("the materialisation is in the registry")
        .ports
}

/// The materialisations of every unit this machine has, in creation order.
fn materialisations(machine: &Machine, store: &Store) -> Vec<EnvId> {
    let project = machine.project(store);
    let mut all: Vec<EnvId> = Vec::new();
    for unit in nodal_core::store::units::list(store.conn(), project.id).unwrap() {
        all.extend(
            environments::list_for_unit(store.conn(), unit.id).unwrap().iter().map(|row| row.id),
        );
    }
    all.sort();
    all
}

#[test]
fn two_units_of_one_project_are_never_granted_one_port() {
    let machine = Machine::new();
    machine.unit("worker-import");
    machine.unit("payroll-export");

    let store = machine.store();
    let project = machine.project(&store);
    let block = port_blocks::get(store.conn(), project.id).unwrap().expect("the project's block");
    let all: Vec<u16> = materialisations(&machine, &store)
        .into_iter()
        .flat_map(|environment| granted(&store, environment).0.into_values())
        .collect();

    let distinct: BTreeSet<u16> = all.iter().copied().collect();
    assert_eq!(all.len(), 4, "two units, two names each: {all:?}");
    assert_eq!(distinct.len(), all.len(), "two units were granted one port: {all:?}");
    assert!(distinct.iter().all(|port| block.contains(*port)), "{distinct:?} is outside {block:?}");
}

#[test]
fn concurrent_grants_never_hand_out_one_port_twice() {
    let machine = Machine::new();
    for index in 0..RACERS {
        machine.unit(&format!("racer-{index}"));
    }
    let store = machine.store();
    let project = machine.project(&store);
    let all = materialisations(&machine, &store);
    assert_eq!(all.len(), RACERS, "one materialisation per unit");

    // Give every port back, so that each caller is asking for a port and not being told
    // which one it already holds.
    for environment in &all {
        ports::release(store.conn(), *environment).unwrap();
        assert!(
            port_allocations::list_for_environment(store.conn(), *environment).unwrap().is_empty(),
            "a port was not given back before the race"
        );
    }
    let path = machine.registry();
    drop(store);

    let barrier = Barrier::new(all.len());
    let taken: Vec<Ports> = std::thread::scope(|scope| {
        let handles: Vec<_> = all
            .iter()
            .map(|environment| {
                let (barrier, path, environment) = (&barrier, path.clone(), *environment);
                scope.spawn(move || {
                    // Each caller opens the registry for itself, as its own process would.
                    let mut store = Store::open(path).unwrap();
                    barrier.wait();
                    let block = ports::ensure_block(&mut store, project.id).unwrap();
                    ports::allocate(store.conn(), block, environment, &wanted()).unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect()
    });

    let store = machine.store();
    let block = port_blocks::get(store.conn(), project.id).unwrap().expect("the project's block");
    let held: Vec<u16> = taken.iter().flat_map(|ports| ports.0.values().copied()).collect();
    let distinct: BTreeSet<u16> = held.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        held.len(),
        "{} callers came away with {} ports and {} distinct ones",
        all.len(),
        held.len(),
        distinct.len()
    );
    assert_eq!(held.len(), all.len() * wanted().len());
    assert!(distinct.iter().all(|port| block.contains(*port)), "{distinct:?} is outside {block:?}");
    assert_eq!(port_blocks::list(store.conn()).unwrap().len(), 1, "one project, one block");
}

#[test]
fn a_bound_port_is_seen_for_the_unit_that_holds_it_and_for_no_other() {
    if !platform::reads_bound_ports()
        && platform::skipped(
            "a bound port is seen for its own unit only",
            "the scan reads /proc/net/tcp*, which this host does not publish",
        )
    {
        return;
    }
    let machine = Machine::new();
    machine.unit("worker-import");
    machine.unit("payroll-export");
    let store = machine.store();
    let all = materialisations(&machine, &store);
    let (mine, theirs) = (granted(&store, all[0]), granted(&store, all[1]));

    let app = PortName::parse("app").unwrap();
    let port = mine.0[&app];
    let Ok(socket) = std::net::TcpListener::bind(("127.0.0.1", port)) else {
        assert!(platform::skipped(
            "a bound port is seen for its own unit only",
            "something outside this suite already holds the port the project granted",
        ));
        return;
    };

    let seen = listeners::scan(&mine).unwrap();
    let bound: Vec<&PortName> =
        seen.iter().filter(|report| report.listening).map(|report| &report.name).collect();
    assert_eq!(bound, vec![&app], "the unit that holds the port is the unit the socket shows in");

    let elsewhere = listeners::scan(&theirs).unwrap();
    assert!(
        elsewhere.iter().all(|report| !report.listening),
        "a socket opened on one unit's port is reported for another unit: {elsewhere:?}"
    );
    drop(socket);
}
