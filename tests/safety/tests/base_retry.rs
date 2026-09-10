//! A failed base build keeps what it paid for, and says why it failed.
//!
//! Two promises, and both were broken in the same way. A base build runs a clone, a
//! checkout and an install, and the install is the step that fails: a lockfile the host
//! cannot satisfy, a registry that is down, a package manager of the wrong version. When
//! that happened, Nodal undid every step before it. The clone went in the bin, and the
//! person was shown a failure with nothing in it.
//!
//! That is what cal.com and formbricks both hit. The second `nodal new` cloned the
//! repository again, waited for it again, and failed again, with the same empty message.
//!
//! So:
//!
//! * **Never an error without its reason.** A tool that exits non-zero has its output
//!   carried into the error, from both streams. Which of the two a package manager
//!   writes its reason to is the package manager's choice, and pnpm chooses standard
//!   output. An error that kept standard error alone was empty exactly when the reason
//!   existed.
//! * **Never discard a clone because a later step failed.** The clone stays where the
//!   failed build left it, and the attempt after it carries on from the step that
//!   failed. One clone, across both attempts.
//!
//! The two are asserted together because they are one event: the same failed install
//! both leaves the clone and produces the message.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::InState as _;
use nodal_safety::{Machine, answer, stderr};

/// What the stub package manager writes to standard output when it refuses to install.
const REASON: &str = "ERR_PNPM_OUTDATED_LOCKFILE";

/// What it writes to standard error at the same moment.
const NOTE: &str = "a note that is not the reason";

#[test]
fn a_failed_install_reports_what_the_tool_wrote_on_both_streams() {
    let machine = Machine::failing_once();
    let failed = machine.nodal(&["base", "build"]);

    assert!(!failed.status.success(), "the failing install was reported as a success");
    let told = format!("{}{}", answer(&failed), stderr(&failed));
    assert!(told.contains(REASON), "the reason the tool gave is not in the failure: {told}");
    assert!(told.contains(NOTE), "the other stream is not in the failure either: {told}");
}

#[test]
fn a_failed_install_keeps_the_clone_it_was_installing_into() {
    let machine = Machine::failing_once();
    let failed = machine.nodal(&["base", "build"]);

    assert!(!failed.status.success(), "{}", stderr(&failed));
    assert!(machine.bases().is_empty(), "a build that failed left a base: {:?}", machine.bases());
    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "the clone the failed build paid for is gone: {kept:?}");
    assert!(kept[0].join(".git").is_dir(), "what it kept is not a clone: {:?}", kept[0]);
}

#[test]
fn the_attempt_after_a_failure_carries_on_with_the_same_clone() {
    let machine = Machine::failing_once();
    let failed = machine.nodal(&["base", "build"]);
    assert!(!failed.status.success(), "{}", stderr(&failed));

    // A file no clone would make, put where only this attempt's work is. If the second
    // attempt cloned again, its base will not have it.
    let kept = machine.partials();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let witness = kept[0].join(".the-first-attempt-was-here");
    std::fs::write(&witness, "one clone, not two").unwrap();

    let built = machine.nodal(&["base", "build"]);

    assert!(built.status.success(), "the retry failed: {}", stderr(&built));
    let bases = machine.bases();
    assert_eq!(bases.len(), 1, "the retry did not finish one base: {bases:?}");
    assert!(
        bases[0].join(".the-first-attempt-was-here").is_file(),
        "the retry cloned again instead of carrying on with the clone that was there"
    );
    assert!(
        bases[0].join(Machine::installed()).is_file(),
        "the retry did not run the install step that failed"
    );
    assert!(machine.partials().is_empty(), "the promoted base left its partial behind");
}

#[test]
fn the_retry_says_which_step_stopped_and_why_before_it_carries_on() {
    let machine = Machine::failing_once();
    assert!(!machine.nodal(&["base", "build"]).status.success());

    let built = machine.nodal(&["base", "build"]);
    let told = format!("{}{}", answer(&built), stderr(&built));

    assert!(told.contains("install"), "the report does not name the step: {told}");
    assert!(told.contains(REASON), "the report does not carry the reason: {told}");
}
