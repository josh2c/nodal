//! Tracked excludes: a copy never leaves out a path the project tracks.
//!
//! An exclusion list is what keeps a home small. Its rows are named for content a build
//! makes — `coverage`, `test-results` — and a project that commits a baseline report
//! into one of those directories makes the name wrong for itself. A copy made with the
//! list as written would be missing every file under it, and `git status` in the unit
//! would report one deletion per file the moment the unit was made: work the person
//! never did, on the day they first tried Nodal.
//!
//! The gate stands where a copy is made, so this file asks for it the way a person meets
//! it: `nodal new` on a project that tracks such a path. What the gate answers depends
//! on who wrote the row.
//!
//! * A row of Nodal's **default** table is a guess from a directory's name, and no
//!   recipe key can take it off the list. So it yields to the commit: the unit is made,
//!   it carries the directory, and the report names the row that was kept.
//! * A row the project wrote in `base.exclude` is an instruction. The create stops at
//!   the step that would make the home, with a message naming the path, and no home is
//!   made.
//!
//! `crates/nodal-core/tests/tracked.rs` is where the inference half and the cost of the
//! hole are measured.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::{Machine, git, stderr, stdout};

/// A heavy directory the exclusion table names, which this project commits into.
const TRACKED: &str = "coverage";

/// One the project leaves to its ignore rules.
const UNTRACKED: &str = "test-results";

#[test]
fn a_default_row_the_project_tracks_is_kept_and_the_report_names_it() {
    let machine = Machine::tracking(&[TRACKED]);
    let made = machine.nodal(&["new", "--name", "worker-import"]);

    assert!(made.status.success(), "a tracked default row refused the create: {}", stderr(&made));
    let home = machine.home_of("worker-import");
    assert!(home.join(TRACKED).is_dir(), "the home is missing the directory the project tracks");
    assert!(
        !home.join(UNTRACKED).exists(),
        "the whole default list yielded, not the one row the project tracks"
    );

    let told = format!("{}{}", stdout(&made), stderr(&made));
    assert!(told.contains(TRACKED), "no note names the row that was kept: {told}");
    assert!(
        told.contains("output of a run that did not happen here"),
        "the note does not say why the row is on the list: {told}"
    );
}

#[test]
fn the_unit_a_kept_row_makes_is_clean_the_moment_it_exists() {
    let machine = Machine::tracking(&[TRACKED]);
    let home = machine.unit("worker-import");
    let status = git(&home, &["status", "--porcelain"]);

    assert!(status.is_empty(), "the unit is dirty the moment it was made: {status}");
}

#[test]
fn a_row_the_project_wrote_is_refused_and_the_message_names_it() {
    let machine = Machine::excluding(&[TRACKED], &[TRACKED]);
    let refused = machine.nodal(&["new", "--name", "worker-import"]);

    assert!(!refused.status.success(), "a copy that would drop tracked content was made");
    let told = stderr(&refused);
    assert!(told.contains(TRACKED), "the refusal does not name the path: {told}");
    assert!(
        !told.contains(UNTRACKED),
        "the refusal names a path the project does not track: {told}"
    );
    assert!(machine.homes().is_empty(), "the refusal left a home behind: {:?}", machine.homes());
}

#[test]
fn a_path_the_project_does_not_track_is_left_out_of_every_home() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");

    assert!(machine.source.join(UNTRACKED).is_dir(), "the project has the directory");
    assert!(!home.join(UNTRACKED).exists(), "an untracked heavy directory was carried into a home");
    assert!(!home.join(TRACKED).exists(), "so was the other one");
    assert!(
        home.join(Machine::installed()).is_file(),
        "a row the table keeps rather than drops was left out"
    );
}
