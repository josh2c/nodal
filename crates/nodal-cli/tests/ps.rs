//! Acceptance for `nodal ps`: the rows a person sees, and the same answer as
//! JSON.
//!
//! Three claims, driven through the binary rather than through the library:
//!
//! 1. A process started with the home's environment is one row, `certain`, by `env`.
//! 2. A process that only stands in the home is one row, `probable`, by `cwd`.
//! 3. `--json` is the same answer: the same row, with the confidence as a field a tool
//!    reads rather than a word a person reads.
//!
//! The rows are read from `/proc`, so on a host without one each check reports itself as
//! skipped. `crates/nodal-core/tests/attribution.rs` is what covers that host.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod home;
mod state;

use home::{Fixture, SLUG};
use nodal_safety::{platform, process};

/// The line of `nodal ps` output about one process, when there is one.
fn line(text: &str, pid: u32) -> Option<String> {
    text.lines()
        .find(|line| line.split_whitespace().any(|word| word == pid.to_string()))
        .map(ToOwned::to_owned)
}

/// Run `nodal ps` again until it has a row about `pid`, and answer with that row.
///
/// A process exists before it has replaced itself with the program it was started for, and
/// a scan taken in that instant reads the environment the test binary had rather than the
/// one the test gave the process. So there is no row — rarely, and never twice the same
/// way. The command is a question about the machine, so it is asked again until the machine
/// answers or the deadline passes. What is asserted about the row is asserted once.
fn row_for(fixture: &Fixture, pid: u32) -> String {
    process::until("a row in nodal ps for the process the test started", || {
        let output = fixture.nodal(&["ps"], &fixture.outside());
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        line(&String::from_utf8_lossy(&output.stdout), pid)
    })
}

/// The same, for the rows `--json` answers with.
fn json_row_for(fixture: &Fixture, pid: u32) -> serde_json::Value {
    process::until("a JSON row for the process the test started", || {
        let output = fixture.nodal(&["ps", "--json"], &fixture.outside());
        let answer: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("--json is one document");
        answer["rows"]
            .as_array()?
            .iter()
            .find(|row| row["pid"].as_u64() == Some(u64::from(pid)))
            .cloned()
    })
}

#[test]
fn a_process_started_with_the_homes_environment_is_certain() {
    if !platform::reads_process_table("certain by environment") {
        return;
    }
    let fixture = Fixture::new();
    let child = process::carrying(Fixture::unit_id(), &fixture.home);

    let row = row_for(&fixture, child.pid());
    assert!(row.contains(SLUG), "{row}");
    assert!(row.contains("process"), "{row}");
    assert!(row.contains("sleep 30"), "{row}");
    assert!(row.contains("certain"), "{row}");
    assert!(row.contains("env"), "{row}");
}

#[test]
fn a_process_that_only_stands_in_the_home_is_probable() {
    if !platform::reads_process_table("probable by directory") {
        return;
    }
    let fixture = Fixture::new();
    let child = process::standing_in(&fixture.home);

    let row = row_for(&fixture, child.pid());
    assert!(row.contains(SLUG), "{row}");
    assert!(row.contains("probable"), "{row}");
    assert!(row.contains("cwd"), "{row}");
}

#[test]
fn the_json_answer_carries_the_same_row_with_its_confidence() {
    if !platform::reads_process_table("json") {
        return;
    }
    let fixture = Fixture::new();
    let child = process::carrying(Fixture::unit_id(), &fixture.home);

    let row = json_row_for(&fixture, child.pid());
    assert_eq!(row["confidence"], "certain");
    assert_eq!(row["signal"], "environment");
    assert_eq!(row["kind"], "process");
    assert_eq!(row["slug"], SLUG);
    assert_eq!(row["unit"], Fixture::unit_id());
}
