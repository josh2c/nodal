//! The behaviour the operation model must keep while it is refactored.
//!
//! Two properties are locked here, one per operation, and they are the specification
//! the step-output work is written against.
//!
//! **The plan is a value, and its shape is fixed.** [`Plan::keys`] is the list of step
//! keys in the order they are applied, and it is what a rebuilt plan is lined up with.
//! A refactor that reorders, renames, adds or drops a step changes what an interrupted
//! run of an older build is undone by, so every such change has to be deliberate. The
//! `plan_*` tests here state the keys of all four lifecycle operations, in every shape
//! their parameters put them in, and check that the same plan comes back out of the
//! journal.
//!
//! **A run that is resumed writes what a run that finished wrote.** This is the whole
//! reason the journal exists. The `resume_*` tests run each operation twice against an
//! identical world: once to completion, and once as a run whose process died after step
//! `k` and which the next invocation took over. The registry rows and the events of the
//! two runs must be equal.
//!
//! ## Two of them are not equal, and that is recorded rather than hidden
//!
//! A step takes `&self`, so it cannot hand a value to the step after it or to the
//! registry write at the end. Four operations work around that with an
//! `Arc<OnceLock<T>>` that the step fills and the commit reads. The journal has a row
//! per step but no column for what the step produced, so a plan rebuilt from the
//! journal gets an empty lock, and a resumed run skips the step that would have filled
//! it. The commit then reads nothing where the first run read something.
//!
//! Two of those have a consequence in the registry, and both are reproduced below:
//!
//! - `resume_reclaim_keeps_the_session_row_of_a_group_it_could_not_stop` — the commit
//!   asks the teardown which process groups survived, so that their session rows stay
//!   open for `nodal gc` to act on later. A resumed reclaim asks an empty lock, gets no
//!   groups, and closes every open row. The surviving group is then orphaned: nothing
//!   records it, so nothing ever reaps it.
//! - `resume_new_writes_the_relocation_event_a_first_run_wrote` — the commit writes the
//!   event that says which caches the relocation removed, because an event names a unit
//!   and no unit row exists until the commit runs. A resumed create writes no such
//!   event, so `nodal explain` cannot say why a build in that home started cold.
//!
//! Both are the specification for the step-output change: give a step a typed output,
//! write it into the journal beside `applied`, and hand the outputs to the commit. When
//! that lands, both tests pass on their own, and `Recovery::Resume` becomes a choice an
//! operation is allowed to make.
//!
//! ## What the two were, and what they are now
//!
//! Both were expected failures until the step-output column landed, gated so that a
//! known bug did not turn the suite red and so that a fix could not land unnoticed.
//! They are plain assertions now, which is what that gate said to do the day it went
//! green. `NODAL_ENFORCE_STEP_OUTPUTS` is gone with it.
//!
//! The create half kept its shape: a first run and a resumed run write the same rows
//! and the same events, and the reason they do is that the relocation report the first
//! run's step made is in the journal for the second run's commit to read.
//!
//! The reclaim half could not. A comparison between two runs stated the property while
//! the value lived in memory, because a first run had it and a resumed run did not.
//! Both now read it from the same column, so the two runs agree whatever the commit
//! does with the value, and the comparison asserts nothing. What is asserted instead is
//! the substance: a teardown that reports a surviving group leaves that group's session
//! row open, and one that reports none closes it.
//!
//! ## What is *not* claimed here, and what the two locks are for
//!
//! Neither divergence ever damaged data. All four operations recover by
//! [`Recovery::RollBack`], so no invocation of `nodal` reached the resume path for
//! them: a killed create or reclaim is undone, not finished, and its commit never runs
//! a second time. The one operation that does resume is the base build, which carried
//! no sink at all, so it had no step output for a rebuild to lose. Both divergences
//! were latent, and they are reproduced here by journalling the run as
//! [`Recovery::Resume`], which is the single line that separates the two paths.
//!
//! That is what these two locks were for. Journalled step outputs are the
//! **precondition for granting [`Recovery::Resume`] to a lifecycle operation**, not a
//! repair of live damage. An operation given that recovery mode before the outputs
//! reached the journal would have lost the values its commit reads, silently, on
//! exactly the runs a person cannot watch. That order of work is now the right way
//! round, and these two say so on every run.
//!
//! The per-machine secrets file activation reads is `secrets.env` in the state
//! directory, and every fixture here has a temporary one, so no test in this file reads
//! or writes the file belonging to whoever is running it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod support;

