//! A bare `nodal` in a repository Nodal has never seen prints the verdict and makes
//! nothing.
//!
//! This is the first command most people who ever use Nodal will run, and it runs on a
//! machine whose owner has not agreed to anything yet. They have twelve worktrees, a
//! full disk and no reason to trust a new tool. So the promise the table is printed
//! under is stronger than the one `nodal doctor` makes: doctor leaves the machine as it
//! found it, and **the verdict additionally does not create Nodal's own state directory
//! or its registry**. A person who types `nodal` once, reads the table and never types
//! it again is owed a machine with no trace of it.
//!
//! Four properties, and each is one test here.
//!
//! | property | what a break looks like |
//! |---|---|
//! | nothing is made | a state directory or a registry appears on a machine that had none |
//! | nothing is written | a byte of the checkout, or of a worktree beside it, differs |
//! | `--json` is the same reading | the flag chooses how much is shown, never what is touched |
//! | the two newer readings read | dating a ref or recovering an intent leaves a mark on the repository or on the agent's own records |
//! | an error carries its reason | a directory that is no repository is refused with a sentence that names it |
//!
//! The worktree beside the checkout is watched as a tree of its own, for the reason
//! `doctor_writes_nothing.rs` gives: the repository names it, so the verdict opens it,
//! and a snapshot of the checkout would not be watching the directory that was opened.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::checkout::{BESIDE, NESTED};
use nodal_safety::{Machine, Snapshot, checkout, stderr, stdout};

/// The checkout every property here is asserted on, and the two worktrees it names.
///
/// [`checkout::plain`] is the shape, stated once for every suite about the verdict.
fn plant(machine: &Machine) -> (PathBuf, PathBuf) {
    let plain = checkout::plain(machine);
    (plain.checkout, plain.beside)
}

/// Whether the state directory is there at all.
fn state_exists(machine: &Machine) -> bool {
    Path::new(&machine.state).exists()
}

#[test]
fn a_bare_nodal_in_an_unknown_checkout_makes_no_state_directory_and_no_registry() {
    let machine = Machine::new();
    let (checkout, _) = plant(&machine);
    assert!(!state_exists(&machine), "this machine already has a state directory");

    let report = stdout(&machine.nodal_in(&checkout, &[]));

    assert!(report.contains(NESTED), "the verdict reported nothing about this checkout:\n{report}");
    assert!(report.contains("nodal removed nothing"), "the closing line is missing:\n{report}");
    assert!(
        !state_exists(&machine),
        "nodal made its state directory to answer a question about a checkout it holds no row \
         for; the first command a person types must leave no trace"
    );
}

#[test]
fn the_verdict_leaves_the_checkout_and_every_worktree_byte_for_byte_as_it_found_them() {
    let machine = Machine::new();
    let (checkout, beside) = plant(&machine);

    let source = Snapshot::of(&checkout);
    let outside = Snapshot::of(&beside);
    assert!(!source.is_empty(), "there is nothing here to leave alone");
    assert!(!outside.is_empty(), "the worktree beside the checkout is not there");

    let report = stdout(&machine.nodal_in(&checkout, &[]));
    assert!(
        report.contains(BESIDE),
        "the verdict did not report the worktree beside the checkout:\n{report}"
    );

    source.assert_unchanged(&Snapshot::of(&checkout), "the verdict wrote in the checkout");
    outside.assert_unchanged(
        &Snapshot::of(&beside),
        "the verdict wrote in the worktree beside the checkout",
    );
    assert!(!state_exists(&machine), "the verdict made a state directory");
}

#[test]
fn the_json_answer_is_the_same_reading_and_makes_nothing_either() {
    let machine = Machine::new();
    let (checkout, beside) = plant(&machine);

    let source = Snapshot::of(&checkout);
    let outside = Snapshot::of(&beside);

    let report = stdout(&machine.nodal_in(&checkout, &["ls", "--json"]));
    assert!(report.contains(NESTED), "the JSON answer reported nothing:\n{report}");
    assert!(report.contains(BESIDE), "the JSON answer left out a worktree:\n{report}");
    assert!(report.contains("\"unpushed\""), "the JSON answer carries no row fields:\n{report}");

    source.assert_unchanged(&Snapshot::of(&checkout), "nodal ls --json wrote in the checkout");
    outside.assert_unchanged(&Snapshot::of(&beside), "nodal ls --json wrote in a worktree");
    assert!(!state_exists(&machine), "nodal ls --json made a state directory");
}

