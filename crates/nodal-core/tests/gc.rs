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

/// The retention this project asks for: a record may go as soon as its run is over.
const RECIPE: &str = "[reclaim]\ntrash_retention = 0\n";

/// A world with the unit's rows, its home on the disk, and a recipe that keeps nothing.
fn world() -> World {
    let world = World::plain();
    std::fs::write(world.source.join("nodal.toml"), RECIPE).unwrap();
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
