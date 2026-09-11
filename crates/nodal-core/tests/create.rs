//! The create operation as a plan: what its steps do, what they undo, and what happens
//! to a run of it that a process never finished.
//!
//! The end-to-end test lives in `nodal-cli` (`tests/new.rs`), where a real binary is
//! killed with `SIGKILL`. This one is the same property without the race: the world a
//! killed run leaves is built here by hand, from the journal downwards, so that the
//! rebuild-and-roll-back path is exercised the same way on every machine.
//!
//! The base the plan clones is made here by hand for the same reason every identifier
//! is: a plan built twice has to be the same plan, and resolving a base is the other
//! operation's work, tested in `tests/substrate.rs`. It is made the way the substrate
//! makes one — a clone of the checkout, detached at the commit — so what a step reads
//! here is what a step reads in the product.
//!
//! The per-machine secrets file activation reads is `secrets.env` in the state
//! directory, and every fixture here has a temporary one, so no test in this file reads
//! or writes the file belonging to whoever is running it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use nodal_core::lifecycle;
use nodal_core::lifecycle::journal::{self, State, StepRecord, StepState};
use nodal_core::lifecycle::ops::new::{self, Params};
use nodal_core::lifecycle::{Action, guard, marker, ops};
use nodal_core::model::{OperationId, Timestamp};
use nodal_core::store::{Store, bases, environments, projects, units};
use serde_json::Value;
use support::World;

#[test]
fn the_plan_is_the_same_plan_however_it_is_built() {
    let fixture = World::plain();
    let params = fixture.create_params();
    let built = new::plan(&params).unwrap();
    assert_eq!(
        built.keys(),
        [
            "home.materialize",
            "home.relocate",
            "git.scrub",
            "git.refresh",
            "git.branch",
            "git.hide",
            "home.marker",
            "env.activate",
        ]
    );

    let record = journalled(&fixture, &params);
    let rebuilt = ops::rebuilders()
        .iter()
        .find(|entry| entry.kind() == new::KIND)
        .expect("this build knows how to rebuild a create")
        .rebuild(&record)
        .unwrap();
    assert_eq!(rebuilt.keys(), built.keys(), "a rebuilt plan lines up with the run's journal");
    assert_eq!(rebuilt.subject, built.subject);
}

#[test]
fn applying_the_steps_twice_changes_nothing_and_undoing_them_leaves_nothing() {
    let fixture = World::plain();
    let params = fixture.create_params();
    let home = params.environment.home.clone();

    let plan = new::plan(&params).unwrap();
    for step in &plan.steps {
        step.apply().unwrap();
        step.apply().unwrap();
    }
    assert_eq!(marker::read(&home).unwrap(), Some(params.unit.id));
    assert!(home.join(".envrc").is_file());

    for step in plan.steps.iter().rev() {
        step.undo().unwrap();
        step.undo().unwrap();
    }
    assert!(!home.exists(), "the home is gone, and undoing again is not a failure");
}

#[test]
fn a_run_whose_process_is_gone_is_rebuilt_and_rolled_back_to_nothing() {
    let fixture = World::plain();
    let params = fixture.create_params();
    let home = params.environment.home.clone();
    let record = journalled(&fixture, &params);

    // The world as a create killed inside its sixth step would have left it.
    let plan = new::plan(&params).unwrap();
    let store = fixture.store();
    for (position, step) in plan.steps.iter().enumerate().take(5) {
        step.apply().unwrap();
        mark(&store, record.id, (position, &step.key()), StepState::Applied, None);
    }
    assert!(home.join(".git").is_dir(), "the killed run left a home behind");
    drop(store);

    let mut store = fixture.store();
    let reported = lifecycle::resolve(&mut store, &ops::rebuilders()).unwrap();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(
        reported[0].action,
        Action::RolledBack {
            undone: vec![
                String::from("git.branch"),
                String::from("git.refresh"),
                String::from("git.scrub"),
                String::from("home.relocate"),
                String::from("home.materialize"),
            ]
        },
        "every step the journal saw is undone, in reverse"
    );

    assert!(!home.exists(), "nothing of the killed run is left on disk");
    assert!(
        units::get(store.conn(), params.unit.id).unwrap().is_none(),
        "and none in the registry"
    );
    assert!(environments::get(store.conn(), params.environment.id).unwrap().is_none());
    assert_eq!(journal::get(store.conn(), record.id).unwrap().unwrap().state, State::RolledBack);
    assert_eq!(lifecycle::resolve(&mut store, &ops::rebuilders()).unwrap(), Vec::new());
}

