//! What a host genuinely cannot do, said out loud.
//!
//! The suite runs on Linux and on macOS, and every property in it holds on both. One
//! reading does not: the scan that says which granted ports are really bound reads
//! `/proc/net/tcp*`, which macOS does not publish
//! ([`nodal_core::services::listeners`]).
//!
//! A check that cannot run says so on standard output, names the claim it is not making
//! and says why. It never passes quietly. A skip nobody reads is how a suite comes to
//! cover less than its name says, so the words are printed by the test that skipped and
//! `ci/acceptance-safety.sh` shows them.

use std::process::Output;

use crate::text::{answer, stderr};

/// Whether this host publishes the table the listener scan reads.
#[must_use]
pub const fn reads_bound_ports() -> bool {
    cfg!(target_os = "linux")
}

/// Say that a claim is not being made on this host, and why. Always answers `true`, so
/// a test reads `if skipped(…) { return; }`.
#[must_use]
pub fn skipped(claim: &str, why: &str) -> bool {
    println!("SKIPPED on {}: {claim} — {why}", std::env::consts::OS);
    eprintln!("SKIPPED on {}: {claim} — {why}", std::env::consts::OS);
    true
}

/// Whether this host publishes a process table, and a word about it when it does not.
///
/// A process scan reads `/proc`, which macOS does not have. A test that needs one says
/// which claim it is not making rather than passing quietly.
#[must_use]
pub fn reads_process_table(claim: &str) -> bool {
    if cfg!(target_os = "linux") {
        return true;
    }
    eprintln!("skipped ({claim}): a process scan reads /proc, which this host does not have");
    false
}

/// Whether a reclaim moves a home without `--force` on this host.
///
/// A reclaim does not move a home while the process table is unread, because nothing
/// found standing in the home is not proof that nothing stands in it. macOS publishes no
/// table, so there a reclaim refuses and `--force` moves the home.
#[must_use]
pub const fn moves_a_home_unforced() -> bool {
    cfg!(target_os = "linux")
}

/// The words a reclaim refuses with when it did not move a home over an unread table.
pub const UNREAD_TABLE: &str = "was not moved: the process table could not be read";

/// Reclaim a unit whose home holds nothing only it has, and assert what this host does.
///
/// On Linux the reclaim goes ahead. On a host with no process table the reclaim refuses,
/// names the unread table, and the same command with `--force` goes ahead. The answer is
/// the reclaim that went ahead.
///
/// # Panics
///
/// If the host did not do what it should.
pub fn reclaim(run: impl Fn(&[&str]) -> Output, args: &[&str]) -> Output {
    if moves_a_home_unforced() {
        return run(args);
    }
    assert_unread_refusal(&run(args));
    let forced: Vec<&str> = args.iter().copied().chain(["--force"]).collect();
    run(&forced)
}

/// Merge a unit, and on a host with no process table take the home the merge left.
///
/// On Linux the merge removes the home. Elsewhere the merge lands, its remove stage
/// refuses and names the unread table, and a forced reclaim of `slug` moves the home. The
/// answer is the merge's own report, on standard output.
///
/// # Panics
///
/// If the host did not do what it should.
pub fn merge(run: impl Fn(&[&str]) -> Output, args: &[&str], slug: &str) -> Output {
    let merged = run(args);
    if moves_a_home_unforced() {
        return merged;
    }
    let report = answer(&merged);
    assert!(!merged.status.success(), "a merge whose remove was refused says so: {report}");
    assert!(report.contains(UNREAD_TABLE), "the merge names the unread table: {report}");
    let forced = run(&["reclaim", slug, "--force"]);
    assert!(forced.status.success(), "--force moves the home: {}", stderr(&forced));
    merged
}

/// Insist that a reclaim refused over the unread table and said so.
///
/// # Panics
///
/// If the command went ahead, or refused for another reason.
pub fn assert_unread_refusal(refused: &Output) {
    assert!(!refused.status.success(), "the home is not moved: {}", answer(refused));
    assert!(stderr(refused).contains(UNREAD_TABLE), "{}", stderr(refused));
}
