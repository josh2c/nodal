//! What `nodal gc` does about the records the runner writes before an operation.
//!
//! The runner commits a unit's home to `refs/nodal/<unit>/pre/<operation>` before the
//! first step of any operation that changes the home ([`nodal_core::git::snapshot`]).
//! Until this sweep nothing ever removed one, so a home that had been merged, adopted
//! and reclaimed kept every record it ever took.
//!
//! A record is kept for the window the project asked the trash for, and a record of a
//! run that is still open is never removed: that record is what the run's own rollback
//! reads.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

mod support;

use nodal_core::git::refs;
use nodal_core::lifecycle::journal::{self, State};
use nodal_core::lifecycle::ops::{gc, reclaim};
use nodal_core::model::{OperationId, Timestamp};
use nodal_core::output::view::{Swept, UnitDetail};
use nodal_core::runtime::processes::Live;
use nodal_core::runtime::{ls, show};
use nodal_safety::git::{git, git_ok};
use support::World;

/// A world whose records may go as soon as their runs are over.
fn world() -> World {
    keeping_for(0)
}

/// A world with the unit's rows, its home on the disk, and a recipe that keeps a home
/// and a record for `days`.
fn keeping_for(days: u32) -> World {
    let world = World::plain();
    let recipe = format!("[reclaim]\ntrash_retention = {days}\n");
    std::fs::write(world.source.join("nodal.toml"), recipe).unwrap();
    world.insert_unit();
    let home = world.home();
    std::fs::create_dir_all(home.parent().unwrap()).unwrap();
    git_ok(
        home.parent().unwrap(),
        &["clone", "--quiet", "--", &world.base.display().to_string(), &home.display().to_string()],
    );
    world
}

/// Journal a run of this world's reclaim, and write the record it would have taken.
///
/// The ref is written by hand so that this asserts the sweep and not the operation that
/// would have taken one. Its name is the one the runner uses ([`refs::pre`]).
fn record(world: &World, finished: Option<State>) -> String {
    let plan = reclaim::plan(&world.reclaim_params()).unwrap();
    let run = support::journal_of(world, &plan);
    if let Some(state) = finished {
        let store = world.store();
        assert!(journal::finish(store.conn(), run.id, state, Timestamp::now()).unwrap());
    }
    let reference = refs::pre(&world.unit_id.to_string(), &run.id.to_string());
    let head = git(world.home(), &["rev-parse", "HEAD"]);
    git_ok(world.home(), &["update-ref", &reference, head.trim()]);
    reference
}

/// Every ref of the unit's namespace that is on the disk now.
fn refs_of(world: &World) -> String {
    let prefix = format!("{}{}/", refs::NAMESPACE, world.unit_id);
    git(world.home(), &["for-each-ref", "--format=%(refname)", &prefix])
}

/// Sweep, with nothing asked for beyond the ordinary work.
fn sweep(world: &World) -> Swept {
    let mut store = world.store();
    gc::collect(&mut store, Timestamp::now(), &gc::Options::default()).unwrap()
}

/// What `nodal show` answers for this world's unit.
fn shown(world: &World) -> UnitDetail {
    let store = world.store();
    let project = world.project();
    let listed = ls::list(store.conn(), &Live, &project, Timestamp::now()).unwrap();
    show::detail(store.conn(), listed, &world.unit()).unwrap()
}

/// Whether a report lists this ref.
fn lists(detail: &UnitDetail, reference: &str) -> bool {
    detail.snapshots.iter().any(|snapshot| snapshot.reference == reference)
}

/// A record of a run that is over goes once the project's retention has run out, and
/// the report that offered to read it back stops offering it.
#[test]
fn a_record_of_a_finished_run_is_collected_and_stops_being_listed() {
    let world = world();
    let committed = record(&world, Some(State::Committed));
    let rolled_back = record(&world, Some(State::RolledBack));
    assert!(lists(&shown(&world), &committed), "the report lists it before the sweep");

    let swept = sweep(&world);

    assert!(swept.records.contains(&committed), "the sweep says what it removed: {swept:?}");
    assert!(swept.records.contains(&rolled_back), "a rolled-back run's record too: {swept:?}");
    let left = refs_of(&world);
    assert!(!left.contains(&committed), "the record is still on the disk: {left}");
    assert!(!left.contains(&rolled_back), "the record is still on the disk: {left}");
    let detail = shown(&world);
    assert!(!lists(&detail, &committed), "the report still lists a record that has gone");
    assert!(!lists(&detail, &rolled_back), "the report still lists a record that has gone");
}

