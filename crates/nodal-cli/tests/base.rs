//! `nodal base` end to end: the three actions the user meets.
//!
//! The engine's acceptance test lives in `nodal-core` (`tests/substrate.rs`). This one
//! covers the seam: that a build reports itself on standard error and answers on
//! standard output, that `--json` keeps the answer alone on standard output, that `ls`
//! shows what was built, and that `gc` removes it.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::{Command, Output};

/// A project with a remote to clone, nothing to install, and no build command.
fn world(root: &Path) {
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(seed.join("package.json"), "{ \"name\": \"cli-fixture\" }\n").unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "--quiet", "-m", "first"]);
    git(root, &["clone", "--quiet", "--bare", "--", path(&seed), path(&root.join("remote"))]);
    git(root, &["clone", "--quiet", "--", path(&root.join("remote")), path(&root.join("work"))]);
}

fn path(value: &Path) -> &str {
    value.to_str().unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=nodal test", "-c", "user.email=test@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

/// Run `nodal base`, with this test's own Nodal home and registry.
fn nodal(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nodal"))
        .arg("base")
        .args(args)
        .arg(root.join("work"))
        .args(["--store", path(&root.join("registry.db"))])
        .env("HOME", root)
        .env("USERPROFILE", root)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn base_build_reports_on_stderr_and_answers_on_stdout() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    world(root);

    let built = nodal(root, &["build"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let progress = String::from_utf8(built.stderr.clone()).unwrap();
    assert!(progress.contains("a fresh clone of"), "the first base clones the remote: {progress}");
    assert!(stdout(&built).contains("built"), "{}", stdout(&built));

    // Asking again finds the base rather than building a second one.
    let again = nodal(root, &["build"]);
    assert!(again.status.success());
    assert!(stdout(&again).contains("already warm"), "{}", stdout(&again));

    let listed = nodal(root, &["ls", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(json["bases"].as_array().unwrap().len(), 1, "{json}");
    assert_eq!(json["bases"][0]["pins"], 0);
}

#[test]
fn json_keeps_the_answer_alone_on_standard_output() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    world(root);

    let built = nodal(root, &["build", "--json"]);
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    assert!(built.stderr.is_empty(), "nothing but the answer: {:?}", built.stderr);
    let json: serde_json::Value = serde_json::from_slice(&built.stdout).unwrap();
    assert_eq!(json["built"], true, "{json}");
}

#[test]
fn base_gc_removes_the_bases_that_are_not_kept() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    world(root);
    assert!(nodal(root, &["build"]).status.success());

    let swept = nodal(root, &["gc", "--keep", "0", "--json"]);
    assert!(swept.status.success(), "{}", String::from_utf8_lossy(&swept.stderr));
    let json: serde_json::Value = serde_json::from_slice(&swept.stdout).unwrap();
    assert_eq!(json["removed"].as_array().unwrap().len(), 1, "{json}");

    let listed = nodal(root, &["ls"]);
    assert!(stdout(&listed).contains("no base built yet"), "{}", stdout(&listed));
}
