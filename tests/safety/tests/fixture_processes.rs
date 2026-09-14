//! What a fixture leaves behind, which must be nothing.
//!
//! These are the properties of [`nodal_safety::process::Owned`], asserted against real
//! processes. The first one is the leak the owner was written for, made small enough to
//! read: a leader killed by itself, the way a fixture with a handle on one process kills it,
//! and a parked descendant still running afterwards. It is the only test here that lets a
//! process escape, and it hands the group to the owner on the next line. Every other one
//! asserts a shape that cannot leak — on a normal return, on a failed assertion, on a
//! deadline, on a panic, and over a process the fixture adopted rather than started.
//!
//! Zero survivors is what is asserted, never the order of the calls that get there. A test
//! that checked "cleanup ran" would pass against a cleanup that reached nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use nodal_safety::process::{self, Owned};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// The fixture the leak had.
// ---------------------------------------------------------------------------

/// A leader that starts one grandchild in its group, running `inner`, and waits for it.
///
/// The group is two generations deep on purpose: that is the shape a handle on one process
/// does not reach, and the shape every fixture in this workspace that starts anything has.
/// It runs in a temporary tree of its own, and the grandchild writes its own identifier
/// outside that tree — a test that read the record from inside it could not ask what
/// happened after the tree was removed.
fn group_of(records: &Path, inner: &str) -> Owned {
    let child = records.join("child");
    let line =
        format!("sh -c 'printf %s \"$$\" > {child}; {inner}' & wait", child = child.display());
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    Owned::in_tree(TempDir::new().unwrap(), &mut command)
}

/// The fixture the leak had: a grandchild parked in the loop the real build script parks in,
/// forking one `sleep` a second, which is what made the leak a growing one rather than a
/// still one.
fn parked(records: &Path) -> Owned {
    group_of(records, "while : ; do sleep 1; done")
}

/// The same, with a grandchild that starts nothing of its own — so the group is exactly two
/// processes and a test can say which of them was signalled.
fn steady(records: &Path) -> Owned {
    group_of(records, "sleep 30")
}

/// The identifier the parked descendant wrote, once it has written one.
fn descendant(records: &Path) -> u32 {
    let child = records.join("child");
    process::until("the fixture's descendant to say which process it is", || {
        std::fs::read_to_string(&child).ok()?.trim().parse().ok()
    })
}

/// The reproduction, and the fix, in one test.
///
/// The leader is signalled by itself, which is what the substrate fixture did: a handle on
/// a child is a handle on one process, and the family it started is not in it. The parked
/// descendant is still running afterwards — that is the property failing, and it fails only
/// because the owner has not exited yet. The owner then takes the group, and the group is
/// the whole family.
#[test]
fn killing_the_leader_alone_leaves_the_parked_descendant_and_the_owner_takes_the_group() {
    let records = TempDir::new().unwrap();
    let mut owner = parked(records.path());
    let parked_pid = descendant(records.path());

    // What the fixture used to do: one signal, to one process, by the number it held.
    assert!(
        Command::new("kill").args(["-9", &owner.pid().to_string()]).status().unwrap().success(),
        "the leader was signalled"
    );
    process::wait_for("the leader to go", || owner.exited());
    assert!(
        process::alive(parked_pid),
        "the leak this sprint is about did not reproduce: the descendant went with its parent"
    );

    owner.reclaim();

    assert!(!process::alive(parked_pid), "the parked descendant outlived the owner");
    assert_eq!(owner.survivors(), Vec::<u32>::new(), "the group still holds something");
    assert!(!owner.running(), "the group is still there");
}

/// A process that joined the group after its leader was reaped is not the owner's to signal.
///
/// Killing the leader by itself gives its number back, and the group's number with it: what
/// is left in the group holds it now, and when the last of them exits the kernel is free to
/// give it to somebody else's leader. An owner that went on naming the group would then
/// signal that stranger — in this binary, where another test's fixture is the likeliest
/// stranger there is.
///
/// A test cannot make the kernel hand out a number it chooses, so the stranger is put into
/// the group instead. It is the same mistake seen from the other side: a process standing in
/// the number the owner used to name, which the owner never took. It is started by a shell
/// that exits, so it is nobody's child, exactly as the leader of a reissued number would be.
#[cfg(unix)]
#[test]
fn a_process_that_joined_the_group_after_the_leader_was_reaped_is_spared() {
    let records = TempDir::new().unwrap();
    let mut owner = steady(records.path());
    let taken = descendant(records.path());

    owner.kill_the_leader_alone();
    let stranger = joined(owner.pid());

    owner.reclaim();

    assert!(!process::alive(taken), "the process the owner took was left running");
    assert!(process::alive(stranger.pid()), "the owner signalled a process it never took");
}

