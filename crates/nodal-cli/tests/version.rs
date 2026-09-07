//! Acceptance test for T0.1: the built binary reports its version.
//!
//! CI runs the same assertion against the downloaded release artifact through
//! `ci/acceptance-version.sh`, so a binary that only works in the build tree fails.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn nodal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nodal"))
}

#[test]
fn version_flag_prints_name_and_version() {
    let output = nodal().arg("--version").output().unwrap();
    assert!(output.status.success(), "--version exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("nodal {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn bare_invocation_prints_help_and_succeeds() {
    let output = nodal().output().unwrap();
    assert!(output.status.success(), "bare invocation exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--store"), "help is missing the global options: {stdout}");
}

#[test]
fn unknown_flag_is_an_error() {
    let output = nodal().arg("--not-a-flag").output().unwrap();
    assert!(!output.status.success(), "unknown flag should not succeed");
}
