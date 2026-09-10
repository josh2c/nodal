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

/// The line of `nodal ps` output about one process.
fn line(text: &str, pid: u32) -> String {
    text.lines()
        .find(|line| line.split_whitespace().any(|word| word == pid.to_string()))
        .unwrap_or_else(|| panic!("no row for {pid} in:\n{text}"))
        .to_owned()
}

#[test]
fn a_process_started_with_the_homes_environment_is_certain() {
    if !platform::reads_process_table("certain by environment") {
        return;
    }
    let fixture = Fixture::new();
    let child = process::carrying(Fixture::unit_id(), &fixture.home);

    let output = fixture.nodal(&["ps"], &fixture.outside());
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout);
    let row = line(&text, child.pid());
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

    let output = fixture.nodal(&["ps"], &fixture.outside());
    let text = String::from_utf8_lossy(&output.stdout);
    let row = line(&text, child.pid());
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

    let output = fixture.nodal(&["ps", "--json"], &fixture.outside());
    let answer: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json is one document");
    let rows = answer["rows"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|row| row["pid"].as_u64() == Some(u64::from(child.pid())))
        .unwrap_or_else(|| panic!("no row for the process in {answer}"));
    assert_eq!(row["confidence"], "certain");
    assert_eq!(row["signal"], "environment");
    assert_eq!(row["kind"], "process");
    assert_eq!(row["slug"], SLUG);
    assert_eq!(row["unit"], Fixture::unit_id());
}
