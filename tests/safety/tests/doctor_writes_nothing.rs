//! `nodal doctor` reads this machine and leaves every part of it exactly as it was.
//!
//! Doctor is the command a person runs when a disk is full and they are already
//! worried. It reports the checkout's other worktrees, stale caches, orphan databases
//! and units, each with a size, and it removes nothing. A read command that wrote
//! would be worse than one that refused: the person ran it to find out what is there,
//! not to change it.
//!
//! "Nothing changed" is asserted byte for byte, on every tree doctor reads: the person's
//! checkout, a worktree of that checkout that lives beside it rather than inside it, and
//! Nodal's whole state directory with every home, base and trashed home in it. The
//! worktree outside the checkout is a tree of its own here for the same reason it is a
//! row of its own in the report: doctor reads it because the repository names it, so a
//! snapshot of the checkout would not be watching the directory doctor opened.
//!
//! The registry is the one thing left out, and it is left out for a stated reason rather
//! than to make a test pass. Two things about it are not doctor. Every command that opens
//! it first finishes or rolls back an operation an earlier run was killed in the middle
//! of, and that preamble writes. And SQLite makes `registry.db-wal` and `registry.db-shm`
//! beside it on every open and removes them on every close, which moves the state root's
//! change time whatever the command was. Both were measured, and both happen to a command
//! that only reads. So the registry's *rows* are read before and after instead, and they
//! have to be the same rows.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::store::{Store, environments, projects, units};
use nodal_core::workspace::sharing;
use nodal_safety::InState as _;
use nodal_safety::{Machine, Snapshot, git, stdout};

/// The worktree planted inside the checkout, so doctor has something to report.
const NESTED: &str = ".claude/worktrees/side";

/// A worktree of that same checkout planted beside it. This is the shape a real machine
/// had: registered by the repository, and not underneath it.
const BESIDE: &str = "project-beside";

/// A local branch of the checkout that no worktree has checked out. The branch audit
/// reads every ref, so the machine has to hold one for the audit to be exercised here.
const LEFT_BEHIND: &str = "left-behind";

/// The checkout, a worktree inside it and a worktree beside it, and the paths of the two
/// trees a snapshot has to watch.
fn plant(machine: &Machine) -> (PathBuf, PathBuf) {
    git(&machine.source, &["branch", LEFT_BEHIND]);
    git(&machine.source, &["worktree", "add", "--quiet", "--detach", NESTED]);
    let beside = machine.source.parent().expect("the machine root").join(BESIDE);
    git(&machine.source, &["worktree", "add", "--quiet", "--detach", beside.to_str().unwrap()]);
    (machine.source.clone(), beside)
}

/// Whether a path under the state directory is the registry, which every command writes.
fn is_registry(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("registry.db"))
}

/// Every row of the registry a survey could touch, as text.
fn rows(machine: &Machine) -> String {
    let store: Store = machine.store();
    let mut lines = Vec::new();
    for project in projects::list(store.conn()).unwrap() {
        lines.push(format!("{} {} {}", project.id, project.name, project.root.display()));
        for unit in units::list(store.conn(), project.id).unwrap() {
            lines.push(format!("  {} {} {:?}", unit.id, unit.slug, unit.status));
            for row in environments::list_for_unit(store.conn(), unit.id).unwrap() {
                lines.push(format!("    {} {} {:?}", row.id, row.home.display(), row.state));
            }
        }
    }
    lines.join("\n")
}

/// The change time of a directory, in whole nanoseconds, which moves when anything is
/// made in it or removed from it. A file written and removed again leaves no trace a
/// snapshot can see, so this is the reading that catches a probe.
///
/// Seconds are not enough. The change this catches happens in a few tens of
/// milliseconds, so a reading in seconds answers "unchanged" for every run that fits
/// inside one second and "changed" for the ones that straddle a boundary. That is a
/// coin, not an instrument.
fn changed_at(path: &Path) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt as _;

    let held = std::fs::metadata(path).expect("a readable directory");
    (held.ctime(), held.ctime_nsec())
}

/// What a probe leaves behind, anywhere under `root`.
///
/// The whole tree and not the top of it. A probe goes in the directory it is asking
/// about, and doctor reports on every base, home and trashed home under the state root.
fn probe_leftovers(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with(".nodal-sharing-") {
                found.push(path.clone());
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                pending.push(path);
            }
        }
    }
    found
}

/// Doctor asks no filesystem whether it shares blocks. It reads the record that was
/// written when the state root was made, so nothing is written into any directory it
/// reports on.
///
/// A snapshot cannot see this. A probe writes a file, clones it and removes both, and a
/// reading taken afterwards finds the directory holding exactly what it held before. The
/// change time is what the write moved, and the project's bases directory is where the
/// probe goes: nothing else writes there while a report is being made.
///
/// **The state root itself is not watched this way, and the reason is measured.** Every
/// command that opens the registry makes `registry.db-wal` and `registry.db-shm` beside
/// it and removes them again when the connection closes. Four directory entries come and
/// go, so the state root's change time moves on every run of every command, doctor
/// included. That is SQLite keeping its own book, not doctor writing, and it is the same
/// thing [`is_registry`] leaves out of the snapshots below for the same stated reason. A
/// reading that cannot tell a probe from a write-ahead log is not evidence about a
/// probe. What is evidence about the state root is
/// [`doctor_says_when_nothing_recorded_whether_the_state_root_shares_blocks`], which
/// removes the record and proves doctor does not take it again.
#[test]
fn doctor_asks_no_directory_whether_it_shares_blocks() {
    let machine = Machine::new();
    machine.unit("worker-import");
    let watched = [machine.segment("b").expect("the bases"), machine.source.clone()];

    let before: Vec<(i64, i64)> = watched.iter().map(|path| changed_at(path)).collect();
    let report = stdout(&machine.nodal(&["doctor"]));
    assert!(!report.is_empty(), "doctor reported nothing");

    for (path, was) in watched.iter().zip(before) {
        assert_eq!(changed_at(path), was, "doctor wrote in {}", path.display());
    }
    let left = probe_leftovers(&machine.state);
    assert!(left.is_empty(), "doctor left a probe's files under the state root: {left:?}");
}

