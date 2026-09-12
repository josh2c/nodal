//! Hook process ownership: nothing a recipe hook starts becomes invisible.
//!
//! A hook is a command line in a project's recipe, and Nodal runs it through a shell
//! ([`nodal_core::lifecycle::hooks`]). A hook that finishes leaves nothing behind, and
//! almost all of them do. A hook that deliberately backgrounds work leaves a process
//! that outlives the command a person typed, and that process is the subject here.
//!
//! Every other way of finding such a process fails on purpose-built input:
//!
//! - the `NODAL_*` variables do not find it, because a descendant may run under `env -i`;
//! - the working directory does not find it, because a descendant may change it;
//! - neither signal exists at all on a host with no readable process table
//!   ([`nodal_core::runtime::processes`]), which is every macOS machine Nodal ships for.
//!
//! So the hook's shell is started in a process group of its own, and a group that is
//! still running when the shell exits is written into the registry as the unit's. The
//! row is the same one `nodal run --tether` writes, which is why `nodal reclaim` and
//! `nodal gc` already stop it: a signal to a process group is answered on every host,
//! whatever the process table does.
//!
//! The obligation runs the other way too. A group that cannot be written down is
//! stopped before the command returns, because the alternative is a lifecycle operation
//! reporting a clean result around a process nothing on the machine can name. Three
//! cases reach it: `pre_new`, which runs before the unit has any rows for a group to
//! belong to; a hook that exited non-zero, whose operation is about to be refused; and a
//! storage failure after the shell has already started.
//!
//! Nothing in this file reads `/proc`, and nothing in it is skipped on a host without
//! one. Every claim is made through `kill` on a plain process id, which both supported
//! platforms answer, because a claim that only held on Linux would be no claim at all
//! for the platform this property matters most on.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_core::model::{EnvId, Session};
use nodal_core::store::{environments, projects, sessions, units};
use nodal_safety::process::{alive, in_a_group_of_its_own, wait_for};
use nodal_safety::{InState as _, Machine, answer, git, stderr};

/// The unit every test here makes.
const UNIT: &str = "worker-import";

/// The prefix the actor of a hook's own session row carries.
const HOOK_ACTOR: &str = "hook:";

/// A shell line that backgrounds a process nothing but a process group can find, and
/// writes that process's identifier where the test can read it.
///
/// Three things make it invisible to everything else. It runs under `env -i`, so it
/// carries none of the `NODAL_*` variables attribution reads. It changes to the root
/// directory, so it stands in no home. And it is a generation below the shell Nodal
/// waited for, which is the shape of every development server: the process that was
/// started is long gone, and what is running is something it started.
///
/// The whole background list is redirected, not just the sleep. A backgrounded command
/// inherits the pipes Nodal reads the hook's output from, and one that kept them open
/// would hold the hook open for as long as it ran — which is today's behaviour and is
/// not what this suite is about.
fn backgrounds(record: &Path) -> String {
    format!(
        "( cd / && exec env -i sleep 300 ) >/dev/null 2>&1 & printf %s \"$!\" > {}",
        record.display()
    )
}

/// The fixture project with `hooks` declared in its recipe and approved on this machine.
///
/// Committed, because the recipe is a tracked file of the project and a home cloned from
/// a dirty tree is a home the uniqueness check has something to say about. Approved
/// through `nodal init`, because an unapproved command is refused before it runs and
/// every property here is about one that ran.
fn machine_declaring(hooks: &str) -> Machine {
    let machine = Machine::new();
    let path = machine.source.join(nodal_fixture::RECIPE);
    let recipe = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{recipe}\n[hooks]\n{hooks}\n")).unwrap();
    git(&machine.source, &["add", "--", nodal_fixture::RECIPE]);
    git(&machine.source, &["commit", "--quiet", "--message", "declare the project's hooks"]);
    let approved = machine.nodal(&["init", "--force"]);
    assert!(approved.status.success(), "the hooks were not approved: {}", stderr(&approved));
    machine
}

/// One phase declared as a TOML literal string, so the shell line reaches the shell as
/// it was written. A basic string would need every backslash and quote escaped twice.
fn phase(name: &str, line: &str) -> String {
    assert!(!line.contains('\''), "a literal TOML string cannot hold a single quote: {line}");
    format!("{name} = '{line}'")
}

