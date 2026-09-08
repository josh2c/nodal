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
//! ## How the two are gated
//!
//! They are expected failures, not skipped tests. Each runs in full on every run, and
//! [`expected_failure`] decides what its outcome means:
//!
//! - the divergence is still there: the reproduction is printed and the test passes, so
//!   a red suite does not hide a real regression behind a known bug;
//! - the divergence has gone: the test **fails**, and says to delete the gate. A fix
//!   that lands without anyone noticing is a fix nothing protects afterwards.
//!
//! Setting `NODAL_ENFORCE_STEP_OUTPUTS=1` inverts that: the divergence fails and
//! equality passes. That is the one edit CI needs the day the step-output change lands,
//! and `ci/acceptance-behaviour-lock.sh` runs both ways round.
//!
//! ## What is *not* claimed here, and what the two locks are for
//!
//! Neither divergence damages data today. All four operations recover by
//! [`Recovery::RollBack`], so no invocation of `nodal` reaches the resume path for
//! them: a killed create or reclaim is undone, not finished, and its commit never runs
//! a second time. The one operation that does resume is the base build, which carries
//! no sink at all, so it has no step output for a rebuild to lose. Both divergences are
//! therefore latent. They are reproduced here by journalling the run as
//! [`Recovery::Resume`], which is the single line that separates the two paths.
//!
//! That is what these two locks are for. Journalled step outputs are the **precondition
//! for ever granting [`Recovery::Resume`] to a lifecycle operation**, not a repair of
//! live damage. Any operation given that recovery mode before the outputs reach the
//! journal starts losing the values its commit reads, silently, on exactly the runs a
//! person cannot watch. The two tests below are the gate on that order of work: while
//! they still reproduce, no lifecycle operation may be moved to `Resume`.
//!
//! The per-machine secrets file activation reads is `secrets.env` in the state
//! directory, and every fixture here has a temporary one, so no test in this file reads
//! or writes the file belonging to whoever is running it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod support;

use nodal_core::lifecycle::ops::{adopt, merge, new, reclaim};
use nodal_core::lifecycle::{Plan, Recovery, ops};

use support::{World, expected_failure, journal_of};

/// What the step-output change is called where a comment has to name it.
const STEP_OUTPUTS: &str = "the journalled step-output column";

// ---------------------------------------------------------------------------
// The plans.
// ---------------------------------------------------------------------------

#[test]
fn plan_new_has_seven_steps_and_rolls_back() {
    let world = World::new();
    let params = world.create_params();
    let plan = new::plan(&params, &support::sink()).unwrap();
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
    let in_place = adopt::plan(&params, &support::sink()).unwrap();
    assert_eq!(in_place.kind, adopt::KIND);
    assert_eq!(in_place.recovery, Recovery::RollBack);
    assert_eq!(in_place.keys(), ["git.hide", "home.marker", "env.activate"]);
    assert_rebuilds_the_same(&world, &in_place, adopt::KIND);

    let params = world.adopt_params();
    let materialized = adopt::plan(&params, &support::sink()).unwrap();
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
    let sink = std::sync::Arc::new(std::sync::OnceLock::new());

    let params = world.merge_params(merge::Stages::all(), false);
    let whole = merge::plan(&params, &sink).unwrap();
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
        merge::plan(&params, &sink).unwrap().keys(),
        ["target.fetch", "work.commit", "branch.rebase", "target.forward"],
        "dropping the squash drops the record of what preceded it too"
    );

    let params = world.merge_params(merge::Stages::all().without(merge::Stage::Commit), false);
    assert_eq!(
        merge::plan(&params, &sink).unwrap().keys(),
        ["target.fetch", "premerge.record", "branch.squash", "branch.rebase", "target.forward"]
    );

    let params = world.merge_params(merge::Stages::all(), true);
    assert_eq!(
        merge::plan(&params, &sink).unwrap().keys(),
        ["target.fetch", "branch.rebase", "target.forward"],
        "a run that continues a stopped rebase rewrites nothing a second time"
    );

    let params = world.merge_params(merge::Stages::default(), false);
    assert_eq!(
        merge::plan(&params, &sink).unwrap().keys(),
        ["target.fetch", "target.forward"],
        "with no stage to run the plan still fetches the target and forwards it"
    );
}

#[test]
fn plan_reclaim_stops_the_runtime_and_then_does_one_of_three_things() {
    let world = World::new();
    let teardown = std::sync::Arc::new(std::sync::OnceLock::new());
    let released = std::sync::Arc::new(std::sync::OnceLock::new());

    let params = world.reclaim_params();
    let trashing = reclaim::plan(&params, &teardown, &released).unwrap();
    assert_eq!(trashing.kind, reclaim::KIND);
    assert_eq!(trashing.recovery, Recovery::RollBack);
    assert_eq!(trashing.keys(), ["runtime.stop", "home.trash"]);
    assert_rebuilds_the_same(&world, &trashing, reclaim::KIND);

    let mut params = world.reclaim_params();
    params.entry = None;
    assert_eq!(
        reclaim::plan(&params, &teardown, &released).unwrap().keys(),
        ["runtime.stop"],
        "a managed home that has already gone leaves only the runtime to stop"
    );

    let mut params = world.reclaim_params();
    params.entry = None;
    params.environment.managed = false;
    assert_eq!(
        reclaim::plan(&params, &teardown, &released).unwrap().keys(),
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
    // reads. The next invocation skips it, so the report is never made a second time.
    let finished = World::new().run_new_to_completion();
    let resumed = World::new().resume_new_after(2);
    expected_failure(
        "a resumed create writes no relocation event",
        STEP_OUTPUTS,
        &finished,
        &resumed,
    );
}

#[test]
fn resume_reclaim_keeps_the_session_row_of_a_group_it_could_not_stop() {
    // Killed straight after `runtime.stop`, which is the step whose teardown the commit
    // reads. The next invocation skips it, so the commit asks an empty lock.
    let finished = World::new().run_reclaim_to_completion();
    let resumed = World::new().resume_reclaim_after(1);
    expected_failure(
        "a resumed reclaim closes the session row of a group that is still running",
        STEP_OUTPUTS,
        &finished,
        &resumed,
    );
}