/// A record inside the window the project asked for stays, and is still offered.
///
/// The run is over and the record could go on that count alone. What keeps it is the
/// clock: the commit was made now and the project keeps a record for a fortnight.
#[test]
fn a_record_inside_the_window_the_project_asked_for_stays() {
    let world = keeping_for(14);
    let committed = record(&world, Some(State::Committed));

    let swept = sweep(&world);

    assert!(swept.records.is_empty(), "a record inside its window went: {swept:?}");
    assert!(refs_of(&world).contains(&committed), "the record went");
    assert!(lists(&shown(&world), &committed), "the report stopped offering a record that is here");
}

/// A second sweep of a swept home removes nothing and reports nothing.
///
/// Every record the first sweep could take is gone, and a ref that is not there is not
/// a failure to remove. This is the property that makes `nodal gc` a thing to run on a
/// timer.
#[test]
fn a_second_sweep_removes_nothing() {
    let world = world();
    let committed = record(&world, Some(State::Committed));
    let first = sweep(&world);
    assert_eq!(first.records, [committed], "the first sweep takes the record: {first:?}");

    let second = sweep(&world);

    assert!(second.records.is_empty(), "the second sweep removed something: {second:?}");
    assert!(second.leftovers.is_empty(), "the second sweep reported something: {second:?}");
}

/// A run that is still open keeps its record, however old the record is. That record is
/// what the run's own rollback reads, and the sweep never takes it.
#[test]
fn a_record_of_an_open_run_is_never_collected() {
    let world = world();
    let open = record(&world, None);

    let swept = sweep(&world);

    assert!(swept.records.is_empty(), "the sweep removed an open run's record: {swept:?}");
    assert!(refs_of(&world).contains(&open), "the record of an open run went");
    assert!(lists(&shown(&world), &open), "the report stopped listing a record that is here");
}

/// A project whose recipe will not load is reported, and nothing of it is swept.
///
/// The window it asked for is the one thing the sweep needs from that file. A window
/// nobody can read is not a window to guess at: the default is shorter than many a
/// project asks for, and acting on it would take a record away early.
#[test]
fn a_project_whose_recipe_will_not_load_is_reported_and_swept_for_nothing() {
    let world = world();
    let committed = record(&world, Some(State::Committed));
    std::fs::write(world.source.join("nodal.toml"), "reclaim = [\n").unwrap();

    let swept = sweep(&world);

    assert!(swept.records.is_empty(), "the sweep acted on an unreadable window: {swept:?}");
    assert!(refs_of(&world).contains(&committed), "the record went");
    let reported: Vec<&str> =
        swept.leftovers.iter().filter(|left| left.kind == "project").map(|l| &*l.detail).collect();
    assert_eq!(reported.len(), 1, "the project is one line of the report: {swept:?}");
    assert!(reported[0].contains("project"), "the line names the project: {reported:?}");
}

/// Every other ref of the namespace is left where it is. The work-in-progress ref, the
/// branch before a squash and the copies a home took of the checkout are not records of
/// a run, and no retention applies to them. A failed run keeps its record, which is the
/// record most worth keeping, and so does one whose journal row is no longer there.
#[test]
fn only_the_records_of_runs_are_collected() {
    let world = world();
    let mut kept = vec![
        refs::wip(&world.unit_id.to_string()),
        refs::premerge(&world.unit_id.to_string()),
        refs::target(&world.unit_id.to_string()),
        refs::pre(
            &world.unit_id.to_string(),
            &OperationId::from_ulid(ulid::Ulid::new()).to_string(),
        ),
    ];
    let head = git(world.home(), &["rev-parse", "HEAD"]);
    for reference in &kept {
        git_ok(world.home(), &["update-ref", reference, head.trim()]);
    }
    kept.push(record(&world, Some(State::Failed)));

    let swept = sweep(&world);

    assert!(swept.records.is_empty(), "the sweep removed a ref that records no run: {swept:?}");
    let left = refs_of(&world);
    for reference in &kept {
        assert!(left.contains(reference.as_str()), "{reference} went: {left}");
    }
}