/// A sleeping process in an existing process group, owned by number because the shell that
/// started it has exited and it is nobody's child.
#[cfg(unix)]
fn joined(group: u32) -> Owned {
    use std::os::unix::process::CommandExt as _;

    let mut command = Command::new("sh");
    command.arg("-c").arg("sleep 30 >/dev/null 2>&1 & printf %s \"$!\"");
    command.process_group(i32::try_from(group).expect("a process group identifier"));
    let output = command.output().expect("the shell runs");
    let pid: u32 =
        String::from_utf8_lossy(&output.stdout).trim().parse().expect("the shell wrote a pid");
    assert!(process::alive(pid), "the stranger is running");
    Owned::adopt(pid)
}

// ---------------------------------------------------------------------------
// The tree.
// ---------------------------------------------------------------------------

/// A temporary tree is removed after the last process using it has gone, never before.
///
/// The order is not asserted by watching the calls. It is asserted by what is on the disk
/// at two moments a test can see: the tree is still there when the group has gone, and it
/// is gone when the owner is.
#[test]
fn a_tree_outlives_the_group_that_stands_in_it() {
    let records = TempDir::new().unwrap();
    let mut owner = parked(records.path());
    let parked_pid = descendant(records.path());
    let tree = owner.tree().to_path_buf();
    assert!(tree.is_dir(), "the fixture was given a tree");

    owner.reclaim();

    assert!(!process::alive(parked_pid), "the descendant is gone");
    assert!(tree.is_dir(), "the tree was removed while the group was still standing in it");

    drop(owner);
    assert!(!tree.exists(), "the tree outlived the owner");
}

/// A fixture that unwinds takes its group first and its tree after it.
///
/// This is the case the leak was hiding in. A fixture returns normally on a good day; on
/// the day the assertion fails it unwinds, and that is the day nobody was watching what it
/// left behind.
#[test]
fn a_fixture_that_unwinds_leaves_no_process_and_no_tree() {
    let records = TempDir::new().unwrap();
    let mut seen = (0, PathBuf::new());

    let failed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let owner = parked(records.path());
        seen = (descendant(records.path()), owner.tree().to_path_buf());
        panic!("the assertion this fixture was written for");
    }));

    assert!(failed.is_err(), "the fixture did not fail");
    let (parked_pid, tree) = seen;
    process::wait_for("the unwound fixture's descendant to go", || !process::alive(parked_pid));
    assert!(!tree.exists(), "the unwound fixture left its tree");
}

/// A deadline takes the whole group, and not only the command the deadline was about.
///
/// [`process::within`] is the bounded runner: a command that has not finished is a named
/// failure rather than a suite that hangs. What it kills is the group, so a command that
/// backgrounded something before it hung leaves nothing either.
#[test]
fn a_deadline_takes_the_whole_group() {
    let records = TempDir::new().unwrap();
    let child = records.path().join("child");
    let line = format!(
        "sh -c 'printf %s \"$$\" > {child}; while : ; do sleep 1; done' & wait",
        child = child.display()
    );
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);

    let failed = std::panic::catch_unwind(AssertUnwindSafe(|| {
        process::within(&mut command, Duration::from_millis(300));
    }));

    assert!(failed.is_err(), "a command that never finishes was called finished");
    let parked_pid: u32 =
        process::until("the deadline's descendant to have said which process it is", || {
            std::fs::read_to_string(&child).ok()?.trim().parse().ok()
        });
    process::wait_for("the deadline's descendant to go", || !process::alive(parked_pid));
}

// ---------------------------------------------------------------------------
// What an owner does not reach, and what it does twice.
// ---------------------------------------------------------------------------

/// A process in a group of its own is not reached by an owner of another group.
///
/// The bystander is the whole point of owning a group rather than sweeping a machine. It is
/// a `sleep` like the owned one, started at the same moment, in a group nothing recorded.
#[test]
fn a_bystander_in_a_group_of_its_own_is_untouched() {
    let records = TempDir::new().unwrap();
    let bystander = process::in_a_group_of_its_own();
    let mut owner = parked(records.path());
    let parked_pid = descendant(records.path());

    owner.reclaim();

    assert!(!process::alive(parked_pid), "the owned group is gone");
    assert!(process::alive(bystander.pid()), "an owner signalled a group it does not own");
}

