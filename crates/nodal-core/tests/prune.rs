//! What a reclaimed home loses on its way into the trash, on real repositories.
//!
//! The sweep asks Git which paths an ignore rule covers, so every claim it makes needs a
//! repository with a commit in it to be a claim at all. That is what puts these tests
//! here rather than beside the code: a unit test that spawns `git` is a spawn seam in
//! product source, and Nodal keeps one seam per tool
//! ([`nodal_core::git`]). The two cases that need no repository stay in the module,
//! where they are the only tests of it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::Command;

use nodal_core::workspace::prune::sweep;

/// A home with a build directory, a dependency tree under a second package, a
/// committed directory whose name the table also holds, and the local state a
/// person goes back into the trash for.
fn home() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    for directory in ["target/debug", "apps/web/node_modules/react", "src", "vendor/build"] {
        std::fs::create_dir_all(path.join(directory)).unwrap();
    }
    std::fs::write(path.join("target/debug/app"), [0_u8; 4096]).unwrap();
    std::fs::write(path.join("apps/web/node_modules/react/index.js"), [0_u8; 2048]).unwrap();
    std::fs::write(path.join("vendor/build/thing.o"), [0_u8; 512]).unwrap();
    std::fs::write(path.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(path.join(".env.local"), "TOKEN=local").unwrap();
    std::fs::write(path.join("dev.sqlite"), [0_u8; 128]).unwrap();
    std::fs::write(path.join(".gitignore"), "target/\nnode_modules/\n.env.local\n*.sqlite\n")
        .unwrap();
    git(path, &["init", "--quiet"]);
    git(path, &["config", "user.email", "test@example.invalid"]);
    git(path, &["config", "user.name", "test"]);
    git(path, &["add", "--all"]);
    git(path, &["commit", "--quiet", "-m", "the tree"]);
    root
}

fn git(repo: &Path, args: &[&str]) {
    let ran = Command::new("git").args(args).current_dir(repo).output().unwrap();
    assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
}

fn paths(entries: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = entries.iter().map(|path| path.display().to_string()).collect();
    names.sort();
    names
}

#[test]
fn build_output_and_dependencies_go_wherever_they_sit() {
    let root = home();
    let report = sweep(root.path());
    let removed: Vec<PathBuf> = report.removed.iter().map(|removal| removal.path.clone()).collect();
    assert_eq!(paths(&removed), ["apps/web/node_modules", "target"]);
    assert!(!root.path().join("target").exists());
    assert!(!root.path().join("apps/web/node_modules").exists());
    assert!(report.bytes >= 6144, "the report counts what it dropped: {}", report.bytes);
}

#[test]
fn a_directory_the_project_tracks_stays_whatever_its_name_is() {
    let root = home();
    let report = sweep(root.path());
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert!(
        root.path().join("vendor/build/thing.o").is_file(),
        "a committed build directory is not the prune's to take"
    );
    assert!(root.path().join("src/main.rs").is_file());
}

#[test]
fn local_state_stays_and_the_report_names_it() {
    let root = home();
    let report = sweep(root.path());
    let kept: Vec<PathBuf> = report.kept.iter().map(|entry| entry.path.clone()).collect();
    assert_eq!(paths(&kept), [".env.local", "dev.sqlite"]);
    assert!(root.path().join(".env.local").is_file());
    assert!(root.path().join("dev.sqlite").is_file());
    assert!(report.kept_bytes > 0, "the kept state is measured too");
}

#[test]
fn the_files_nodal_wrote_are_not_reported_as_a_person_local_state() {
    let root = home();
    let path = root.path();
    std::fs::create_dir_all(path.join(".nodal")).unwrap();
    std::fs::write(path.join(".nodal/env"), "NODAL_ID=x\n").unwrap();
    std::fs::write(path.join(".nodal/id"), "x\n").unwrap();
    std::fs::write(path.join(".envrc"), "dotenv .nodal/env\n").unwrap();
    std::fs::write(path.join("WORKUNIT.md"), "the objective\n").unwrap();
    let exclude = path.join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "/.nodal/\n/.envrc\n/WORKUNIT.md\n").unwrap();

    let report = sweep(path);
    let kept: Vec<PathBuf> = report.kept.iter().map(|entry| entry.path.clone()).collect();
    assert_eq!(
        paths(&kept),
        [".env.local", "dev.sqlite"],
        "the report offers a person their own state and not nodal's"
    );
    assert!(path.join(".nodal/id").is_file(), "nodal's own files are still in the trash");
    assert!(path.join(".envrc").is_file());
}

#[test]
fn a_second_sweep_finds_nothing_and_removes_nothing() {
    let root = home();
    assert_eq!(sweep(root.path()).removed.len(), 2);
    let again = sweep(root.path());
    assert!(again.changed_nothing());
    assert_eq!(again.bytes, 0);
    assert!(again.describe().contains("no build output"));
}

#[test]
fn the_report_says_what_went() {
    let root = home();
    let body = sweep(root.path()).describe();
    assert!(body.contains("Dropped 2 directories"), "{body}");
    assert!(body.contains("target"), "{body}");
}
