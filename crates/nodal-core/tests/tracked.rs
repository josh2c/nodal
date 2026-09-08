//! Acceptance for T1.4b and T1.4c: an exclusion list against what the project tracks.
//!
//! The fixture project ([`nodal_fixture`]) has two heavy directories the exclusion
//! table names, `test-results` and `coverage`, and a `.gitignore` that ignores both.
//! Here the fixture is made a repository that **tracks** `coverage`, which is what a
//! project does when it commits a baseline report. The name still says the content is
//! generated; the commit says it is the project's own, and the commit wins.
//!
//! What "the commit wins" costs depends on who wrote the row, and both answers are
//! checked here:
//!
//!   - inference proposes `test-results` and not `coverage`, so a recipe Nodal writes
//!     never carries the row;
//!   - a row the **project wrote** in `base.exclude` is refused, and the message names
//!     the path, so a recipe a person wrote by hand is caught;
//!   - a **default** row of Nodal's own table yields instead: no recipe key can override
//!     such a row, so a refusal would leave the project unable to make any unit at all.
//!     The copy keeps the directory and answers with one note naming the row (T1.4c).
//!
//! The cost of the hole is what the fourth check measures: a copy made with the bad
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
    let mut list = Excludes::with_recipe(&[PathBuf::from(TRACKED)]);
    let refused =
        tracked::enforce(&root, &mut list).expect_err("a tracked exclude must be refused");
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
    let mut list = Excludes::from_paths([Path::new(UNTRACKED)]);
    let kept = tracked::enforce(&root, &mut list).expect("an untracked exclude is allowed");
    assert!(kept.is_empty(), "a row nothing tracks was reported as kept: {kept:?}");
    assert!(list.excludes(Path::new(UNTRACKED)), "the row is still on the list");
}

/// The gate reads the whole list, and Nodal's own table yields where it is wrong.
///
/// `coverage` is a row of [`nodal_core::workspace::exclude::ROWS`], which no recipe key
/// can take off the list. On a project that tracks that directory the row yields: the
/// copy keeps the directory, and one note names the row and why it was kept. Refusing
/// here would leave such a project unable to make a unit at all (T1.4c).
#[test]
fn a_default_row_the_project_tracks_yields_and_the_copy_says_which() {
    let (_directory, root) = fixture_repository();
    let mut list = Excludes::default_list();
    let kept = tracked::enforce(&root, &mut list).expect("a default row yields, never refuses");

    let paths: Vec<&Path> = kept.iter().map(|one| one.path.as_path()).collect();
    assert_eq!(paths, [Path::new(TRACKED)], "the wrong set of rows yielded");
    assert!(!list.excludes(Path::new(TRACKED)), "the yielded row still leaves the path out");
    assert!(list.excludes(Path::new(UNTRACKED)), "a row nothing tracks was taken off the list");

    let note = kept[0].to_string();
    assert!(note.contains(TRACKED), "the note does not name the row: {note}");
    assert!(
        note.contains("output of a run that did not happen here"),
        "the note does not say why the row is on the list: {note}"
    );
}

/// The copy such a note is about carries the tracked directory and is clean.
///
/// This is the other half of `the_copy_the_refusal_prevents_is_dirty_at_birth`: with the
/// row yielded, the same clone has nothing to report.
#[test]
fn the_copy_a_yielded_row_allows_is_clean_at_birth() {
    let (_directory, root) = fixture_repository();
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let home = elsewhere.path().join("home");
    let mut list = Excludes::default_list();
    tracked::enforce(&root, &mut list).expect("a default row yields");

    select_backend(elsewhere.path()).clone_tree(&root, &home, &list).expect("a copy");

    assert!(home.join(TRACKED).is_dir(), "the tracked directory was left out of the copy");
    assert!(!home.join(UNTRACKED).exists(), "an untracked heavy directory was carried in");
    assert!(status(&home).is_empty(), "the copy is dirty: {:?}", status(&home));
}