/// Reclaiming twice is harmless, and so is reclaiming a process that had already gone.
#[test]
fn reclaiming_twice_is_harmless() {
    let records = TempDir::new().unwrap();
    let mut owner = parked(records.path());
    let parked_pid = descendant(records.path());

    owner.reclaim();
    owner.reclaim();

    assert!(!process::alive(parked_pid), "the group is gone");
    assert_eq!(owner.survivors(), Vec::<u32>::new(), "and it stayed gone");
}

/// A child that exited by itself is reaped.
///
/// An unreaped child is a zombie, and a zombie is something the fixture left behind: it
/// holds an entry in the process table and it answers `kill -0` as a living process does.
/// So "it exited" is not the end of the owner's work.
#[test]
fn a_child_that_exited_by_itself_is_reaped() {
    let mut command = Command::new("sh");
    command.arg("-c").arg("exit 0");
    let mut owner = Owned::spawn(&mut command);
    let pid = owner.pid();
    process::wait_for("the child to exit", || owner.exited());

    owner.reclaim();

    assert!(!process::alive(pid), "the exited child was left unreaped");
}

// ---------------------------------------------------------------------------
// A process the fixture did not start.
// ---------------------------------------------------------------------------

/// A process a fixture adopted is stopped by the number the fixture recorded.
///
/// This is the backstop over what the product backgrounds. The shell that starts it is gone
/// before the fixture sees the number, which is the shape of a tether and of what a recipe
/// hook leaves.
#[test]
fn an_adopted_process_is_stopped() {
    let output = Command::new("sh")
        .arg("-c")
        .arg("sleep 300 >/dev/null 2>&1 & printf %s \"$!\"")
        .output()
        .unwrap();
    let pid: u32 = String::from_utf8_lossy(&output.stdout).trim().parse().unwrap();
    assert!(process::alive(pid), "the backgrounded process is running");

    let mut owner = Owned::adopt(pid);
    owner.reclaim();

    assert!(!process::alive(pid), "the adopted process outlived the fixture that adopted it");
}

/// A target a fixture may not signal is left running, and the test fails saying so.
///
/// This is the rule that keeps the owner from becoming the thing it was written against. An
/// owner that cannot prove the number it holds — because the host will not say what the
/// process is, or because the reading has changed and the number has been given to somebody
/// else — signals nothing at all. The process stays visible and the test fails.
///
/// The target here is this test process itself, which is the one number that is certainly
/// not the fixture's to signal. A reading that no longer matches takes the same path and
/// ends with the same refusal.
#[test]
fn a_target_this_test_may_not_signal_is_left_running_and_the_test_fails() {
    let mut owner = Owned::adopt(std::process::id());

    let refused = std::panic::catch_unwind(AssertUnwindSafe(|| owner.reclaim()));

    let said = refused.expect_err("an owner signalled a target it may not signal");
    let told = said.downcast_ref::<String>().expect("the refusal says what it refused");
    assert!(told.contains("nothing was signalled"), "{told}");
    assert!(process::alive(std::process::id()), "the owner signalled this test process");
}

// ---------------------------------------------------------------------------
// The marker the sentinel counts.
// ---------------------------------------------------------------------------

/// Every process an owner starts carries the marker of this run.
///
/// The process is asked rather than the machine, so the claim holds on a host with no
/// process table. What reads the machine is `ci/acceptance-process-hygiene.sh`, and this is
/// what makes its reading mean something.
#[test]
fn every_process_an_owner_starts_carries_the_marker() {
    let records = TempDir::new().unwrap();
    let said = records.path().join("said");
    let line = format!("printf %s \"${}\" > {}; sleep 30", process::MARKER, said.display());
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    let mut owner = Owned::spawn(&mut command);

    let carried = process::until("the owned process to say what it carries", || {
        let text = std::fs::read_to_string(&said).ok()?;
        (!text.is_empty()).then_some(text)
    });

    assert_eq!(carried, process::token(), "an owned process carries another run's marker");
    owner.reclaim();
    assert!(!process::alive(owner.pid()), "the group is gone");
}
