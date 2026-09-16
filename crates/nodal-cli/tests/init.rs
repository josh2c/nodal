//! `nodal init` end to end: the binary reads a project, writes a recipe, and says what
//! it could not answer.
//!
//! The engine's own acceptance test lives in `nodal-core` (`tests/recipe.rs`); this one
//! covers the seam the user meets — the arguments, the file appearing where it was
//! promised, and the refusal to overwrite a recipe without being asked twice.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

mod state;

use std::path::Path;
use std::process::Output;

use state::Machine;

/// The smallest project that infers something and still leaves a gap: a package manager
/// and a script, no toolchain pin, no migrations and no services.
fn write_project(root: &Path) {
    std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    std::fs::write(root.join("package.json"), "{ \"scripts\": { \"test\": \"vitest\" } }\n")
        .unwrap();
}

fn nodal(machine: &Machine, root: &Path, args: &[&str]) -> Output {
    machine.nodal().arg("init").args(args).arg(root).output().unwrap()
}

#[test]
fn init_writes_the_recipe_and_names_every_gap() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    let output = nodal(&machine, root, &[]);
    assert!(output.status.success(), "init exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    for key in ["toolchain", "db.migrations_dir", "services"] {
        assert!(stdout.contains(key), "{key} was not reported as a gap: {stdout}");
    }

    let written = std::fs::read_to_string(root.join("nodal.toml")).unwrap();
    assert!(written.contains("package_manager = [\"pnpm\"]"), "{written}");
    assert!(written.contains("# GAP:"), "{written}");
}

#[test]
fn init_refuses_to_overwrite_a_recipe_until_it_is_told_to() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);
    assert!(nodal(&machine, root, &[]).status.success());

    let refused = nodal(&machine, root, &[]);
    assert!(!refused.status.success(), "a second init should have failed");
    assert!(String::from_utf8(refused.stderr).unwrap().contains("--force"));
    assert!(nodal(&machine, root, &["--force"]).status.success());
}

#[test]
fn print_writes_nothing_and_json_carries_the_gaps() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);

    let printed = nodal(&machine, root, &["--print"]);
    assert!(printed.status.success());
    assert!(String::from_utf8(printed.stdout).unwrap().contains("[commands]"));
    assert!(!root.join("nodal.toml").exists(), "--print must not write");

    let json = nodal(&machine, root, &["--json"]);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["existed"], false);
    assert_eq!(value["gaps"].as_array().map(Vec::len), Some(3));
    assert!(!root.join("nodal.toml").exists(), "--json must not write");
}

/// EV-3, met by the comparison harness: `--force` kept every key and replaced every
/// comment with the template's own, and said nothing about it. It still writes the
/// template's comments — the file is rendered from the merged recipe — but it now names
/// every line it takes out before it takes it.
#[test]
fn force_names_the_line_it_takes_out_of_a_file_a_person_edited() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);
    assert!(nodal(&machine, root, &[]).status.success());

    let path = root.join("nodal.toml");
    let note = "# ours: the staging copy needs the seed step";
    let edited = format!("{note}\n{}", std::fs::read_to_string(&path).unwrap());
    std::fs::write(&path, &edited).unwrap();

    let forced = nodal(&machine, root, &["--force"]);
    assert!(forced.status.success());
    let said = String::from_utf8(forced.stdout).unwrap();
    assert!(said.contains("lines this rewrite changes"), "{said}");
    assert!(said.contains(note), "the comment it dropped is not named: {said}");
    assert!(!std::fs::read_to_string(&path).unwrap().contains(note), "it was dropped");

    // The same reading in the document a tool reads.
    let again = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{note}\n{again}")).unwrap();
    let json = nodal(&machine, root, &["--force", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let changes = value["changes"].as_array().unwrap();
    assert!(
        changes.iter().any(|change| change["edit"] == "removed" && change["text"] == note),
        "{value}"
    );
}

/// A rewrite that changes nothing says so by carrying no changed line, not by claiming
/// a change it did not make.
#[test]
fn force_over_a_file_init_itself_wrote_changes_no_line() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let machine = Machine::new();
    write_project(root);
    assert!(nodal(&machine, root, &[]).status.success());

    let json = nodal(&machine, root, &["--force", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["existed"], true);
    assert!(value.get("changes").is_none(), "no changed line is carried: {value}");
}
