//! What a host genuinely cannot do, said out loud.
//!
//! The suite runs on Linux and on macOS, and every property in it holds on both. Both
//! hosts read the process table, and both read which ports are bound
//! ([`nodal_core::services::listeners`]), so a test that reads either asserts the same
//! reading on both. No property in the suite is skipped for the host it runs on.
//!
//! A check that cannot run says so on standard output, names the claim it is not making
//! and says why. It never passes quietly. A skip nobody reads is how a suite comes to
//! cover less than its name says, so the words are printed by the test that skipped and
//! `ci/acceptance-safety.sh` shows them.

/// Say that a claim is not being made on this host, and why. Always answers `true`, so
/// a test reads `if skipped(…) { return; }`.
#[must_use]
pub fn skipped(claim: &str, why: &str) -> bool {
    println!("SKIPPED on {}: {claim} — {why}", std::env::consts::OS);
    eprintln!("SKIPPED on {}: {claim} — {why}", std::env::consts::OS);
    true
}
