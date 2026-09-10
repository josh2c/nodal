//! Reclaim scope: only what carries the unit's identifier is signalled.
//!
//! A unit's home is a directory, and a directory is reachable by anything. A tmux pane
//! is left in one. An editor server over SSH holds one open. A teammate on a shared box
//! stands in one. None of the three says which unit it is working on, and a teardown
//! that signalled everything it found by working directory would kill all three.
//!
//! Attribution already has the two levels this file is about
//! ([`nodal_core::runtime::attribute::Confidence`]). Certain is a recorded tether or a
//! process carrying `NODAL_ID`, which Nodal wrote into the home's environment. Probable
//! is a process standing in the home and saying nothing else. This suite asserts that
//! `nodal reclaim` signals the first level and reports the second.
//!
//! Both levels are real processes, started against a real unit, because the property is
//! about what a signal reaches and a table a test wrote reaches nothing.
//!
//! The scan reads `/proc`, which macOS does not have, so the four tests that need it say
//! which claim they are not making rather than passing quietly. The one that does not is
//! the control: what it asserts is that a tether is stopped, and `kill` answers for a
//! process on every host.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nodal_core::model::EnvId;
use nodal_core::store::{environments, projects, sessions, units};
use nodal_safety::{InState as _, Machine, answer, platform, process, stderr};

/// The unit every test here reclaims.
const UNIT: &str = "worker-import";

/// How long a test waits for a signalled group to go.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How often a wait looks.
const POLL: Duration = Duration::from_millis(10);

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

/// Start a tether in the home, and answer with the process it left behind.
///
/// The tethered command starts a sleeping child, records its process id, and returns.
/// That is the shape of a development server: the process `nodal run` waited for is gone
/// long before anybody reclaims anything, and what is still running is a generation
/// below it. So the assertion is about that child, which is the process a signal to the
/// group has to reach.
///
/// The child is asked for its own id rather than looked up, and `record` is outside
/// every home. A file written inside the home would be untracked work, and the
/// uniqueness check would refuse the reclaim, which is that check doing its job and
/// nothing to do with this property.
fn tether(machine: &Machine, home: &Path, record: &Path) -> u32 {
    let line = format!("sleep 300 >/dev/null 2>&1 & printf %s \"$!\" > {}", record.display());
    let ran = machine
        .command(&["run", "--tether", "sh", "-c", &line])
        .current_dir(home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(ran.success(), "the tethered command ran");

    let open = sessions::list_open_tethers(machine.store().conn(), environment(machine)).unwrap();
    assert_eq!(open.len(), 1, "the unit holds one tether");
    open[0].pgid.expect("the row records a process group");

    let pid: u32 = std::fs::read_to_string(record).unwrap().trim().parse().unwrap();
    assert!(alive(pid), "the tethered command left a process behind");
    pid
}

/// Whether a process is still there.
///
/// `kill -0` on one process id, rather than `/proc` and rather than a process group. A
/// host with no process table still answers this, and `kill` is asked about a plain
/// positive number, which every implementation of it reads the same way. A negative
/// argument does not read the same way everywhere, so no test here passes one.
fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Wait for something to become true, and insist that it does.
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("{what} did not happen within {TIMEOUT:?}");
}

/// A process standing in the home is never signalled, and the refusal names it.
///
/// This is the whole property in one test. The tether is the certain level and it dies.
/// The bystander is the probable level: it goes on running, the reclaim names it by
/// command and process, and the home stays where it is.
#[test]
fn a_bystander_standing_in_the_home_survives_a_reclaim_and_is_named() {
    if !platform::reads_process_table("a reclaim reports what only stands in the home") {
        return;
    }
    let machine = Machine::new();
    let home = machine.unit(UNIT);
    let outside = tempfile::TempDir::new().unwrap();
    let tethered = tether(&machine, &home, &outside.path().join("tethered"));
    let bystander = process::standing_in(&home);

    let refused = machine.nodal(&["reclaim", UNIT]);

    assert!(!refused.status.success(), "the home moved out from under a live process");
    let told = stderr(&refused);
    assert!(told.contains(&format!("pid {}", bystander.pid())), "the refusal names no pid: {told}");
    assert!(told.contains("sleep"), "the refusal names no command: {told}");
    assert!(told.contains("--force"), "the refusal says nothing to do about it: {told}");

    assert!(alive(bystander.pid()), "a process carrying no unit id was signalled");
    assert!(home.is_dir(), "the refusal moved the home");
    assert_eq!(machine.homes(), vec![home.clone()], "the home is still the project's");
    assert!(machine.trashed().is_empty(), "the refusal put something in the trash");

    wait_for("the tether to go", || !alive(tethered));
}

