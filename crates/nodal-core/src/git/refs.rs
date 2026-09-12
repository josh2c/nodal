//! Reading and writing refs, including the ref a unit's WIP snapshot lives on.

use std::path::Path;

use super::oid::Oid;
use super::{cmd, layout};
use crate::error::{Error, Result};

/// Where a repository keeps the log of each ref, under its common git directory.
const LOGS: &str = "logs";

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

/// When a ref last moved in this checkout, `None` when nothing on disk says.
///
/// This is the second half of a BEHIND reading. The count of commits is arithmetic over
/// whatever the checkout last fetched, and a checkout that last fetched three weeks ago
/// gives a count that is right about the wrong commits. Only a fetch would make the
/// count current, and Nodal makes no network call of its own, so the reading says how
/// old it is instead.
///
/// Two sources, in this order, and both are read from the files Git writes rather than
/// from a process:
///
/// 1. The last line of the ref's log, which is the instant Git recorded the last time it
///    moved the ref. This is the answer whenever the repository keeps logs, which an
///    ordinary clone does for every branch and every remote-tracking ref.
/// 2. The modification time of the loose ref file, for a repository whose logs are off.
///
/// A ref that exists only in `packed-refs` is dated by neither. `packed-refs` is
/// rewritten whenever any ref in it is packed, so its time would date this ref by
/// another ref's update, and the error would be in the direction of calling a stale
/// reading fresh. Nothing is said instead.
///
/// `reference` is named as a person names it: `origin/main`, `main`, or a full
/// `refs/...` name. The short forms are looked for where Git looks for them.
#[must_use]
pub fn last_moved(checkout: &Path, reference: &str) -> Option<crate::model::Timestamp> {
    let common = layout::common_dir(checkout)?;
    candidates(reference)
        .into_iter()
        .find_map(|name| logged(&common, &name).or_else(|| written(&common.join(&name))))
}

/// The full ref names a person's name for a ref can mean, in Git's own order.
fn candidates(reference: &str) -> Vec<String> {
    if reference.starts_with("refs/") {
        return vec![reference.to_owned()];
    }
    ["refs/heads/", "refs/remotes/", "refs/tags/"]
        .iter()
        .map(|prefix| format!("{prefix}{reference}"))
        .collect()
}

/// The instant the last line of a ref's log records, `None` when there is no log.
fn logged(common: &Path, name: &str) -> Option<crate::model::Timestamp> {
    let text = std::fs::read_to_string(common.join(LOGS).join(name)).ok()?;
    let last = text.lines().rev().find(|line| !line.trim().is_empty())?;
    moment(last)
}

/// The instant one line of a ref's log records.
///
/// A line is `<old> <new> <who> <email> <seconds> <zone>`, a tab, and the reason. The
/// name and the address of whoever moved it may hold spaces, so the two fields are taken
/// from the end of the line rather than counted from its start.
fn moment(line: &str) -> Option<crate::model::Timestamp> {
    let head = line.split('\t').next().unwrap_or(line);
    let mut fields = head.split_whitespace().rev();
    let _zone = fields.next()?;
    let seconds: i64 = fields.next()?.parse().ok()?;
    crate::model::Timestamp::from_unix_seconds(seconds).ok()
}

