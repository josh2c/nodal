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
