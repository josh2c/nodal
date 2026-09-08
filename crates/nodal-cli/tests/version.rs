//! Acceptance test for T0.1: the built binary reports its version.
//!
//! CI runs the same assertion against the downloaded release artifact through
//! `ci/acceptance-version.sh`, so a binary that only works in the build tree fails.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod state;

use state::Machine;

/// The command under test, with a state directory of this test's own.
///
/// `--version` and the help never open the registry, and a bare invocation does. All
/// three are built the same way, so that the one which does cannot reach the state
/// directory belonging to whoever is running the tests.
fn nodal(machine: &Machine) -> std::process::Command {
    machine.nodal()
}

#[test]
fn version_flag_prints_name_and_version() {
    let machine = Machine::new();
    let output = nodal(&machine).arg("--version").output().unwrap();
    assert!(output.status.success(), "--version exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("nodal {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_prints_help_and_succeeds() {
    let machine = Machine::new();
    let output = nodal(&machine).output().unwrap();
    assert!(output.status.success(), "bare invocation exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--store"), "help is missing the global options: {stdout}");
}

#[test]
fn unknown_flag_is_an_error() {
    let machine = Machine::new();
    let output = nodal(&machine).arg("--not-a-flag").output().unwrap();
    assert!(!output.status.success(), "unknown flag should not succeed");
}
