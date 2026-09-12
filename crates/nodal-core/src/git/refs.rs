//! Reading and writing refs, including the ref a unit's WIP snapshot lives on.

use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::{Error, Result};

/// Where Nodal keeps its own refs inside a unit's repository.
pub const NAMESPACE: &str = "refs/nodal/";

/// Where a home keeps the copy it took of the person's own checkout's branches.
///
/// A home is cloned from a base, and a base is a clone of the remote. Neither of them
/// has the branches the person made in their own checkout, and `--from` names one of
/// those as often as it names a branch the remote carries. So the create copies them
/// here, under a namespace of Nodal's own, rather than into `refs/heads/`, where they
/// would look to the person's `git` like branches of the unit's own repository.
pub const CHECKOUT: &str = "refs/nodal/checkout/";

/// Where a home keeps the copy it took of what the person's checkout knows of `origin`.
///
/// Not `refs/remotes/origin/`, and the distinction is the whole of this constant. A
/// home has an `origin` of its own, it pushes to it, and `refs/remotes/origin/*` is its
/// own record of what it has sent — which is what decides whether a unit's work exists
/// anywhere but this machine ([`super::remote::containment`]). Writing the checkout's
/// reading over that would answer a question about the remote with a reading taken
/// somewhere else, in a namespace whose meaning several other operations depend on.
///
/// So the copy lives beside it, under a name that says whose reading it is.
pub const ORIGIN: &str = "refs/nodal/origin/";

/// The refspec that brings a checkout's reading of `origin` in, under [`ORIGIN`].
///
/// Forced, because the point of copying is that the checkout's copy is newer than the
/// one the base was built with, and a fast-forward rule would refuse exactly the case
/// where the remote branch was rewritten.
pub const MIRROR_ORIGIN: &str = "+refs/remotes/origin/*:refs/nodal/origin/*";

/// The refspec that brings a checkout's own branches in, under [`CHECKOUT`].
pub const MIRROR_HEADS: &str = "+refs/heads/*:refs/nodal/checkout/*";

/// A ref and what it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    /// Full ref name, for example `refs/nodal/01J.../wip`.
    pub name: String,
    /// The object the ref points at.
    pub oid: Oid,
}

/// The ref a unit's work-in-progress snapshot is written to.
#[must_use]
pub fn wip(unit_id: &str) -> String {
    format!("{NAMESPACE}{unit_id}/wip")
}

/// The ref that holds a unit's branch as it was before a merge squashed it.
///
/// The one place the commits a squash folded stay reachable. It is written before the
/// squash and never overwritten, it travels to the trash with the home, and `nodal gc`
/// removing that home is what finally lets go of it.
#[must_use]
pub fn premerge(unit_id: &str) -> String {
    format!("{NAMESPACE}{unit_id}/premerge")
}

/// The ref a merge fetches the branch it merges into onto.
#[must_use]
pub fn target(unit_id: &str) -> String {
    format!("{NAMESPACE}{unit_id}/target")
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

/// What a symbolic ref points at, in full, `None` when there is no such ref.
///
/// `refs/remotes/origin/HEAD` is the one a project's default branch is read from: a
/// clone records it, and it is what the remote said its default branch was.
///
/// # Errors
/// [`Error::GitEncoding`] when the answer is not UTF-8.
pub(super) fn symbolic(repo: &Path, name: &str) -> Result<Option<String>> {
    let output = cmd::run(repo, &["symbolic-ref", "--quiet", "--", name])?;
    if !output.ok() {
        return Ok(None);
    }
    Ok(Some(output.text()?.to_owned()))
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
    for_each_ref(repo, Some(prefix))
}

/// Every ref the repository has, sorted by name.
///
/// One process for the whole repository, and the reading behind the uniqueness proof:
/// a ref tip is a commit this clone's object store holds, whatever the ref is called.
///
/// # Errors
/// [`Error::Git`] when `git for-each-ref` failed, [`Error::GitParse`] on an unreadable
/// record.
pub(super) fn all(repo: &Path) -> Result<Vec<Ref>> {
    for_each_ref(repo, None)
}

/// One `for-each-ref`, with a pattern or over everything.
fn for_each_ref(repo: &Path, prefix: Option<&str>) -> Result<Vec<Ref>> {
    let mut args = vec!["for-each-ref", "--sort=refname", "--format=%(objectname) %(refname)"];
    args.extend(prefix);
    let output = cmd::run_ok(repo, &args)?;
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
