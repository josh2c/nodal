//! A warm base hands over a tree that is still warm.
//!
//! `nodal base build --warm` runs the project's build command so that the units cloned
//! from the base do not have to. The command returning zero was taken as proof that it
//! had. It is not. A build does its work at a path, and the tools it runs write that
//! path into what they produce: Cargo records the absolute path of every source file
//! outside a package root, a Python environment writes it into the first line of each
//! script, a Node install writes it into its links. A base that ran the build under one
//! name and was handed over under another hands over work that the next command does
//! again.
//!
//! On this project that cost the three workspace crates, every time, on a base that had
//! just reported success. So:
//!
//! * **The warm build runs where the base is delivered.** The recorded directory is the
//!   base's own, not a name it was assembled under.
//! * **Never call a base ready when its warm step lacks a valid result.** A build
//!   command that failed leaves a directory that is not a base and no row that offers
//!   one.
//! * **Never discard a clone because a later step failed.** The clone and the install
//!   stay where the failed build left them, and the attempt after it carries on with
//!   them.
//! * **Never an error without its reason.** What the build command wrote, on both
//!   streams, is in the failure.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::InState as _;
use nodal_safety::{Machine, answer, stderr};

/// What the stub build command writes to standard output when it refuses.
const REASON: &str = "ERR_BUILD_FAILED";

/// What it writes to standard error at the same moment.
const NOTE: &str = "a note that is not the reason";

/// A file no clone and no install would make, to tell one attempt's work from another's.
const WITNESS: &str = ".the-first-attempt-was-here";

/// The directory the stub build command recorded, as a path this host can compare.
fn built_in(base: &Path) -> PathBuf {
    let recorded = base.join(Machine::built_in());
    let text = std::fs::read_to_string(&recorded).unwrap_or_else(|why| {
        panic!("the warm build recorded nothing at {at}: {why}", at = recorded.display())
    });
    PathBuf::from(text.trim())
}

/// A path as the stub reports it, so that a comparison is about the directory and not
/// about which of its names the host prefers.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|why| panic!("{at} is not there: {why}", at = path.display()))
}

#[test]
fn a_warm_base_runs_the_build_where_it_hands_the_base_over() {
    let machine = Machine::new();
    let built = machine.nodal(&["base", "build", "--warm"]);

    assert!(built.status.success(), "{}", stderr(&built));
    let base = machine.base();
    assert_eq!(
        built_in(&base),
        resolved(&base),
        "the warm build ran somewhere other than the base it was handed over as, so \
         everything it produced names a path that is not there"
    );
}

#[test]
fn a_base_whose_build_command_failed_is_not_a_base() {
    let machine = Machine::failing_to_warm();
    let failed = machine.nodal(&["base", "build", "--warm"]);

    assert!(!failed.status.success(), "the failing build command was reported as a success");
    assert!(machine.bases().is_empty(), "a failed warm left a base: {:?}", machine.bases());
    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "the tree the failed build worked in is gone: {kept:?}");
    let store = machine.store();
    let project = machine.project(&store);
    let rows = nodal_core::store::bases::list_for_project(store.conn(), project.id).unwrap();
    assert!(
        rows.is_empty(),
        "a failed warm wrote a base row for units to be cloned from: {rows:?}"
    );
}

#[test]
fn a_failed_warm_keeps_the_clone_and_the_install_it_paid_for() {
    let machine = Machine::failing_to_warm();
    let failed = machine.nodal(&["base", "build", "--warm"]);

    assert!(!failed.status.success(), "{}", stderr(&failed));
    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].join(".git").is_dir(), "the clone it paid for is gone: {:?}", kept[0]);
    assert!(
        kept[0].join(Machine::installed()).is_file(),
        "the install it paid for is gone: {:?}",
        kept[0]
    );
}

#[test]
fn a_failed_warm_reports_what_the_build_wrote_on_both_streams() {
    let machine = Machine::failing_to_warm();
    let failed = machine.nodal(&["base", "build", "--warm"]);

    assert!(!failed.status.success(), "the failing build command was reported as a success");
    let told = format!("{}{}", answer(&failed), stderr(&failed));
    assert!(told.contains(REASON), "the reason the build gave is not in the failure: {told}");
    assert!(told.contains(NOTE), "the other stream is not in the failure either: {told}");
}

#[test]
fn the_attempt_after_a_failed_warm_carries_on_and_ends_up_warm() {
    let machine = Machine::failing_to_warm_once();
    let failed = machine.nodal(&["base", "build", "--warm"]);
    assert!(!failed.status.success(), "{}", stderr(&failed));

    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "{kept:?}");
    std::fs::write(kept[0].join(WITNESS), "one clone, not two").unwrap();

    let built = machine.nodal(&["base", "build", "--warm"]);

    assert!(built.status.success(), "the retry failed: {}", stderr(&built));
    let base = machine.base();
    assert!(
        base.join(WITNESS).is_file(),
        "the retry cloned again instead of carrying on with the work that was there"
    );
    assert_eq!(built_in(&base), resolved(&base), "the retry warmed a path it did not hand over");
    assert!(machine.partials().is_empty(), "the finished base is still marked unbuilt");
}
