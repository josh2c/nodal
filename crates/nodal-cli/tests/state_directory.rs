//! A test's commands keep their state where the test says, never where the person
//! running the tests keeps theirs.
//!
//! Nodal's state directory is `<home>/.nodal` unless `NODAL_HOME` says otherwise, and
//! the first command that opens the registry makes both the directory and the file. A
//! test that built a command for the binary and named no state directory therefore ran
//! against the registry of whoever was running it. That is not a theory: `base`, `env`,
//! `init`, `logging` and `version` all did, and on a workstation whose registry a newer
//! build had already migrated, four of those suites failed while every clean runner
//! stayed green.
//!
//! `tests/state/mod.rs` is the fix, and this file is what keeps it fixed. It asserts the
//! two halves that together mean the guard is doing something:
//!
//! 1. a command built the old way really does write into the home directory it is given,
//!    so the reproduction reproduces and the second claim is not vacuous;
//! 2. a command built by the harness leaves that home directory untouched and writes
//!    into the state directory the test owns.
//!
//! Both halves are asserted with `nodal ps`, and the choice of command is the point of
//! this paragraph. It has to be a command that opens the registry, because the claim is
//! about where the registry is made. A bare `nodal` is not one: it answers about the
//! directory it stands in, and where that directory is a checkout Nodal holds no row
//! for, it prints the verdict on the checkout's worktrees without making a registry at
//! all (`cli::registry_if_present`). That is a promise of its own, kept by
//! `tests/safety/tests/verdict_writes_nothing.rs`, and a command that keeps it cannot
//! also demonstrate the fault this file is about.
//!
//! Neither half goes near the real `~/.nodal`. Each gives the command a temporary
//! directory as its home, which is the fault under a microscope rather than the fault.
//!
//! The third test is the part that lasts: nowhere else in these tests may name the
//! binary, so a command built any other way cannot be added by accident.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod state;

use std::path::{Path, PathBuf};
use std::process::Command;

use state::Machine;
use tempfile::TempDir;

/// What Nodal calls its state directory under a home directory that names none.
const DEFAULT: &str = ".nodal";

/// A home directory belonging to nobody, standing in for the person's.
fn a_home_of_its_own() -> TempDir {
    TempDir::new().unwrap()
}

/// Give a command this home directory, under both names a platform looks it up by.
fn living_in<'a>(command: &'a mut Command, home: &Path) -> &'a mut Command {
    command.env("HOME", home).env("USERPROFILE", home)
}

#[test]
fn a_command_that_names_no_state_directory_writes_into_the_home_directory() {
    let home = a_home_of_its_own();
    let mut command = Command::new(state::BINARY);
    living_in(&mut command, home.path()).args(["ps", "-vv"]).env_remove("NODAL_HOME");

    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        home.path().join(DEFAULT).join("registry.db").is_file(),
        "the fault this file is about did not happen, so the next test proves nothing"
    );
}

#[test]
fn a_command_the_harness_built_leaves_the_home_directory_alone() {
    let home = a_home_of_its_own();
    let machine = Machine::new();
    let mut command = machine.nodal();
    living_in(&mut command, home.path()).args(["ps", "-vv"]);

    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        machine.registry().is_file(),
        "the command wrote its registry somewhere other than the state directory it was given"
    );
    assert!(!home.path().join(DEFAULT).exists(), "the command reached the home directory");
    assert_eq!(
        std::fs::read_dir(home.path()).unwrap().count(),
        0,
        "the command left something in the home directory"
    );
}

/// Every command for the binary is built in one place, so that none can be built
/// without a state directory.
///
/// The name of the variable Cargo publishes the binary's path under is put together
/// here rather than written out, because a test that searched for a string it contains
/// would find itself.
#[test]
fn only_the_harness_names_the_binary() {
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let allowed = tests.join("state").join("mod.rs");
    let needle = concat!("CARGO_BIN", "_EXE_nodal");

    let mut named = Vec::new();
    for file in rust_files(&tests) {
        if file == allowed {
            continue;
        }
        if std::fs::read_to_string(&file).unwrap().contains(needle) {
            named.push(file.strip_prefix(&tests).unwrap_or(&file).to_path_buf());
        }
    }
    assert!(
        named.is_empty(),
        "{named:?} name the binary themselves. Build the command with `state::nodal`, or \
         take its path from `state::BINARY` and give the process the state directory."
    );
}

/// Every Rust file under a directory, however deep.
fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|kind| kind == "rs") {
            found.push(path);
        }
    }
    found.sort();
    found
}