/// The process the hook backgrounded, read from the file it wrote.
fn recorded(record: &Path) -> u32 {
    let text = std::fs::read_to_string(record)
        .unwrap_or_else(|error| panic!("the hook wrote no process id: {error}"));
    text.trim().parse().unwrap_or_else(|_| panic!("the hook wrote {text:?}, not a process id"))
}

/// The materialisation of the one unit this machine has.
fn environment(machine: &Machine) -> EnvId {
    let store = machine.store();
    let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
    let unit = units::list(store.conn(), project.id)
        .unwrap()
        .into_iter()
        .find(|unit| unit.slug.as_str() == UNIT)
        .expect("the unit was made");
    environments::latest_for_unit(store.conn(), unit.id).unwrap().expect("it has a home").id
}

/// Every open row of the unit that records a process group.
fn open_groups(machine: &Machine) -> Vec<Session> {
    let environment = environment(machine);
    sessions::list_open_tethers(machine.store().conn(), environment).unwrap()
}

/// Somewhere outside every home and every project for a hook to write to.
///
/// Outside, because a file written inside a home is untracked work: the uniqueness check
/// would refuse the reclaim, and the test would be asserting that check rather than the
/// property it is named for.
fn outside() -> tempfile::TempDir {
    tempfile::TempDir::new().unwrap()
}

/// **The central property.** A hook may background a process that nothing else on the
/// machine can name, and `nodal reclaim` still stops it.
///
/// This is the test that fails on a Nodal that does not record hook groups, and it fails
/// on the last line: the process is still running after the unit it belongs to has been
/// reclaimed. Everything before that line holds either way.
#[test]
fn a_process_a_hook_hid_from_every_other_signal_is_stopped_by_the_reclaim() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("post_new", &backgrounds(&record)));

    let home = machine.unit(UNIT);
    let hidden = recorded(&record);
    assert!(alive(hidden), "the hook backgrounded nothing, so there is nothing to assert about");

    let reclaimed = machine.nodal(&["reclaim", UNIT]);

    assert!(reclaimed.status.success(), "{}", stderr(&reclaimed));
    assert!(!home.exists(), "the reclaim left the home where it was");
    wait_for("the process the hook left behind to be stopped", || !alive(hidden));
}

/// The same group is a row, and the row is what makes the stop possible.
///
/// Kept apart from the property above so that the property fails for the reason it is
/// about. This one says how: one open session of the unit, recording a process group,
/// named for the phase that left it.
#[test]
fn a_surviving_hook_group_is_recorded_against_the_unit_and_names_its_phase() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("post_new", &backgrounds(&record)));

    drop(machine.unit(UNIT));

    let open = open_groups(&machine);
    assert_eq!(open.len(), 1, "the unit holds one recorded group: {open:?}");
    let group = open[0].pgid.expect("the row records a process group");
    assert!(group > 1, "the row records a reserved group identifier");
    assert_eq!(
        open[0].actor.name.as_str(),
        format!("{HOOK_ACTOR}post_new"),
        "the row does not say which hook left the group",
    );
    // And the group is the one the process is in, which is what makes the row worth
    // having: the process was started by the shell the group leader was.
    assert!(alive(recorded(&record)), "the recorded group holds nothing");
    drop(machine.nodal(&["reclaim", UNIT]));
}

/// An ordinary hook is untouched. It runs, it finishes, and it leaves no open group.
///
/// Without this the suite would pass on a Nodal that recorded a group for every hook,
/// which would put a row nothing can ever close against every unit anybody creates.
#[test]
fn a_hook_that_leaves_nothing_running_records_no_open_group() {
    let out = outside();
    let witness = out.path().join("ran");
    let line = format!("printf ran > {}", witness.display());
    let machine = machine_declaring(&phase("post_new", &line));

    drop(machine.unit(UNIT));

    assert_eq!(std::fs::read_to_string(&witness).unwrap(), "ran", "the hook did not run");
    assert!(open_groups(&machine).is_empty(), "a synchronous hook was recorded as a group");
}

/// A hook that fails leaves no child alive.
///
/// The operation is about to be refused, and a refusal that left a process running would
/// be the worst of the three outcomes: the caller is told nothing happened, and something
/// is still running. So the group is stopped, and the error is still the one the hook's
/// exit code earns, with the words the hook wrote.
#[test]
fn a_failed_hook_leaves_no_child_alive_and_still_reports_its_own_failure() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let line = format!("{}; echo the hook said no >&2; exit 3", backgrounds(&record));
    let machine = machine_declaring(&phase("post_new", &line));

    let made = machine.nodal(&["new", "--name", UNIT]);

    assert!(!made.status.success(), "a hook that exited 3 was reported as success");
    let told = stderr(&made);
    assert!(told.contains("post_new"), "the error does not name the hook: {told}");
    assert!(told.contains("exited 3"), "the error does not name the exit code: {told}");
    assert!(told.contains("the hook said no"), "the error dropped what the hook wrote: {told}");
    wait_for("the child of a failed hook to be stopped", || !alive(recorded(&record)));
    assert!(open_groups(&machine).is_empty(), "a failed hook's group was recorded");
}