/// When a file was last written, `None` when it is not there or has no time.
fn written(path: &Path) -> Option<crate::model::Timestamp> {
    let modified = std::fs::symlink_metadata(path).ok()?.modified().ok()?;
    let seconds = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    crate::model::Timestamp::from_unix_seconds(i64::try_from(seconds).ok()?).ok()
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
    use std::path::Path;

    use super::{last_moved, moment, wip};

    /// A reflog line as Git writes one, with a name and an address that hold spaces.
    const LINE: &str =
        "dc3f89c fab8002 A Person With Spaces <a@b.invalid> 1787208926 -0700\tupdate by push";

    /// A repository with one ref and one log line for it, laid out as Git lays one out.
    fn repository(at: &Path, name: &str, log: &str) {
        let logs = at.join(".git/logs").join(name);
        std::fs::create_dir_all(logs.parent().unwrap()).unwrap();
        std::fs::write(logs, log).unwrap();
    }

    #[test]
    fn wip_refs_live_in_the_nodal_namespace() {
        assert_eq!(wip("01JABC"), "refs/nodal/01JABC/wip");
    }

    /// The name and the address of whoever moved a ref may hold spaces, so the instant is
    /// taken from the end of the line. Counting fields from the start read a name.
    #[test]
    fn the_instant_is_taken_from_the_end_of_a_reflog_line() {
        assert_eq!(moment(LINE).unwrap().unix_seconds(), 1_787_208_926);
    }

    #[test]
    fn a_line_that_is_not_a_reflog_line_dates_nothing() {
        assert_eq!(moment(""), None);
        assert_eq!(moment("dc3f89c fab8002 someone <a@b.invalid> not-a-time -0700"), None);
    }

    /// The last line, not the first: a ref that moved four times was last moved by the
    /// fourth.
    #[test]
    fn a_ref_is_dated_by_the_last_line_of_its_log() {
        let at = tempfile::tempdir().unwrap();
        let old = LINE.replace("1787208926", "1700000000");
        repository(at.path(), "refs/remotes/origin/main", &format!("{old}\n{LINE}\n"));
        let moved = last_moved(at.path(), "origin/main").unwrap();
        assert_eq!(moved.unix_seconds(), 1_787_208_926);
    }

    /// A short name is looked for where Git looks for it, so a branch and a
    /// remote-tracking ref are both found by the name a person uses for them.
    #[test]
    fn a_short_name_finds_the_ref_it_names() {
        let at = tempfile::tempdir().unwrap();
        repository(at.path(), "refs/heads/main", &format!("{LINE}\n"));
        assert!(last_moved(at.path(), "main").is_some());
        assert_eq!(last_moved(at.path(), "origin/main"), None);
    }

    /// A directory that is no repository, and a ref with neither a log nor a loose file,
    /// each date nothing. Neither is an error: the reading they feed says nothing about
    /// an age rather than guessing at one.
    #[test]
    fn a_ref_with_nothing_on_disk_to_date_it_dates_nothing() {
        let at = tempfile::tempdir().unwrap();
        assert_eq!(last_moved(at.path(), "origin/main"), None);
        std::fs::create_dir_all(at.path().join(".git")).unwrap();
        assert_eq!(last_moved(at.path(), "origin/main"), None);
    }

    /// A repository whose logs are off is dated by the ref file itself, which is written
    /// each time the ref moves.
    #[test]
    fn a_repository_with_no_logs_is_dated_by_the_ref_file() {
        let at = tempfile::tempdir().unwrap();
        let ref_path = at.path().join(".git/refs/remotes/origin/main");
        std::fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
        std::fs::write(&ref_path, "fab8002737b38fcb08da4da731a80a0a3c370b43\n").unwrap();
        assert!(last_moved(at.path(), "origin/main").is_some());
    }

    /// A reading taken in a linked worktree is the reading taken in the checkout. The
    /// refs are shared, so the file is found through `commondir` and not beside the
    /// worktree's own git directory, where there is nothing to find.
    #[test]
    fn a_linked_worktree_is_dated_by_the_refs_it_shares() {
        let at = tempfile::tempdir().unwrap();
        let checkout = at.path().join("checkout");
        repository(&checkout, "refs/remotes/origin/main", &format!("{LINE}\n"));
        let linked = checkout.join(".git/worktrees/w1");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join("commondir"), "../..\n").unwrap();
        let worktree = at.path().join("w1");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(worktree.join(".git"), format!("gitdir: {}\n", linked.display())).unwrap();
        assert_eq!(last_moved(&worktree, "origin/main"), last_moved(&checkout, "origin/main"));
        assert!(last_moved(&worktree, "origin/main").is_some());
    }
}
