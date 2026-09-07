//! Acceptance for `nodal ps` (T1.10): the rows a person sees, and the same answer as
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

use std::process::{Child, Command};

use home::{Fixture, SLUG};

/// A child that is killed when the test ends, whichever way it ends.
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Whether this host publishes a process table, and a word about it when it does not.
fn has_proc(claim: &str) -> bool {
    if cfg!(target_os = "linux") {
        return true;
    }
    eprintln!("skipped ({claim}): a process scan reads /proc, which this host does not have");
    false
}

/// The line of `nodal ps` output about one process.
fn line(text: &str, pid: u32) -> String {
    text.lines()
        .find(|line| line.split_whitespace().any(|word| word == pid.to_string()))
        .unwrap_or_else(|| panic!("no row for {pid} in:\n{text}"))
        .to_owned()
}

#[test]
fn a_process_started_with_the_homes_environment_is_certain() {
    if !has_proc("certain by environment") {
        return;
    }
    let fixture = Fixture::new();
    let child = Sleeper(
        Command::new("sleep")
            .arg("30")
            .env("NODAL_ID", Fixture::unit_id())
            .env("NODAL_ROOT", &fixture.home)
            .spawn()
            .unwrap(),
    );

    let output = fixture.nodal(&["ps"], &fixture.outside());
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout);
    let row = line(&text, child.0.id());
    assert!(row.contains(SLUG), "{row}");
    assert!(row.contains("process"), "{row}");
    assert!(row.contains("sleep 30"), "{row}");
    assert!(row.contains("certain"), "{row}");
    assert!(row.contains("env"), "{row}");
}

#[test]
fn a_process_that_only_stands_in_the_home_is_probable() {
    if !has_proc("probable by directory") {
        return;
    }
    let fixture = Fixture::new();
    let child = Sleeper(
        Command::new("sleep")
            .arg("30")
            .current_dir(&fixture.home)
            .env_remove("NODAL_ID")
            .env_remove("NODAL_ROOT")
            .spawn()
            .unwrap(),
    );

    let output = fixture.nodal(&["ps"], &fixture.outside());
    let text = String::from_utf8_lossy(&output.stdout);
    let row = line(&text, child.0.id());
    assert!(row.contains(SLUG), "{row}");
    assert!(row.contains("probable"), "{row}");
    assert!(row.contains("cwd"), "{row}");
}

#[test]
fn the_json_answer_carries_the_same_row_with_its_confidence() {
    if !has_proc("json") {
        return;
    }
    let fixture = Fixture::new();
    let child = Sleeper(
        Command::new("sleep")
            .arg("30")
            .env("NODAL_ID", Fixture::unit_id())
            .env("NODAL_ROOT", &fixture.home)
            .spawn()
            .unwrap(),
    );

    let output = fixture.nodal(&["ps", "--json"], &fixture.outside());
    let answer: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json is one document");
    let rows = answer["rows"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|row| row["pid"].as_u64() == Some(u64::from(child.0.id())))
        .unwrap_or_else(|| panic!("no row for the process in {answer}"));
    assert_eq!(row["confidence"], "certain");
    assert_eq!(row["signal"], "environment");
    assert_eq!(row["kind"], "process");
    assert_eq!(row["slug"], SLUG);
    assert_eq!(row["unit"], Fixture::unit_id());
}
