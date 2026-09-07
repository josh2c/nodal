//! `nodal init` end to end: the binary reads a project, writes a recipe, and says what
//! it could not answer.
//!
//! The engine's own acceptance test lives in `nodal-core` (`tests/recipe.rs`); this one
//! covers the seam the user meets — the arguments, the file appearing where it was
//! promised, and the refusal to overwrite a recipe without being asked twice.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

/// The smallest project that infers something and still leaves a gap: a package manager
/// and a script, no toolchain pin, no migrations and no services.
fn write_project(root: &Path) {
    std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    std::fs::write(root.join("package.json"), "{ \"scripts\": { \"test\": \"vitest\" } }\n")
        .unwrap();
}

fn nodal(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nodal")).arg("init").args(args).arg(root).output().unwrap()
}

#[test]
fn init_writes_the_recipe_and_names_every_gap() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write_project(root);

    let output = nodal(root, &[]);
    assert!(output.status.success(), "init exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    for key in ["toolchain", "db.migrations_dir", "services"] {
        assert!(stdout.contains(key), "{key} was not reported as a gap: {stdout}");
    }

    let written = std::fs::read_to_string(root.join("nodal.toml")).unwrap();
    assert!(written.contains("package_manager = \"pnpm\""), "{written}");
    assert!(written.contains("# GAP:"), "{written}");
}

#[test]
fn init_refuses_to_overwrite_a_recipe_until_it_is_told_to() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write_project(root);
    assert!(nodal(root, &[]).status.success());

    let refused = nodal(root, &[]);
    assert!(!refused.status.success(), "a second init should have failed");
    assert!(String::from_utf8(refused.stderr).unwrap().contains("--force"));
    assert!(nodal(root, &["--force"]).status.success());
}

#[test]
fn print_writes_nothing_and_json_carries_the_gaps() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    write_project(root);

    let printed = nodal(root, &["--print"]);
    assert!(printed.status.success());
    assert!(String::from_utf8(printed.stdout).unwrap().contains("[commands]"));
    assert!(!root.join("nodal.toml").exists(), "--print must not write");

    let json = nodal(root, &["--json"]);
    assert!(json.status.success());
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["existed"], false);
    assert_eq!(value["gaps"].as_array().map(Vec::len), Some(3));
    assert!(!root.join("nodal.toml").exists(), "--json must not write");
}
