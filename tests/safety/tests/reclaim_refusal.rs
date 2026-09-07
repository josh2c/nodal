//! Reclaim refusal: a home holding work that exists nowhere else is not taken away.
//!
//! Reclaim is the one command that removes a person's tree, so it is the one command
//! that can lose a morning. One check answers whether a home holds work that is only
//! there ([`nodal_core::lifecycle::uniqueness`]), and it reports three kinds of loss.
//! Each of the three is a test here, because a check that finds two of them is a check
//! that loses the third.
//!
//! Every one of them asserts the same three things after the refusal: the command
//! failed, the message named what it found, and the home and its work are still exactly
//! where they were. A refusal that removed the home first would be no refusal.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::Path;

use nodal_safety::{Machine, git, stderr, stdout};

/// A tracked file of the fixture, changed to make uncommitted work.
const TRACKED: &str = "apps/web/app/page.tsx";

/// A path no ignore rule of the fixture covers, so a file there is work.
const UNTRACKED: &str = "notes.txt";

/// Insist that a refusal named what it found and changed nothing.
fn refused(machine: &Machine, home: &Path, slug: &str, expected: &str) {
    let output = machine.nodal(&["reclaim", slug]);
    assert!(!output.status.success(), "a home holding work that is only there was reclaimed");
    let told = stderr(&output);
    assert!(told.contains(expected), "the refusal does not say {expected:?}: {told}");

    assert!(home.is_dir(), "the refusal moved the home");
    assert_eq!(machine.homes(), vec![home.to_path_buf()], "the home is still the project's");
    assert!(machine.trashed().is_empty(), "the refusal put something in the trash");
}

#[test]
fn a_reclaim_is_refused_when_the_home_holds_uncommitted_changes() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");
    std::fs::write(home.join(TRACKED), "half-finished\n").unwrap();

    refused(&machine, &home, "worker-import", "uncommitted changes (1): apps/web/app/page.tsx");
    assert_eq!(
        std::fs::read_to_string(home.join(TRACKED)).unwrap(),
        "half-finished\n",
        "the work the refusal was about is still there"
    );
}

#[test]
fn a_reclaim_is_refused_when_the_home_holds_untracked_files() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");
    std::fs::write(home.join(UNTRACKED), "notes to self\n").unwrap();
    // A path an ignore rule covers is a build artefact and not work, so it must not be
    // what the refusal is about.
    std::fs::create_dir_all(home.join("dist")).unwrap();
    std::fs::write(home.join("dist/out.js"), "generated\n").unwrap();

    refused(&machine, &home, "worker-import", "untracked files (1): notes.txt");
    let told = stderr(&machine.nodal(&["reclaim", "worker-import"]));
    assert!(!told.contains("dist/out.js"), "an ignored path was counted as work: {told}");
}

#[test]
fn a_reclaim_is_refused_when_the_home_holds_commits_no_other_tree_has() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");
    std::fs::write(home.join(TRACKED), "committed in this home\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);

    refused(&machine, &home, "worker-import", "commits on no remote (1)");

    // The commits a home inherited are in the person's own checkout, so they are not
    // work that is only here: once the commit is somewhere else, the reclaim goes ahead.
    git(&machine.source, &["fetch", "--quiet", home.to_str().unwrap(), "HEAD"]);
    let allowed = machine.nodal(&["reclaim", "worker-import"]);
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert!(stdout(&allowed).contains("nothing that is only here"));
    assert_eq!(machine.trashed().len(), 1, "the home the reclaim took is in the trash");
}

#[test]
fn a_clean_unit_is_reclaimed_so_the_refusals_are_about_the_work_and_not_the_command() {
    let machine = Machine::new();
    let home = machine.unit("worker-import");
    let reclaimed = machine.nodal(&["reclaim", "worker-import"]);

    assert!(reclaimed.status.success(), "{}", stderr(&reclaimed));
    assert!(!home.exists(), "the home is not where it was");
    assert!(machine.homes().is_empty(), "a live home was left: {:?}", machine.homes());
    assert_eq!(machine.trashed().len(), 1, "the trash holds it, and only it");
}
