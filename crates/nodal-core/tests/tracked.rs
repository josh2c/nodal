//! Acceptance for T1.4b: `base.exclude` never drops a path the project tracks.
//!
//! The fixture project ([`nodal_fixture`]) has two heavy directories the exclusion
//! table names, `test-results` and `coverage`, and a `.gitignore` that ignores both.
//! Here the fixture is made a repository that **tracks** `coverage`, which is what a
//! project does when it commits a baseline report. The name still says the content is
//! generated; the commit says it is the project's own, and the commit wins.
//!
//! Both ends are checked, because either one alone leaves the hole open:
//!
//!   - inference proposes `test-results` and not `coverage`, so a recipe Nodal writes
//!     never carries the row;
//!   - the copy refuses a list that holds `coverage` whatever wrote it, and the message
//!     names the path, so a recipe a person wrote by hand is caught as well.
//!
//! The cost of the hole is what the second check measures: a copy made with the bad
//! list is missing every tracked file under the directory, and `git status` in it
//! reports one deletion for each.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use nodal_core::recipe;
use nodal_core::workspace::{Excludes, select_backend, tracked};

/// The heavy directory the fixture tracks in these tests.
const TRACKED: &str = "coverage";

/// The heavy directory the fixture leaves untracked.
const UNTRACKED: &str = "test-results";

/// The fixture, written into a temporary directory and committed as a repository.
///
/// `coverage` is added with `--force`, because the fixture's own `.gitignore` names it.
/// That is exactly the situation this task is about: the ignore rule and the index
/// disagree, and only the index says what a copy must carry.
fn fixture_repository() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = nodal_fixture::write(directory.path());
    git(&root, &["init", "--quiet", "."]);
    git(&root, &["add", "--all"]);
    git(&root, &["add", "--force", "--", TRACKED]);
    git(
        &root,
        &[
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "user.name=test",
            "commit",
            "--quiet",
            "--message=fixture",
        ],
    );
    (directory, root)
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git").arg("-C").arg(dir).args(args).status().expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

/// What `git status --porcelain` reports in a tree, one line per path.
fn status(dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .expect("git runs");
    String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()
}

#[test]
fn inference_proposes_the_heavy_directory_the_project_does_not_track() {
    let (_directory, root) = fixture_repository();
    let effective = recipe::load(&root).expect("the fixture is a readable project");
    let proposed: Vec<&Path> = effective.recipe.base.exclude.iter().map(PathBuf::as_path).collect();
    assert!(
        proposed.contains(&Path::new(UNTRACKED)),
        "the untracked heavy directory is still proposed: {proposed:?}"
    );
    assert!(
        !proposed.contains(&Path::new(TRACKED)),
        "a tracked heavy directory was proposed as an exclude: {proposed:?}"
    );
}

#[test]
fn a_copy_that_would_drop_a_tracked_path_is_refused_by_a_message_naming_it() {
    let (_directory, root) = fixture_repository();
    let list = Excludes::with_recipe(&[PathBuf::from(TRACKED)]);
    let refused = tracked::refuse(&root, &list).expect_err("a tracked exclude must be refused");
    let message = refused.to_string();
    assert!(message.contains(TRACKED), "the message does not name the path: {message}");
    assert!(
        !message.contains(UNTRACKED),
        "the message names a path that is not tracked: {message}"
    );
}

#[test]
fn the_copy_the_refusal_prevents_is_dirty_at_birth() {
    let (_directory, root) = fixture_repository();
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let home = elsewhere.path().join("home");
    let list = Excludes::with_recipe(&[PathBuf::from(TRACKED)]);

    assert!(status(&root).is_empty(), "the fixture repository is clean before it is copied");
    select_backend(elsewhere.path()).clone_tree(&root, &home, &list).expect("a copy");

    let dirty = status(&home);
    assert!(
        dirty.iter().all(|line| line.starts_with(" D ") && line.contains(TRACKED)),
        "the copy is dirty for another reason than the dropped path: {dirty:?}"
    );
    assert!(!dirty.is_empty(), "the copy of a tracked directory left out is not clean");
}

#[test]
fn a_list_that_drops_nothing_tracked_is_allowed() {
    let (_directory, root) = fixture_repository();
    let list = Excludes::from_paths([Path::new(UNTRACKED)]);
    tracked::refuse(&root, &list).expect("an untracked exclude is allowed");
}

/// The gate reads the whole list, and Nodal's own table is part of it.
///
/// `coverage` is a row of [`nodal_core::workspace::exclude::ROWS`]. On a project that
/// tracks that directory the default list is refused too, so the rule holds for the
/// rows Nodal ships and not only for the rows a project adds.
#[test]
fn the_default_table_is_refused_when_the_project_tracks_one_of_its_rows() {
    let (_directory, root) = fixture_repository();
    let refused =
        tracked::refuse(&root, &Excludes::default_list()).expect_err("the table is checked too");
    assert!(refused.to_string().contains(TRACKED), "{refused}");
}