#[test]
fn a_base_that_holds_what_a_real_one_holds_is_cloned() {
    let fixture = World::plain();
    let marked = nodal_fixture::read_only::mark_git_objects(&fixture.base);
    if marked == 0 {
        eprintln!("this filesystem holds no extended attributes; nothing to prove here");
        return;
    }
    let locked = fixture.base.join("vendor/store/library");
    std::fs::create_dir_all(locked.parent().unwrap()).unwrap();
    std::fs::write(&locked, "bytes a package manager wrote").unwrap();
    assert!(nodal_fixture::read_only::mark(&locked));
    nodal_fixture::read_only::lock(&locked);
    nodal_fixture::read_only::lock(locked.parent().unwrap());

    let params = fixture.create_params();
    let home = params.environment.home.clone();
    let plan = new::plan(&params).unwrap();
    for step in &plan.steps {
        step.apply().expect("a base a real machine would produce is cloned");
    }

    let object = home.join(".git/objects");
    assert!(object.is_dir(), "the clone has no object directory");
    assert!(
        nodal_fixture::read_only::marked(&home.join("vendor/store/library")),
        "the attribute on a file nothing may write did not survive"
    );

    for step in plan.steps.iter().rev() {
        step.undo().unwrap();
    }
    assert!(!home.exists(), "a home holding a read-only directory was left behind");
}

#[test]
fn a_materialize_that_fails_part_way_leaves_no_home_and_no_row() {
    let fixture = World::plain();
    // SAFETY: `geteuid` takes no argument, reads no memory the caller owns and has no
    // failure case; it is unsafe only because it is a foreign function.
    let effective_user = unsafe { libc::geteuid() };
    if effective_user == 0 {
        eprintln!("root reads a file whatever its mode; nothing to prove here");
        return;
    }
    // A file the copier reaches and cannot read, so the clone stops with the home part
    // made, which is the state the undo has to take away.
    let shut = fixture.base.join("vendor/store/unreadable");
    std::fs::create_dir_all(shut.parent().unwrap()).unwrap();
    std::fs::write(&shut, "bytes nobody may read").unwrap();
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
    nodal_fixture::read_only::lock(shut.parent().unwrap());

    let params = fixture.create_params();
    let home = params.environment.home.clone();
    let store = fixture.store();
    projects::insert(store.conn(), &params.project).unwrap();
    bases::insert(store.conn(), &fixture.base_row()).unwrap();
    drop(store);

    let plan = new::plan(&params).unwrap();
    let materialize = plan.steps.first().expect("the plan has a first step");
    assert_eq!(materialize.key(), "home.materialize");
    let failed = materialize.apply().expect_err("a file nobody may read stopped the clone");
    assert!(failed.to_string().contains("unreadable"), "{failed}");
    assert!(home.exists(), "the failed clone left a part-made home, which is what undo is for");

    materialize.undo().expect("the undo removes a part-made home");
    assert!(!home.exists(), "a part-made home was left behind");
    assert!(materialize.undo().is_ok(), "undoing again is not a failure");

    let store = fixture.store();
    assert!(units::get(store.conn(), params.unit.id).unwrap().is_none(), "a row was written");
    assert!(environments::get(store.conn(), params.environment.id).unwrap().is_none());
}

#[test]
fn a_home_is_refused_inside_a_project_or_another_units_home() {
    let fixture = World::plain();
    let params = fixture.create_params();
    let store = fixture.store();
    projects::insert(store.conn(), &params.project).unwrap();
    bases::insert(store.conn(), &fixture.base_row()).unwrap();
    units::insert(store.conn(), &params.unit).unwrap();
    environments::insert(store.conn(), &params.environment).unwrap();

    guard::placement(store.conn(), &fixture.state.join("elsewhere"), &fixture.source).unwrap();

    let inside_project = fixture.source.join("nested");
    let refused = guard::placement(store.conn(), &inside_project, &fixture.source).unwrap_err();
    assert!(refused.to_string().contains(guard::SOURCE), "{refused}");

    let inside_home = params.environment.home.join("packages").join("web");
    let refused = guard::placement(store.conn(), &inside_home, Path::new("/nowhere")).unwrap_err();
    assert!(refused.to_string().contains(guard::HOME), "{refused}");
}

/// Journal a run of this plan as started by a process that is no longer there.
fn journalled(world: &World, params: &Params) -> journal::Operation {
    support::journal_of(world, &new::plan(params).unwrap())
}

/// Write down that a step of a run reached a state, with what it produced.
fn mark(
    store: &Store,
    id: OperationId,
    step: (usize, &str),
    state: StepState,
    output: Option<Value>,
) {
    let (position, key) = step;
    let record = StepRecord {
        position: u32::try_from(position).unwrap(),
        key: key.to_owned(),
        state,
        output,
        updated_at: Timestamp::now(),
    };
    journal::mark_step(store.conn(), id, &record).unwrap();
}