/// Never an error without its reason.
///
/// A directory that is neither a project nor a repository is the one case where Nodal
/// has nothing to print, and the refusal has to say which directory it is about. `ls`
/// is used rather than the bare word because a bare `nodal` there prints the help,
/// which is the surface a person who has not started is owed.
#[test]
fn a_directory_that_is_no_repository_is_refused_with_the_reason_and_the_path() {
    let machine = Machine::new();
    let elsewhere = machine.source.parent().expect("the machine root").join("not-a-repository");
    std::fs::create_dir_all(&elsewhere).unwrap();

    let refused = machine.nodal_in(&elsewhere, &["ls"]);
    assert!(!refused.status.success(), "a directory that is no repository was not refused");
    let said = stderr(&refused);
    // The name a running process is given for its own directory, which on a host whose
    // temporary directory is a link is not the name this test used to make it.
    let named = std::fs::canonicalize(&elsewhere).unwrap_or_else(|_| elsewhere.clone());
    assert!(
        said.contains(&named.display().to_string()),
        "the refusal does not say which directory it is about:\n{said}"
    );
    assert!(said.contains("is in no project Nodal knows"), "the refusal gives no reason:\n{said}");
    assert!(!state_exists(&machine), "a refusal made a state directory");
}

/// Never a network call of our own.
///
/// The verdict compares a worktree with a revision, and the tempting way to make that
/// comparison current is to fetch first. It does not. The machine's remote is removed
/// outright, so a reading that needed one would fail rather than quietly succeed
/// against a stale ref, and the table still prints every row.
#[test]
fn the_verdict_asks_no_remote_anything() {
    let machine = Machine::new();
    let (checkout, _) = plant(&machine);
    let _ = std::process::Command::new("git")
        .args(["-C", checkout.to_str().unwrap(), "remote", "remove", "origin"])
        .output();

    let answered = machine.nodal_in(&checkout, &[]);
    let report = stdout(&answered);
    assert!(answered.status.success(), "the verdict failed with no remote: {}", stderr(&answered));
    assert!(report.contains(NESTED), "the verdict left out a worktree:\n{report}");
    assert!(!state_exists(&machine), "the verdict made a state directory");
}

/// The two readings the table gained after it shipped are reads as well.
///
/// One dates the ref that BEHIND is measured against, by opening the log Git keeps for
/// it. The other recovers what a worktree was made for, by opening a session record
/// another tool wrote. Neither is Nodal's file, both are opened on a machine whose owner
/// has agreed to nothing, and both paths are checked because the flag chooses how much is
/// shown and never what is touched.
///
/// Claude Code's own records are watched as a tree of their own. They are not in the
/// checkout and not in the state directory, so nothing else here would notice a write
/// into them.
#[test]
fn dating_a_ref_and_recovering_an_intent_write_nothing_on_either_path() {
    let machine = Machine::new();
    let plain = checkout::plain(&machine);
    checkout::tracking_origin(&plain.checkout);
    checkout::last_moved_days_ago(&plain.checkout, "refs/remotes/origin/main", 23);
    checkout::record_intent(&machine, &plain.nested, "Fix the join in the importer");

    let source = Snapshot::of(&plain.checkout);
    let outside = Snapshot::of(&plain.beside);
    let records = Snapshot::of(machine.config_dir());
    assert!(!source.is_empty(), "there is nothing here to leave alone");
    assert!(!records.is_empty(), "no session record was planted");

    for arguments in [&[][..], &["ls", "--json"][..]] {
        let report = stdout(&machine.nodal_in(&plain.checkout, arguments));
        assert!(report.contains(NESTED), "the verdict reported nothing:\n{report}");
        source
            .assert_unchanged(&Snapshot::of(&plain.checkout), "the verdict wrote in the checkout");
        outside.assert_unchanged(&Snapshot::of(&plain.beside), "the verdict wrote in a worktree");
        records.assert_unchanged(
            &Snapshot::of(machine.config_dir()),
            "the verdict wrote into the records of the agent it recovered an intent from",
        );
        assert!(!state_exists(&machine), "the verdict made a state directory");
    }
}