/// The refusal is recoverable by the route it names, and by that route alone.
///
/// A reclaim that refuses over a bystander rolls back, so the unit is still live and
/// `--force` is an ordinary reclaim of it. That is the one thing a person does next.
#[test]
fn a_refused_reclaim_is_recoverable_by_the_route_the_refusal_names() {
    if !platform::reads_process_table("a refusal over a bystander is recoverable") {
        return;
    }
    let machine = Machine::new();
    let home = machine.unit(UNIT);
    let bystander = process::standing_in(&home);
    assert!(!machine.nodal(&["reclaim", UNIT]).status.success(), "it refuses first");

    let forced = machine.nodal(&["reclaim", UNIT, "--force"]);

    assert!(answer(&forced).contains("standing in the home"), "{}", stderr(&forced));
    assert!(!home.exists(), "the route the refusal names did not move the home");
    assert!(alive(bystander.pid()), "the forced reclaim signalled the bystander");
}

/// The same machine, forced: the home moves, and the bystander is still not signalled.
///
/// `--force` is about the directory and not about the process. A person who says "move
/// it anyway" has said what to do with their own home; they have not asked Nodal to kill
/// somebody's shell. So the report lists the process instead, in the words the contract
/// gives.
#[test]
fn a_forced_reclaim_moves_the_home_and_still_leaves_the_bystander_running() {
    if !platform::reads_process_table("a forced reclaim reports what it did not signal") {
        return;
    }
    let machine = Machine::new();
    let home = machine.unit(UNIT);
    let bystander = process::standing_in(&home);

    let forced = machine.nodal(&["reclaim", UNIT, "--force"]);

    // The report is read whatever the exit code, and the code is part of the property:
    // a process left standing is something the verification found, so a script that
    // reclaims a hundred units is told which one to look at.
    let told = answer(&forced);
    assert!(!forced.status.success(), "a reclaim that left a process running claimed clean");
    assert!(told.contains("standing in the home; not signalled"), "{told}");
    assert!(told.contains(&format!("pid {}", bystander.pid())), "{told}");
    assert!(alive(bystander.pid()), "a forced reclaim signalled a process carrying no unit id");
    assert!(!home.exists(), "the forced reclaim left the home where it was");
    assert_eq!(machine.trashed().len(), 1, "the trash holds the home");
}

/// A tether is stopped although nothing else is, so the refusal is about the level and
/// not about the command failing early.
///
/// Without this the first test would pass on a reclaim that did nothing at all.
#[test]
fn a_reclaim_with_no_bystander_stops_its_tether_and_takes_the_home() {
    let machine = Machine::new();
    let home = machine.unit(UNIT);
    let outside = tempfile::TempDir::new().unwrap();
    let tethered = tether(&machine, &home, &outside.path().join("tethered"));

    let reclaimed = machine.nodal(&["reclaim", UNIT]);

    assert!(reclaimed.status.success(), "{}", stderr(&reclaimed));
    wait_for("the tether to go", || !alive(tethered));
    assert!(!home.exists(), "the home is not where it was");
    assert_eq!(machine.trashed().len(), 1, "the trash holds it");
}

/// The sweep makes the same split. A process standing in a home that is gone is named
/// and left running.
///
/// The bystander's working directory is the trash path by now, because a home is moved
/// rather than deleted and the kernel follows the inode. So this is also what says the
/// sweep watches both names a reclaimed home has.
#[test]
fn a_sweep_reports_a_process_standing_in_a_reclaimed_home_and_signals_nothing() {
    if !platform::reads_process_table("a sweep reports what only stands in a reclaimed home") {
        return;
    }
    let machine = Machine::new();
    let home = machine.unit(UNIT);
    let bystander = process::standing_in(&home);
    assert!(!machine.nodal(&["reclaim", UNIT, "--force"]).status.success(), "it is a leftover");

    let swept = machine.nodal(&["gc"]);

    let told = answer(&swept);
    assert!(told.contains("standing in the home; not signalled"), "{told}");
    assert!(told.contains(&format!("pid {}", bystander.pid())), "{told}");
    assert!(alive(bystander.pid()), "the sweep signalled a process carrying no unit id");
    // The retention has not run out, so the sweep has nothing to remove and the home is
    // still in the trash. What the test is about is what the sweep did not signal.
    assert_eq!(machine.trashed().len(), 1, "{told}");
}