use nodal_core::lifecycle::ops::{adopt, merge, new, reclaim};
use nodal_core::lifecycle::{Plan, Recovery, ops};

use support::{World, journal_of};

// ---------------------------------------------------------------------------
// The plans.
// ---------------------------------------------------------------------------

#[test]
fn plan_new_has_seven_steps_and_rolls_back() {
    let world = World::new();
    let params = world.create_params();
    let plan = new::plan(&params).unwrap();
    assert_eq!(plan.kind, new::KIND);
    assert_eq!(plan.recovery, Recovery::RollBack);
    assert_eq!(
        plan.keys(),
        [
            "home.materialize",
            "home.relocate",
            "git.scrub",
            "git.branch",
            "git.hide",
            "home.marker",
            "env.activate",
        ]
    );
    assert_rebuilds_the_same(&world, &plan, new::KIND);
}

#[test]
fn plan_adopt_has_three_steps_in_place_and_eight_when_it_materializes() {
    let world = World::new();

    let mut params = world.adopt_params();
    params.source = adopt::Source::InPlace;
    let in_place = adopt::plan(&params).unwrap();
    assert_eq!(in_place.kind, adopt::KIND);
    assert_eq!(in_place.recovery, Recovery::RollBack);
    assert_eq!(in_place.keys(), ["git.hide", "home.marker", "env.activate"]);
    assert_rebuilds_the_same(&world, &in_place, adopt::KIND);

    let params = world.adopt_params();
    let materialized = adopt::plan(&params).unwrap();
    assert_eq!(
        materialized.keys(),
        [
            "home.materialize",
            "home.relocate",
            "git.scrub",
            "git.fetch-branch",
            "git.branch",
            "git.hide",
            "home.marker",
            "env.activate",
        ],
        "a materialised adopt is a create with the branch fetched from the checkout"
    );
    assert_rebuilds_the_same(&world, &materialized, adopt::KIND);
}

#[test]
fn plan_merge_has_a_step_per_stage_and_only_the_rebase_when_it_resumes() {
    let world = World::new();

    let params = world.merge_params(merge::Stages::all(), false);
    let whole = merge::plan(&params).unwrap();
    assert_eq!(whole.kind, merge::KIND);
    assert_eq!(whole.recovery, Recovery::RollBack);
    assert_eq!(
        whole.keys(),
        [
            "target.fetch",
            "work.commit",
            "premerge.record",
            "branch.squash",
            "branch.rebase",
            "target.forward",
        ]
    );
    assert_rebuilds_the_same(&world, &whole, merge::KIND);

    let params = world.merge_params(merge::Stages::all().without(merge::Stage::Squash), false);
    assert_eq!(
        merge::plan(&params).unwrap().keys(),
        ["target.fetch", "work.commit", "branch.rebase", "target.forward"],
        "dropping the squash drops the record of what preceded it too"
    );

    let params = world.merge_params(merge::Stages::all().without(merge::Stage::Commit), false);
    assert_eq!(
        merge::plan(&params).unwrap().keys(),
        ["target.fetch", "premerge.record", "branch.squash", "branch.rebase", "target.forward"]
    );

    let params = world.merge_params(merge::Stages::all(), true);
    assert_eq!(
        merge::plan(&params).unwrap().keys(),
        ["target.fetch", "branch.rebase", "target.forward"],
        "a run that continues a stopped rebase rewrites nothing a second time"
    );

    let params = world.merge_params(merge::Stages::default(), false);
    assert_eq!(
        merge::plan(&params).unwrap().keys(),
        ["target.fetch", "target.forward"],
        "with no stage to run the plan still fetches the target and forwards it"
    );
}

#[test]
fn plan_reclaim_stops_the_runtime_and_then_does_one_of_three_things() {
    let world = World::new();

    let params = world.reclaim_params();
    let trashing = reclaim::plan(&params).unwrap();
    assert_eq!(trashing.kind, reclaim::KIND);
    assert_eq!(trashing.recovery, Recovery::RollBack);
    assert_eq!(trashing.keys(), ["runtime.stop", "home.trash"]);
    assert_rebuilds_the_same(&world, &trashing, reclaim::KIND);

    let mut params = world.reclaim_params();
    params.entry = None;
    assert_eq!(
        reclaim::plan(&params).unwrap().keys(),
        ["runtime.stop"],
        "a managed home that has already gone leaves only the runtime to stop"
    );

    let mut params = world.reclaim_params();
    params.entry = None;
    params.environment.managed = false;
    assert_eq!(
        reclaim::plan(&params).unwrap().keys(),
        ["runtime.stop", "home.unadopt"],
        "a checkout adopted in place is given back rather than moved"
    );
}