/// A group that cannot be written down is stopped, and the reason is the answer.
///
/// `pre_new` is the phase this is about: it runs before the unit has any rows, so there
/// is no materialisation for a session to belong to and no later command that would ever
/// read one. Nodal will not leave an unrecorded group running in the hope that something
/// discovers it, so it stops the group and says why — which is the same obligation a
/// registry that refused the write would put it under.
#[test]
fn a_group_no_row_can_hold_is_stopped_and_the_reason_is_reported() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("pre_new", &backgrounds(&record)));

    let made = machine.nodal(&["new", "--name", UNIT]);

    assert!(!made.status.success(), "a create that lost a process reported success");
    let told = stderr(&made);
    assert!(told.contains("pre_new"), "the refusal does not name the hook: {told}");
    assert!(told.contains("could not record"), "the refusal does not say what went wrong: {told}");
    assert!(told.contains("the group was stopped"), "the refusal does not say what it did: {told}");
    wait_for("the group nothing could record to be stopped", || !alive(recorded(&record)));
}

/// The teardown reads the groups after `pre_reclaim` has run, so the hook's own group is
/// one of them.
///
/// A list taken before the hook would journal a teardown that does not stop it, and the
/// registry write that follows would close the row — leaving exactly the unowned process
/// this whole property rules out, produced by the command that exists to remove it.
#[test]
fn a_group_left_by_pre_reclaim_is_stopped_by_the_same_reclaim() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("pre_reclaim", &backgrounds(&record)));
    let home = machine.unit(UNIT);

    let reclaimed = machine.nodal(&["reclaim", UNIT]);

    assert!(reclaimed.status.success(), "{}", stderr(&reclaimed));
    assert!(!home.exists(), "the reclaim left the home where it was");
    wait_for("the group pre_reclaim left to be stopped", || !alive(recorded(&record)));
}

/// A group left by `post_reclaim` outlives the command, is reported rather than hidden,
/// and is what `nodal gc` stops.
///
/// `post_reclaim` runs after the unit's rows are closed, so its group cannot be stopped
/// by the reclaim that started it — the teardown is over. The answer is not to call the
/// operation clean: the group is recorded against the reclaimed materialisation, the
/// verification reports a session that is still open, and the sweep whose whole scope is
/// reclaimed materialisations stops it.
#[test]
fn a_group_left_by_post_reclaim_is_reported_and_then_stopped_by_the_sweep() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("post_reclaim", &backgrounds(&record)));
    drop(machine.unit(UNIT));

    let reclaimed = machine.nodal(&["reclaim", UNIT]);

    let told = answer(&reclaimed);
    assert!(!reclaimed.status.success(), "a reclaim that left a process running claimed clean");
    assert!(told.contains("session"), "the report does not say what is left: {told}");
    let hidden = recorded(&record);
    assert!(alive(hidden), "there is nothing left for the sweep to do");

    drop(machine.nodal(&["gc"]));

    wait_for("the sweep to stop the group post_reclaim left", || !alive(hidden));
}

/// A process group nothing recorded is never signalled, however close it stands.
///
/// Recording hook groups widens what a teardown signals, and this is the fence around
/// that. The bystander is in a process group of its own, exactly like a recorded one, and
/// the only difference is that no row holds its identifier. It survives the create, the
/// reclaim and the sweep.
#[test]
fn a_group_no_row_holds_is_never_signalled() {
    let out = outside();
    let record = out.path().join("backgrounded");
    let machine = machine_declaring(&phase("post_new", &backgrounds(&record)));
    let bystander = in_a_group_of_its_own();

    drop(machine.unit(UNIT));
    drop(machine.nodal(&["reclaim", UNIT]));
    drop(machine.nodal(&["gc"]));

    assert!(alive(bystander.pid()), "a group no row holds was signalled");
    wait_for("the recorded group to be stopped", || !alive(recorded(&record)));
}
