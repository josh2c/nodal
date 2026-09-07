//! Reading and writing refs, including the ref a unit's WIP snapshot lives on.

use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::{Error, Result};

/// Where Nodal keeps its own refs inside a unit's repository.
pub const NAMESPACE: &str = "refs/nodal/";

/// A ref and what it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Full ref name, for example `refs/nodal/01J.../wip`.
    pub name: String,
    /// The object the ref points at.
    pub oid: Oid,
}

/// The ref a unit's work-in-progress snapshot is written to (T2.3 writes it).
#[must_use]
pub fn wip(unit_id: &str) -> String {
    format!("{NAMESPACE}{unit_id}/wip")
}

/// Read a full ref name, returning `None` when it does not exist.
///
/// # Errors
/// [`Error::Git`] when `git` failed for a reason other than a missing ref.
pub(super) fn read(repo: &Path, name: &str) -> Result<Option<Oid>> {
    let output = cmd::run(repo, &["show-ref", "--verify", "--", name])?;
    if !output.ok() {
        return Ok(None);
    }
    let text = output.text()?;
    let (oid, _) = text
        .split_once(' ')
        .ok_or_else(|| Error::GitParse { args: output.args.clone(), record: text.to_owned() })?;
    Ok(Some(Oid::parse(oid)?))
}

/// Point a ref at an object, creating it if needed. Idempotent.
///
/// # Errors
/// [`Error::Git`] when `git update-ref` refused the name or the object.
pub(super) fn write(repo: &Path, name: &str, oid: &Oid, reason: &str) -> Result<()> {
    cmd::run_ok(repo, &["update-ref", "-m", reason, name, oid.as_str()])?;
    Ok(())
}

/// Delete a ref. Deleting a ref that does not exist succeeds.
///
/// # Errors
/// [`Error::Git`] when `git update-ref -d` failed.
pub(super) fn delete(repo: &Path, name: &str) -> Result<()> {
    cmd::run_ok(repo, &["update-ref", "-d", name])?;
    Ok(())
}

/// List every ref under a prefix, sorted by name.
///
/// # Errors
/// [`Error::Git`] when `git for-each-ref` failed, [`Error::GitParse`] on an unreadable
/// record.
pub(super) fn list(repo: &Path, prefix: &str) -> Result<Vec<Ref>> {
    let output = cmd::run_ok(
        repo,
        &["for-each-ref", "--sort=refname", "--format=%(objectname) %(refname)", prefix],
    )?;
    output
        .lines()?
        .iter()
        .map(|line| {
            let (oid, name) = line.split_once(' ').ok_or_else(|| Error::GitParse {
                args: output.args.clone(),
                record: (*line).to_owned(),
            })?;
            Ok(Ref { name: name.to_owned(), oid: Oid::parse(oid)? })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::wip;

    #[test]
    fn wip_refs_live_in_the_nodal_namespace() {
        assert_eq!(wip("01JABC"), "refs/nodal/01JABC/wip");
    }
}
