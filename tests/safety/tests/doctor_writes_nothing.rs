//! `nodal doctor` reads this machine and leaves every part of it exactly as it was.
//!
//! Doctor is the command a person runs when a disk is full and they are already
//! worried. It reports the checkout's other worktrees, stale caches, orphan databases
//! and units, each with a size, and it removes nothing (`decisions/DL-015`). A read
//! command that wrote would be worse than one that refused: the person ran it to find
//! out what is there, not to change it.
//!
//! "Nothing changed" is asserted byte for byte, on every tree doctor reads: the person's
//! checkout, a worktree of that checkout that lives beside it rather than inside it, and
//! Nodal's whole state directory with every home, base and trashed home in it. The
//! worktree outside the checkout is a tree of its own here for the same reason it is a
//! row of its own in the report: doctor reads it because the repository names it, so a
//! snapshot of the checkout would not be watching the directory doctor opened. The registry file is the one thing left out, and it is left out for a stated
//! reason rather than to make the test pass: every command that opens the registry first
//! finishes or rolls back an operation an earlier run was killed in the middle of, and
//! that preamble writes. It is not doctor. So the registry's *rows* are read before and
//! after instead, and they have to be the same rows.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::store::{Store, environments, projects, units};
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
