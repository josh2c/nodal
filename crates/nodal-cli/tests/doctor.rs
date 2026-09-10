//! Acceptance for `nodal doctor`, driven through the binary.
//!
//! Four claims the library tests cannot make, because they are about the command:
//!
//! 1. Doctor answers on a machine that has never run Nodal. There is no unit, no
//!    project row and nothing managed, and the report still names the two sections.
//! 2. `--json` is the same answer: the fields a person reads are fields a tool reads.
//! 3. The words. Doctor removes nothing, and its output never says that it did.
//! 4. A registry a later Nodal wrote does not end the answer. Every other command
//!    refuses that file and stops; doctor is what a person runs when something is
//!    wrong, so it reports what it can read and states the mismatch with the one
//!    command that upgrades this copy.
//!
//! The machine here is one checkout with one nested worktree in it, which is enough to
//! put a row in the first section. `crates/nodal-core/tests/doctor.rs` is what covers
//! every kind of leftover, the lock, the second section and the zero-writes claim.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod home;
mod state;

use std::path::{Path, PathBuf};
use std::process::Command;

use home::Fixture;
use nodal_core::store::Store;

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
        here.iter().any(|row| row["kind"] == "worktree" && row["what"] == ".claude/worktrees/side"),
        "{text}"
    );
    assert!(parsed["elsewhere"].is_array(), "{text}");
    assert!(parsed["now"].is_string(), "{text}");
}

/// Put a reclaimed home in the trash of a project under the state root.
fn trash(fixture: &Fixture, project: &str, home: &str, relative: &str, bytes: usize) {
    let path = fixture.root().join(project).join("trash").join(home).join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![0_u8; bytes]).unwrap();
}

#[test]
fn the_header_says_what_the_trash_holds() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());

    let empty = doctor(&fixture, &checkout, &[]);
    assert!(empty.contains("no reclaimed home is waiting for gc"), "{empty}");

    trash(&fixture, "project", "E00M0001", "src/main.rs", 2048);
    trash(&fixture, "project", "E00M0002", ".env.local", 64);
    trash(&fixture, "storefront", "E00M0003", "src/app.ts", 1024);

    let text = doctor(&fixture, &checkout, &[]);
    assert!(text.contains("3 reclaimed homes"), "the header does not count the trash: {text}");
    assert!(text.contains("3.1 kB"), "the header does not say what the trash holds: {text}");

    let parsed: serde_json::Value =
        serde_json::from_str(&doctor(&fixture, &checkout, &["--json"])).expect("one document");
    assert_eq!(parsed["trash"]["homes"], 3, "the json carries the same count");
    assert_eq!(parsed["trash"]["bytes"], 3136);
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

/// Stamp the fixture's registry at a schema this binary does not know.
///
/// The version is written through the store's own connection, which is what a later
/// Nodal would have left behind, and then every command that opens the file refuses it.
fn write_a_later_schema(fixture: &Fixture) {
    let store = Store::open(&fixture.store).unwrap();
    store.conn().execute_batch("PRAGMA user_version = 99;").unwrap();
}

#[test]
fn a_registry_a_later_nodal_wrote_is_a_note_and_not_the_end_of_the_answer() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    write_a_later_schema(&fixture);

    let text = doctor(&fixture, &checkout, &[]);

    assert!(text.contains("store:"), "the mismatch is a note: {text}");
    assert!(text.contains("schema 99"), "the note names the version the file carries: {text}");
    let known = format!("schema {}", nodal_core::store::SCHEMA_VERSION);
    assert!(text.contains(&known), "and the version this binary knows: {text}");
    assert!(text.contains("upgrade this nodal with:"), "{text}");
    assert!(
        text.contains(".claude/worktrees/side"),
        "what needs no registry is still reported: {text}"
    );
    assert!(text.contains("it removed nothing"), "{text}");
}

#[test]
fn the_note_names_the_command_that_upgrades_this_copy() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    write_a_later_schema(&fixture);

    let parsed: serde_json::Value =
        serde_json::from_str(&doctor(&fixture, &checkout, &["--json"])).expect("one document");

    let notes = parsed["notes"].as_array().expect("the notes");
    let store = notes.iter().find(|note| note["source"] == "store").expect("the store note");
    let why = store["why"].as_str().expect("one line");
    assert!(why.contains("nothing in the registry was read"), "{why}");
    assert!(
        why.contains("cargo install nodal --force") || why.contains("releases"),
        "the note carries the channel's own command: {why}"
    );
}

#[test]
fn every_other_command_still_refuses_the_same_registry() {
    let fixture = Fixture::new();
    let checkout = checkout(fixture.root());
    write_a_later_schema(&fixture);

    let refused = fixture.nodal(&["ls"], &checkout);

    assert!(!refused.status.success(), "a version refusal is doctor's exception, not a change");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert!(told.contains("99"), "{told}");
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

/// The branch section through the binary, in both renderings and with the flag.
///
/// The checkout here holds one branch a worktree has (`side`) and one nothing has
/// (`left-behind`), and it names no remote. A branch with commits and no remote to hold
/// them is unpushed, which is what `remote_containment` means and what the report says.
fn with_a_branch(root: &Path) -> PathBuf {
    let checkout = checkout(root);
    git(&checkout, &["branch", "left-behind"]);
    checkout
}

#[test]
fn a_branch_with_no_worktree_is_reported_and_a_branch_with_one_is_not() {
    let fixture = Fixture::new();
    let checkout = with_a_branch(fixture.root());

    let text = doctor(&fixture, &checkout, &[]);

    assert!(text.contains("local branches with no worktree"), "{text}");
    assert!(text.contains("left-behind"), "{text}");
    let (_, branches) = text.split_once("local branches with no worktree").expect("the section");
    assert!(!branches.contains("side"), "a branch with a worktree is not a branch row: {text}");
}

#[test]
fn the_json_answer_carries_every_branch_and_the_bucket_it_is_in() {
    let fixture = Fixture::new();
    let checkout = with_a_branch(fixture.root());

    let text = doctor(&fixture, &checkout, &["--json"]);

    let parsed: serde_json::Value = serde_json::from_str(&text).expect("one JSON document");
    let rows = parsed["branches"]["rows"].as_array().expect("the branch rows");
    let row = rows.iter().find(|row| row["name"] == "left-behind").expect("the branch");
    assert_eq!(row["standing"], "unpushed", "{text}");
    assert!(row["unpushed"].as_u64().is_some_and(|count| count > 0), "{text}");
    assert!(row["committed"].is_string(), "{text}");
}

/// `--all` changes how much is printed and nothing about what was found.
#[test]
fn all_prints_the_safe_buckets_and_reports_the_same_branches() {
    let fixture = Fixture::new();
    let checkout = with_a_branch(fixture.root());

    let plain: serde_json::Value =
        serde_json::from_str(&doctor(&fixture, &checkout, &["--json"])).expect("one document");
    let opened: serde_json::Value =
        serde_json::from_str(&doctor(&fixture, &checkout, &["--json", "--all"]))
            .expect("one document");

    assert_eq!(
        plain["branches"]["rows"], opened["branches"]["rows"],
        "--all is a rendering choice, not a second answer"
    );
    assert!(doctor(&fixture, &checkout, &["--all"]).contains("left-behind"));
}
