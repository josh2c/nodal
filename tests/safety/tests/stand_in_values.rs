//! A unit is never called ready with a generated value its own generate step needs.
//!
//! This is the safety property the rest of this file asserts, and it is a property
//! about a promise rather than about isolation. `nodal new` answers with a home and
//! says the unit is ready to work in. A person, or an agent, then runs the project's
//! own commands in it. A project whose generate step reads `DATABASE_URL` — Prisma and
//! Drizzle both do — could not run that step in a fresh unit, because the name had no
//! value until the services were up. The create said ready and the first command
//! failed.
//!
//! So every name under `env.generated` that no adapter answered gets a stand-in at
//! create ([`nodal_core::env::stand_in`]). Three claims follow, and each is a test
//! below.
//!
//! **The step runs.** The fixture's `generate` script reads `DATABASE_URL`, parses it
//! and refuses without a port, exactly as a real one does. It runs in a fresh unit.
//!
//! **The stand-in is never silent.** A value that looks real and answers nothing is
//! worse than no value: a person reading a connection refused looks for a service
//! rather than for a placeholder. So the create names every stand-in it made, `nodal
//! env` marks each one, `nodal explain` says where the value came from, and the unit's
//! memory carries the same line for the agent that reads it.
//!
//! **One unit's stand-in is not another's.** The value is derived from the unit's own
//! handle, so two units of one project do not share a database name. And a name that
//! asks for a bare port takes no stand-in at all, because a port a process binds is
//! granted rather than derived: a derived one could be a port this project has already
//! granted to a sibling, and port isolation is the property that would break.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_core::env::files;
use nodal_core::model::manifest::Origin;
use nodal_safety::{Machine, stdout};

/// The name the fixture's generate step reads. The fixture declares it under
/// `env.generated`, and no adapter of a fresh unit produces it.
const GENERATED: &str = "DATABASE_URL";

/// A name the fixture declares under `env.generated` whose shape asks for a bare port.
/// Nothing produces it in a fresh unit, and nothing stands in for it either.
const BARE_PORT: &str = "PORT";

/// The command a person runs in a fresh unit, which is the whole point of the property.
const GENERATE: &str = "node scripts/generate.mjs";

/// What `.nodal/env` assigns `name`, with the quotes the writer puts around a value
/// taken off, which is what a shell reading the file gets.
fn assigned(home: &Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(home.join(files::ENV)).expect("the home is activated");
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(left, _)| *left == name)
        .map(|(_, value)| value.trim_matches('"').to_owned())
}

/// Run one command in the unit's home, with the environment the home carries.
fn in_home(machine: &Machine, home: &Path, command: &str) -> std::process::Output {
    machine.nodal_in(home, &["run", "--", "sh", "-c", command])
}

#[test]
fn the_generate_step_of_a_fresh_unit_runs_because_the_name_has_a_value() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    let value = assigned(&home, GENERATED).expect("the home assigns the generated name");
    assert!(value.starts_with("postgresql://"), "{GENERATED} is not a connection string: {value}");

    let generated = in_home(&machine, &home, GENERATE);
    assert!(
        generated.status.success(),
        "the generate step failed in a fresh unit: {}",
        stdout(&generated)
    );
    assert!(
        home.join("packages/config/src/generated/client.ts").is_file(),
        "the generate step wrote nothing"
    );
}

#[test]
fn the_manifest_marks_the_value_as_a_stand_in() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    let manifest = files::read_manifest(&home).expect("the home has a manifest");
    let name = manifest
        .env
        .iter()
        .find(|(name, _)| name.as_str() == GENERATED)
        .expect("the manifest names the generated value");
    assert_eq!(*name.1, Origin::StandIn, "the manifest does not mark {GENERATED} as a stand-in");
    assert!(
        manifest.missing.iter().all(|line| line.name.as_str() != GENERATED),
        "a name with a stand-in is still on the missing list"
    );
}

#[test]
fn the_create_names_every_stand_in_it_made() {
    let machine = Machine::new();
    let made = machine.nodal(&["new", "--name", "worker-import"]);
    assert!(made.status.success(), "the create failed");

    let report = stdout(&made);
    assert!(report.contains("stand-in"), "the create said nothing about a stand-in: {report}");
    assert!(report.contains(GENERATED), "the create did not name {GENERATED}: {report}");
}

#[test]
fn nodal_env_says_which_names_hold_a_stand_in() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    let reported = machine.nodal_in(&home, &["env"]);
    assert!(reported.status.success(), "nodal env failed");
    let text = stdout(&reported);
    assert!(text.contains("a stand-in"), "nodal env does not mark a stand-in: {text}");
    assert!(text.contains(GENERATED), "nodal env does not name {GENERATED}: {text}");
}

#[test]
fn nodal_explain_says_where_the_stand_in_came_from() {
    let machine = Machine::new();
    machine.unit("worker-import");

    let explained = machine.nodal(&["explain", "worker-import"]);
    assert!(explained.status.success(), "nodal explain failed");
    let text = stdout(&explained);
    assert!(
        text.contains("where the stand-in values came from"),
        "nodal explain has no stand-in section: {text}"
    );
    assert!(text.contains(GENERATED), "nodal explain does not name {GENERATED}: {text}");
    assert!(
        text.contains("no adapter produced it"),
        "nodal explain does not say why the value was made: {text}"
    );
}

#[test]
fn the_memory_tells_an_agent_that_the_value_is_a_stand_in() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    let memory =
        std::fs::read_to_string(home.join(files::WORKUNIT)).expect("the unit has a memory");
    assert!(memory.contains("stand-in env"), "the memory has no stand-in line: {memory}");
    assert!(memory.contains(GENERATED), "the memory does not name {GENERATED}: {memory}");
}

#[test]
fn two_units_of_one_project_do_not_share_a_stand_in() {
    let machine = Machine::new();
    let first = machine.unit("worker-import");
    let second = machine.unit("payroll-export");

    let one = assigned(&first, GENERATED).expect("the first home assigns it");
    let other = assigned(&second, GENERATED).expect("the second home assigns it");
    assert_ne!(one, other, "two units of one project were given one stand-in: {one}");
    assert!(one.ends_with("/worker-import"), "{one} does not name its own unit");
    assert!(other.ends_with("/payroll-export"), "{other} does not name its own unit");
}

#[test]
fn a_name_that_asks_for_a_bare_port_takes_no_stand_in() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    assert_eq!(
        assigned(&home, BARE_PORT),
        None,
        "{BARE_PORT} was given a stand-in; a port a process binds is granted, not derived"
    );
    let manifest = files::read_manifest(&home).expect("the home has a manifest");
    assert!(
        manifest.missing.iter().any(|line| line.name.as_str() == BARE_PORT),
        "{BARE_PORT} has no value and is not reported missing"
    );
}

#[test]
fn no_stand_in_a_unit_carries_is_a_bare_port_number() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");
    let manifest = files::read_manifest(&home).expect("the home has a manifest");

    for (name, _) in manifest.env.iter().filter(|(_, origin)| **origin == Origin::StandIn) {
        let value = assigned(&home, name.as_str()).expect("a marked name has a value");
        assert!(
            value.parse::<u16>().is_err(),
            "{name} holds the bare port {value}, which another unit may have been granted"
        );
    }
}
