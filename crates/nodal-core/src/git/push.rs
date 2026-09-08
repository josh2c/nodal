//! The one thing Nodal does that leaves this machine.
//!
//! Every other Git call Nodal makes reads or writes the local object database. This one
//! sends refs to a remote, and it is deliberately alone in its own file so that the
//! answer to "what does Nodal send, and when" is one function with one caller
//! (`crate::lifecycle::ops::done`).
//!
//! It is the user's own `git`, run through the same seam as every other invocation
//! ([`cmd`]), so the credentials, the helper and the proxy settings are the ones that
//! person's own `git push` would use. Nodal holds no token and speaks no host's API.
//!
//! A refspec is given whole by the caller, including any leading `+`. Nodal forces
//! nothing outside its own `refs/nodal/` namespace: a branch that would not
//! fast-forward is a failure a person is told about, not a history Nodal overwrites.

use std::path::Path;

use super::cmd;
use crate::error::Result;

/// Send `refspecs` to `remote`.
///
/// `--no-verify` is deliberately *not* passed: a person's own pre-push hook is theirs
/// and runs. Nothing is set that a person's `git push` would not set.
///
/// # Errors
/// [`Error::GitSpawn`](crate::Error::GitSpawn) when `git` could not be started,
/// [`Error::Git`](crate::Error::Git) when the push was refused or the remote could not
/// be reached.
pub(super) fn push(repo: &Path, remote: &str, refspecs: &[String]) -> Result<()> {
    let mut args = vec!["push", "--porcelain", "--", remote];
    args.extend(refspecs.iter().map(String::as_str));
    cmd::run_ok(repo, &args)?;
    Ok(())
}

/// The refspec that sends a local ref to the same name on the remote.
#[must_use]
pub fn same_name(reference: &str) -> String {
    format!("{reference}:{reference}")
}

/// The same, for a ref Nodal owns and may move backwards.
///
/// Only [`crate::git::refs::NAMESPACE`] refs are ever given this. A work-in-progress
/// snapshot is rewritten from the working tree each time one is taken, so the new
/// commit is not a descendant of the last, and the remote copy has to be replaceable
/// for the ref to mean "what the home holds now".
#[must_use]
pub fn forced(reference: &str) -> String {
    format!("+{reference}:{reference}")
}

#[cfg(test)]
mod tests {
    use super::{forced, same_name};

    #[test]
    fn a_refspec_keeps_the_name_the_ref_already_has() {
        assert_eq!(same_name("refs/heads/topic"), "refs/heads/topic:refs/heads/topic");
        assert_eq!(forced("refs/nodal/01J/wip"), "+refs/nodal/01J/wip:refs/nodal/01J/wip");
    }
}