/// The instrument the test above depends on. A change time read in whole seconds calls a
/// directory unchanged whenever the write it is watching for lands in the same second,
/// which is most of them, so the reading has to carry nanoseconds.
///
/// This is not hypothetical. It is why `doctor_asks_no_directory_whether_it_shares_blocks`
/// passed on every developer machine and failed on every CI runner while the state root's
/// change time was moving on both.
#[test]
fn the_change_time_reading_notices_a_write_that_lands_in_the_same_second() {
    let directory = tempfile::TempDir::new().expect("a temporary directory");
    let before = changed_at(directory.path());

    let planted = directory.path().join("a-probe-would-write-this");
    std::fs::write(&planted, "x").expect("a writable directory");
    std::fs::remove_file(&planted).expect("a removable file");

    assert_ne!(
        changed_at(directory.path()),
        before,
        "a file written and removed inside one second was not noticed, so the reading \
         above cannot catch a probe"
    );
}

/// A machine whose state root holds no record is told so, and doctor still writes
/// nothing. Taking the record is a write, and this is the command that may not.
#[test]
fn doctor_says_when_nothing_recorded_whether_the_state_root_shares_blocks() {
    let machine = Machine::new();
    machine.unit("worker-import");
    let record = sharing::path_in(&machine.state);
    assert!(record.is_file(), "making the state root recorded no answer");
    std::fs::remove_file(&record).expect("a record that can be removed");

    let report = stdout(&machine.nodal(&["doctor"]));
    assert!(report.contains("has not recorded"), "doctor claimed an answer nobody took:\n{report}");
    assert!(!record.exists(), "doctor took a record, which is a write");
}

#[test]
fn doctor_leaves_the_checkout_and_the_state_directory_byte_for_byte_as_it_found_them() {
    let machine = Machine::new();
    machine.unit("worker-import");
    machine.unit("payroll-export");
    // Leftovers for the report to be about, so that the command is not reporting on an
    // empty machine and passing this test by having nothing to touch.
    let (checkout, beside) = plant(&machine);

    let source = Snapshot::of(&checkout);
    let outside = Snapshot::of(&beside);
    let state = Snapshot::of_except(&machine.state, is_registry);
    let before = rows(&machine);
    assert!(!source.is_empty() && !state.is_empty(), "there is nothing here to leave alone");
    assert!(!outside.is_empty(), "the worktree beside the checkout is not there");

    let report = stdout(&machine.nodal(&["doctor"]));
    assert!(report.contains(NESTED), "doctor reported nothing about this machine:\n{report}");
    assert!(
        report.contains(BESIDE),
        "doctor did not report the worktree beside the checkout:\n{report}"
    );

    source.assert_unchanged(&Snapshot::of(&checkout), "doctor wrote in the checkout");
    outside.assert_unchanged(
        &Snapshot::of(&beside),
        "doctor wrote in the worktree beside the checkout",
    );
    state.assert_unchanged(
        &Snapshot::of_except(&machine.state, is_registry),
        "doctor wrote in the state directory",
    );
    assert_eq!(before, rows(&machine), "doctor changed a row of the registry");
}

/// The branch audit reads every ref of the checkout, and refs are the part of a
/// repository a person would most mind a report touching.
///
/// `--all` is used because it is the rendering that prints every bucket, so the whole
/// audit is on the page and nothing is skipped for being quiet. The audit itself runs
/// either way.
#[test]
fn the_branch_audit_leaves_every_ref_of_the_checkout_alone() {
    let machine = Machine::new();
    machine.unit("worker-import");
    let (checkout, beside) = plant(&machine);

    let source = Snapshot::of(&checkout);
    let outside = Snapshot::of(&beside);
    let state = Snapshot::of_except(&machine.state, is_registry);

    let report = stdout(&machine.nodal(&["doctor", "--all"]));
    assert!(report.contains("branches"), "the branch section is not in the report:\n{report}");
    assert!(report.contains(LEFT_BEHIND), "a branch with no worktree was not reported:\n{report}");

    source.assert_unchanged(&Snapshot::of(&checkout), "the branch audit wrote in the checkout");
    outside.assert_unchanged(&Snapshot::of(&beside), "the branch audit wrote in a worktree");
    state.assert_unchanged(
        &Snapshot::of_except(&machine.state, is_registry),
        "the branch audit wrote in the state directory",
    );
}

#[test]
fn the_json_rendering_leaves_the_machine_alone_as_well() {
    let machine = Machine::new();
    machine.unit("worker-import");
    let (checkout, beside) = plant(&machine);

    let source = Snapshot::of(&checkout);
    let outside = Snapshot::of(&beside);
    let state = Snapshot::of_except(&machine.state, is_registry);

    let report = stdout(&machine.nodal(&["doctor", "--json"]));
    assert!(report.contains(NESTED), "the JSON answer reported nothing:\n{report}");
    assert!(report.contains(BESIDE), "the JSON answer left out a worktree:\n{report}");

    source.assert_unchanged(&Snapshot::of(&checkout), "doctor --json wrote in the checkout");
    outside.assert_unchanged(
        &Snapshot::of(&beside),
        "doctor --json wrote in the worktree beside the checkout",
    );
    state.assert_unchanged(
        &Snapshot::of_except(&machine.state, is_registry),
        "doctor --json wrote in the state directory",
    );
}