/// The plan the journal rebuilds is the plan that was journalled.
///
/// This is the `params` serde round trip: the record holds the plan's own input as
/// JSON, and a build that reads it back has to arrive at the same steps in the same
/// order, or an interrupted run is undone by the wrong plan.
fn assert_rebuilds_the_same(world: &World, plan: &Plan, kind: &str) {
    let record = journal_of(world, plan);
    let rebuilt = ops::rebuilders()
        .iter()
        .find(|entry| entry.kind() == kind)
        .expect("this build knows how to rebuild this operation")
        .rebuild(&record)
        .unwrap();
    assert_eq!(rebuilt.keys(), plan.keys(), "a rebuilt plan lines up with the run's journal");
    assert_eq!(rebuilt.kind, plan.kind);
    assert_eq!(rebuilt.subject, plan.subject);
}

// ---------------------------------------------------------------------------
// Run to completion against kill and resume.
// ---------------------------------------------------------------------------

#[test]
fn resume_merge_writes_the_rows_and_events_a_first_run_wrote() {
    // Killed after the fetch, the commit of the work and the record of the branch tip,
    // so the squash, the rebase and the forward are what the next invocation applies.
    let finished = World::new().run_merge_to_completion();
    let resumed = World::new().resume_merge_after(3);
    assert_eq!(finished, resumed, "a resumed merge records the same merge");
}

#[test]
fn resume_adopt_writes_the_rows_and_events_a_first_run_wrote() {
    // An adopt of a checkout that stays where it is has no relocation step, so it has
    // no output for the commit to lose. A materialised adopt carries the same
    // relocation sink as a create, and the create's reproduction below covers it.
    let finished = World::new().run_adopt_to_completion();
    let resumed = World::new().resume_adopt_after(2);
    assert_eq!(finished, resumed, "a resumed adopt records the same unit");
}

#[test]
fn resume_new_writes_the_relocation_event_a_first_run_wrote() {
    // Killed straight after `home.relocate`, which is the step whose report the commit
    // reads. The next invocation skips it, so the report is never made a second time —
    // and the commit reads it out of the journal instead, which is why the two agree.
    let finished = World::new().run_new_to_completion();
    let resumed = World::new().resume_new_after(2);
    assert_eq!(finished, resumed, "a resumed create writes the relocation event");
}

/// The reclaim half of the same property, which a comparison between two runs can no
/// longer state.
///
/// It could while the value lived in memory: a first run had it and a resumed run did
/// not, and the two snapshots differed. Both now read it from the same journal column,
/// so the two runs agree by construction and the comparison asserts nothing. What is
/// left to assert is the substance — that the commit acts on what the teardown found —
/// and that takes a pair of teardowns rather than a pair of runs.
/// The registry write of a merge asks no repository.
///
/// It runs inside the registry's one `IMMEDIATE` transaction, which every `nodal` on
/// the machine queues behind, so a `git rev-parse` in it holds that lock across a
/// process spawn — and answers for a home that may not be there any more. The step that
/// moved the target read the home while nothing was waiting on it, and the write
/// records what that step found.
#[test]
fn the_registry_write_of_a_merge_reads_its_step_rather_than_the_home() {
    let finished = World::new().run_merge_to_completion();
    let without_a_home = World::new().resume_merge_with_the_home_removed();
    assert_eq!(
        finished.units, without_a_home.units,
        "the merge that happened is recorded as one, with no home left to ask"
    );
}

#[test]
fn resume_reclaim_keeps_the_session_row_of_a_group_it_could_not_stop() {
    let survived = World::new().resume_reclaim_with(support::surviving_teardown());
    assert!(
        survived.has_open_session(),
        "the row of a group that is still running is the only record `nodal gc` has of \
         it, and stays open: {survived:?}"
    );

    let stopped = World::new().resume_reclaim_with(reclaim::Teardown::default());
    assert!(
        !stopped.has_open_session(),
        "a group the teardown stopped keeps no claim on the unit: {stopped:?}"
    );
}
