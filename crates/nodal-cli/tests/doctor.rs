//! Acceptance for `nodal doctor` (T1.0), driven through the binary.
//!
//! Three claims the library tests cannot make, because they are about the command:
//!
//! 1. Doctor answers on a machine that has never run Nodal. There is no unit, no
//!    project row and nothing managed, and the report still names the two sections.
//! 2. `--json` is the same answer: the fields a person reads are fields a tool reads.
//! 3. The words. Doctor removes nothing, and its output never says that it did.
//!
//! The machine here is one checkout with one nested worktree in it, which is enough to
//! put a row in the first section. `crates/nodal-core/tests/doctor.rs` is what covers
//! every kind of leftover, the lock, the second section and the zero-writes claim.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod home;

use std::path::{Path, PathBuf};
use std::process::Command;

use home::Fixture;

/// A checkout with one nested worktree in it.
fn checkout(root: &Path) -> PathBuf {
    let checkout = root.join("code/app");
    std::fs::create_dir_all(&checkout).unwrap();
    git(&checkout, &["init", "--quiet", "."]);
    std::fs::write(checkout.join("README.md"), "# app\n").unwrap();
    git(&checkout, &["add", "--all"]);
    git(
        &checkout,
        &[
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "user.name=test",
            "commit",
            "--quiet",
            "--message=start",
        ],
    );
    git(&checkout, &["worktree", "add", "--quiet", "-b", "side", ".claude/worktrees/side"]);
    checkout
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// The output of `nodal doctor` in a checkout, as text.
fn doctor(fixture: &Fixture, cwd: &Path, args: &[&str]) -> String {
    let mut all = vec!["doctor"];
    all.extend_from_slice(args);
    let output = fixture.nodal(&all, cwd);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_machine_that_has_never_run_nodal_still_gets_both_sections() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    let text = doctor(&fixture, &checkout, &[]);
    assert!(text.contains("this project"), "{text}");
    assert!(text.contains("not this project"), "{text}");
    assert!(text.contains(".claude/worktrees/side"), "{text}");
}

#[test]
fn the_json_answer_carries_the_same_rows() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    let text = doctor(&fixture, &checkout, &["--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("one JSON document");
    let here = parsed["here"].as_array().expect("the first section");
    assert!(
        here.iter().any(|row| {
            row["kind"] == "nested_worktree" && row["what"] == ".claude/worktrees/side"
        }),
        "{text}"
    );
    assert!(parsed["elsewhere"].is_array(), "{text}");
    assert!(parsed["now"].is_string(), "{text}");
}

#[test]
fn the_report_never_says_it_took_anything_away() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    let text = doctor(&fixture, &checkout, &[]).to_lowercase();
    for word in ["deleted", "removed the", "cleaned", "freed", "pruned"] {
        assert!(!text.contains(word), "{word:?} is in the output of a command that only reads");
    }
    assert!(text.contains("it removed nothing"), "{text}");
}

#[test]
fn the_checkout_is_unchanged_by_the_report() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    let before = Command::new("git")
        .arg("-C")
        .arg(&checkout)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    doctor(&fixture, &checkout, &[]);
    let after = Command::new("git")
        .arg("-C")
        .arg(&checkout)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&before.stdout),
        String::from_utf8_lossy(&after.stdout),
        "doctor changed the working tree"
    );
}
