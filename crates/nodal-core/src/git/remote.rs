//! Whether a unit's commits exist anywhere but this machine.
//!
//! `nodal reclaim` refuses to remove work that exists only on this machine; this is the
//! Git half of that answer.

use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::Result;

/// How much of a revision the remotes already have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Containment {
    /// The remotes configured in this repository.
    pub remotes: Vec<String>,
    /// Commits reachable from the revision and from no remote-tracking ref, newest first.
    pub unpushed: Vec<Oid>,
}

impl Containment {
    /// Whether every commit of the revision is on at least one remote.
    #[must_use]
    pub fn is_contained(&self) -> bool {
        self.unpushed.is_empty()
    }
}

/// Compute containment for `rev` against every remote-tracking ref.
///
/// # Errors
/// [`Error::Git`](crate::Error::Git) when the revision is unknown, [`Error::GitOid`](crate::Error::GitOid)
/// on unreadable output.
pub(super) fn containment(repo: &Path, rev: &str) -> Result<Containment> {
    let remotes =
        cmd::run_ok(repo, &["remote"])?.lines()?.iter().map(|r| (*r).to_owned()).collect();
    let listed = cmd::run_ok(repo, &["rev-list", rev, "--not", "--remotes"])?;
    let unpushed = listed.lines()?.iter().map(|line| Oid::parse(line)).collect::<Result<_>>()?;
    Ok(Containment { remotes, unpushed })
}
