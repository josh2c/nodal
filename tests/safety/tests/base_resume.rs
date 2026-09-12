//! A build that stopped is carried on with, whatever release stopped it.
//!
//! Every step of a base build now works at the path the base is handed over at, and a
//! mark beside the directory says the tree there is not a base yet. A release before
//! that one assembled the tree one name away instead. Both leave work on the disk worth
//! minutes, and the journal of either says the clone step is done, so the attempt that
//! resumes one skips the step that would have noticed which shape it is looking at.
//!
//! Three states, and each of them was a way to lose a tree or to announce a base that
//! is not there:
//!
//! * **A tree an earlier release left beside the name.** The steps after the clone run
//!   at a directory that does not exist, and a retry fails with the clone still on the
//!   disk and no way to use it.
//! * **A tree half way between the two names.** Carrying it across by way of a third
//!   name nothing records puts the only copy somewhere the next sweep removes. The mark
//!   is written first and the tree is moved once, so every state in between is one the
//!   attempt after it can finish.
//! * **No tree at all.** A tool is free to remove what it was pointed at. The last step
//!   asks about the tree before it asks about the mark, because a mark that is gone
//!   means the step has already run only where the tree it handed over is still there.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::InState as _;
use nodal_safety::{Machine, answer, stderr};

/// A file no clone and no install would make, to tell one attempt's work from another's.
const WITNESS: &str = ".the-first-attempt-was-here";

/// A sibling of a path, named by adding to the path's own name.
///
/// The suite spells the two suffixes itself rather than reading them off the product,
/// so that a state is built the same way whichever release wrote it.
fn beside(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Where a release before the mark assembled a base.
fn legacy(destination: &Path) -> PathBuf {
    beside(destination, ".partial")
}

/// The mark that says the directory at a base's own name is not a base yet.
fn mark(destination: &Path) -> PathBuf {
    beside(destination, ".building")
}

/// Take away whatever mark the release under test left beside the tree.
///
/// Named by neither suffix alone, so that a state is shaped the same way whichever
/// release wrote the failure the test starts from.
fn unmark(destination: &Path) {
    for suffix in [".building", ".partial"] {
        let at = beside(destination, suffix);
        if at.is_file() {
            std::fs::remove_file(&at).unwrap();
        }
    }
}

/// The tree a failed build left, with a file of the test's own in it.
///
/// The file is what tells one clone from another: an attempt that cloned again rather
/// than carrying on would have a tree without it.
fn stopped(machine: &Machine) -> PathBuf {
    let failed = machine.nodal(&["base", "build", "--warm"]);
    assert!(
        !failed.status.success(),
        "the failing build command was a success: {}",
        stderr(&failed)
    );
    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "the tree the failed build worked in is gone: {kept:?}");
    std::fs::write(kept[0].join(WITNESS), "one clone, not two").unwrap();
    kept[0].clone()
}

/// How many times the package manager installed into this tree.
fn installs(tree: &Path) -> usize {
    let at = tree.join(Machine::installed());
    std::fs::read_to_string(&at)
        .unwrap_or_else(|why| panic!("nothing installed into {at}: {why}", at = at.display()))
        .lines()
        .count()
}

/// The directory the stub build command recorded, as a path this host can compare.
fn built_in(base: &Path) -> PathBuf {
    let recorded = base.join(Machine::built_in());
    let text = std::fs::read_to_string(&recorded).unwrap_or_else(|why| {
        panic!("the warm build recorded nothing at {at}: {why}", at = recorded.display())
    });
    PathBuf::from(text.trim())
}

/// A path as the stub reports it, so a comparison is about the directory and not about
/// which of its names the host prefers.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|why| panic!("{at} is not there: {why}", at = path.display()))
}

/// What the retry has to end with, whichever state it started from.
fn carried_on(machine: &Machine, destination: &Path) {
    let base = machine.base();
    assert_eq!(base, destination, "the retry built a base somewhere else");
    assert!(
        base.join(WITNESS).is_file(),
        "the retry cloned again instead of carrying on with the tree that was there"
    );
    assert_eq!(installs(&base), 1, "the retry installed a second time");
    assert_eq!(built_in(&base), resolved(&base), "the retry warmed a path it did not hand over");
    assert!(!legacy(destination).exists(), "the earlier release's name is still there");
    assert!(machine.partials().is_empty(), "the finished base is still marked unbuilt");
}

#[test]
fn a_tree_an_earlier_release_left_beside_the_name_is_carried_on_with() {
    let machine = Machine::failing_to_warm_once();
    let destination = stopped(&machine);

    // Exactly what a release before the mark leaves: the tree one name away, and no
    // mark, with a journal that says the clone step is done.
    unmark(&destination);
    std::fs::rename(&destination, legacy(&destination)).unwrap();

    let built = machine.nodal(&["base", "build", "--warm"]);

    assert!(built.status.success(), "the retry failed: {}", stderr(&built));
    carried_on(&machine, &destination);
}

#[test]
fn a_tree_half_way_between_the_two_names_is_kept_and_finished() {
    let machine = Machine::failing_to_warm_once();
    let destination = stopped(&machine);

    // Where an attempt that died between writing the mark and moving the tree leaves
    // things: the mark beside the name, and the tree still under the earlier one. The
    // tree is one rename from where it belongs and is nowhere a sweep can reach.
    unmark(&destination);
    std::fs::rename(&destination, legacy(&destination)).unwrap();
    std::fs::write(mark(&destination), "this base is still being built\n").unwrap();

    let built = machine.nodal(&["base", "build", "--warm"]);

    assert!(built.status.success(), "the retry failed: {}", stderr(&built));
    carried_on(&machine, &destination);
}

#[test]
fn a_build_whose_tree_was_removed_announces_no_base_and_says_why() {
    let machine = Machine::warming_into_nothing();

    let built = machine.nodal(&["base", "build", "--warm"]);

    assert!(!built.status.success(), "a build with no tree left was reported as a success");
    let told = format!("{}{}", answer(&built), stderr(&built));
    assert!(told.contains("is not there"), "the failure does not say what was missing: {told}");
    assert!(machine.bases().is_empty(), "a build with no tree left a base: {:?}", machine.bases());
    let store = machine.store();
    let project = machine.project(&store);
    let rows = nodal_core::store::bases::list_for_project(store.conn(), project.id).unwrap();
    assert!(rows.is_empty(), "a build with no tree wrote a base row: {rows:?}");
}
