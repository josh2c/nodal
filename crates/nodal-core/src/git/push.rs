//! The one thing Nodal does that leaves this machine.
//!
//! Every other Git call Nodal makes reads or writes the local object database. These
//! three reach a remote, and they are deliberately alone in this file so that the
//! answer to "what leaves this machine, and when" is one file with two callers.
//!
//! [`push`] sends refs, and `crate::lifecycle::ops::done` is the only caller. [`list`]
//! and [`delete`] are the reclaim's: it asks the remote which of Nodal's own refs are
//! there and deletes those, and `crate::lifecycle::ops::reclaim` is their only caller.
//! Nothing here ever names a branch it did not read from the caller.
//!
//! It is the user's own `git`, run through the same seam as every other invocation
//! ([`cmd`]), so the credentials, the helper and the proxy settings are the ones that
//! person's own `git push` would use. Nodal holds no token and speaks no host's API.
//!
//! A refspec is given whole by the caller, including any leading `+`. Nodal forces
//! nothing outside its own `refs/nodal/` namespace: a branch that would not
//! fast-forward is a failure a person is told about, not a history Nodal overwrites.
//! A deletion is refused the same way: [`delete`] takes only names under
//! [`crate::git::refs::NAMESPACE`], so no call here can remove a branch.

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

/// The refs a remote holds under a prefix, in full, as the remote names them.
///
/// One `git ls-remote`, which reads and sends nothing. A remote that cannot be reached
/// is an error the caller turns into a note.
///
/// # Errors
/// [`Error::GitSpawn`](crate::Error::GitSpawn) when `git` could not be started,
/// [`Error::Git`](crate::Error::Git) when the remote refused or could not be reached.
pub(super) fn list(repo: &Path, remote: &str, prefix: &str) -> Result<Vec<String>> {
    let pattern = format!("{prefix}*");
    let output = cmd::run_ok(repo, &["ls-remote", "--refs", "--", remote, &pattern])?;
    Ok(output
        .lines()?
        .iter()
        .filter_map(|line| line.split_once('\t').map(|(_, name)| name.trim().to_owned()))
        .filter(|name| name.starts_with(prefix))
        .collect())
}

/// Delete refs on a remote, and only ones Nodal owns.
///
/// The filter is the safety property, not a tidiness: a name outside
/// [`crate::git::refs::NAMESPACE`] is dropped rather than sent, so a caller that passed
/// `refs/heads/main` deletes nothing. Given nothing left to send, nothing is run.
///
/// # Errors
/// [`Error::GitSpawn`](crate::Error::GitSpawn) when `git` could not be started,
/// [`Error::Git`](crate::Error::Git) when the remote refused the deletion.
pub(super) fn delete(repo: &Path, remote: &str, names: &[String]) -> Result<Vec<String>> {
    let ours = owned(names);
    if ours.is_empty() {
        return Ok(ours);
    }
    let mut args = vec!["push", "--porcelain", "--delete", "--", remote];
    args.extend(ours.iter().map(String::as_str));
    cmd::run_ok(repo, &args)?;
    Ok(ours)
}

/// The names of `names` that are Nodal's own to delete.
fn owned(names: &[String]) -> Vec<String> {
    names.iter().filter(|name| name.starts_with(super::refs::NAMESPACE)).cloned().collect()
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
    use super::{forced, owned, same_name};

    #[test]
    fn a_deletion_drops_every_name_that_is_not_nodals_own() {
        let asked = [
            String::from("refs/nodal/01J/wip"),
            String::from("refs/heads/main"),
            String::from("refs/heads/nodal/topic"),
            String::from("refs/tags/v1"),
        ];
        assert_eq!(owned(&asked), [String::from("refs/nodal/01J/wip")]);
    }

    #[test]
    fn a_refspec_keeps_the_name_the_ref_already_has() {
        assert_eq!(same_name("refs/heads/topic"), "refs/heads/topic:refs/heads/topic");
        assert_eq!(forced("refs/nodal/01J/wip"), "+refs/nodal/01J/wip:refs/nodal/01J/wip");
    }
}
