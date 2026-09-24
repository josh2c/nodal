//! How much of a revision no named commit already holds, and what a repository holds.
//!
//! `git rev-list <rev> --not <commits>` is the only question a uniqueness proof asks of
//! Git, and it asks it against commits the caller names rather than against a namespace.
//! That distinction is the point of this module. `--not --remotes` names a namespace,
//! `refs/remotes/`, which a clone writes to itself when it fetches or pushes and never
//! corrects afterwards; a clone that pushed a branch once answers "nothing unpushed"
//! about that branch for the rest of its life, whatever the remote did later.
//!
//! So the caller decides what counts as held, and this module counts what is left.
//!
//! # A second copy is a commit a ref reaches, and never an object that is there
//!
//! [`held`] answers the other half: which commits a second repository on this machine
//! holds. It asks for reachability from that repository's own refs, and not for the
//! object being in its store, because the two come apart and the difference decides a
//! removal.
//!
//! An object no ref reaches is what `git gc` removes. A commit a repository fetched by
//! identifier, a commit left behind by a deleted branch, and a commit written into a
//! repository by `git fetch <url> HEAD` all sit in the object store under no name. A
//! reading that counted them called a unit home safe to remove because a second copy
//! existed, and one `git gc --prune=now` in that other repository, which touches nothing
//! else, took the second copy away. That was measured: one such collection moved three
//! units from safe to refuse, over a command that changed no work.
//!
//! So `--not --all` is the exclusion, and `--all` is every ref under `refs/` plus `HEAD`.
//! That covers branches, tags, the stash and Nodal's own `refs/nodal/*` records, and it
//! covers a detached `HEAD`, which is a checked-out commit and a real copy.
//!
//! A repository's own `refs/remotes/*` is inside `--all` as well, and that is a
//! different reading from the one this module refuses at the top. A remote-tracking ref
//! of a third repository may not prove that the *remote* still holds a commit. It does
//! prove that the third repository holds the commit. The objects behind it are in that
//! repository's store, and that store is what survives the removal.
//!
//! Nothing here writes. `rev-list` reads objects, and the facade runs it with
//! `GIT_OPTIONAL_LOCKS=0`.

use std::collections::BTreeSet;
use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::{Error, Result};

/// Which of `wanted` this repository reaches from a ref of its own.
///
/// Two processes at worst and one in the ordinary case. The first asks which of `wanted`
/// the object store has at all, and a repository that has none of them is finished
/// there — which is what an unrelated repository beside the checkout answers. The second
/// asks which of the ones it has no ref reaches, and those are dropped.
///
/// The order is the safe one. `rev-list --ignore-missing` drops an identifier this
/// repository does not have, so the exclusion question alone cannot tell a missing
/// commit from a reachable one, and reading the second as the first is the false safe
/// this function exists to remove.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitOid`] on unreadable output.
pub(super) fn held(repo: &Path, wanted: &[Oid]) -> Result<Vec<Oid>> {
    let stored = stores(repo, wanted)?;
    if stored.is_empty() {
        return Ok(stored);
    }
    let stranded: BTreeSet<Oid> = stranded(repo, &stored)?.into_iter().collect();
    Ok(stored.into_iter().filter(|oid| !stranded.contains(oid)).collect())
}

/// Which of `wanted` this repository's object store has, without walking any history.
///
/// `--no-walk` makes `rev-list` print the revisions it was given rather than the history
/// behind them, and `--ignore-missing` drops the ones this repository does not have. So
/// what comes back is the subset the store has, in one process and at no traversal cost.
///
/// This is not the question "does another copy of this work exist": an object no ref
/// reaches is one `git gc` removes, and [`held`] is the question a removal rests on. Two
/// callers ask this one, and each knows why the answer is enough where it stands.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitOid`] on unreadable output.
pub(super) fn stores(repo: &Path, wanted: &[Oid]) -> Result<Vec<Oid>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    read(repo, &stores_args(wanted))
}

/// Which of `stored` no ref of this repository reaches.
///
/// `--ignore-missing` covers the refs as well as the arguments. A ref that names an
/// object the store does not have is the shape a pruned or half-written store leaves,
/// and it reaches nothing; without the flag one such ref stops the whole reading and the
/// repository proves nothing at all.
fn stranded(repo: &Path, stored: &[Oid]) -> Result<Vec<Oid>> {
    read(repo, &stranded_args(stored))
}

/// Which of `wanted` this repository reaches from a ref it keeps of its own accord.
///
/// [`held`] with one namespace left out, and the namespace decides a removal.
/// `refs/remotes/` inside a repository is that repository's record of a fetch or a push
/// it made, and `git fetch --prune` deletes one the moment the remote drops the branch —
/// exactly as `git gc` deletes an object under no ref. A commit a store holds only under
/// such a ref is therefore a copy one ordinary command takes away, and a verdict that
/// rested a fourteen-day trash timer on it rested it on the weakest ref there is.
///
/// What is left is a ref the store keeps because somebody there wanted it kept: a branch,
/// a tag, the stash, a detached `HEAD`. That is what a second copy means.
///
/// The remote question about such a ref is a different question and it has its own
/// answer: a dated observation of the remote ([`crate::doctor::unique::believed`]). This
/// reading does not try to answer it and does not stand in for it.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitOid`] on unreadable output.
pub(super) fn owned(repo: &Path, wanted: &[Oid]) -> Result<Vec<Oid>> {
    let stored = stores(repo, wanted)?;
    if stored.is_empty() {
        return Ok(stored);
    }
    let elsewhere: BTreeSet<Oid> = read(repo, &owned_args(&stored))?.into_iter().collect();
    Ok(stored.into_iter().filter(|oid| !elsewhere.contains(oid)).collect())
}

/// How many objects behind `commits` this repository has not got.
///
/// A commit being here is not the work being here. The work is the trees and the blobs
/// the commit names, and a partial clone holds every commit and none of them. So the
/// commits are walked for their objects, and `--missing=print` prints one line beginning
/// with a question mark for each object the walk wanted and could not find.
///
/// `boundary` bounds the walk. It is the parents of `commits` that are not themselves in
/// `commits`, so what is walked is the objects those commits introduce and not the whole
/// history behind them. A root commit has no boundary, and then the walk is of that one
/// tree.
///
/// `--ignore-missing` covers the boundary as well as the commits, because a store that
/// holds the work need not hold the history under it, and a boundary it has not got would
/// otherwise stop the reading rather than widen it.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitEncoding`] on unreadable output.
pub(super) fn missing_objects(repo: &Path, commits: &[Oid], boundary: &[Oid]) -> Result<usize> {
    if commits.is_empty() {
        return Ok(0);
    }
    let output = cmd::run_ok(repo, &missing_args(commits, boundary))?;
    Ok(output.lines()?.iter().filter(|line| line.starts_with(MISSING)).count())
}

/// What `rev-list --missing=print` writes before an object it could not find.
const MISSING: char = '?';

/// Whether this repository fetches the objects it has not got, rather than holding them.
///
/// One `git config --get-regexp`, which exits non-zero when nothing matches. A promisor
/// remote and `extensions.partialClone` are the two ways a repository says it is partial,
/// and either is enough.
///
/// # Errors
/// [`Error::GitSpawn`] when `git` could not be started.
pub(super) fn partial(repo: &Path) -> Result<bool> {
    Ok(cmd::run(repo, &["config", "--get-regexp", "--", PARTIAL])?.ok())
}

/// The two settings a partial clone writes, either of which says it is one.
const PARTIAL: &str = "^(remote\\..*\\.promisor|extensions\\.partialclone)$";

/// The parents of `commits` that none of `commits` is, which is where an object walk of
/// them stops.
///
/// One `rev-list --parents --no-walk`, whose every line is a commit and then its parents.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitOid`] on unreadable output.
pub(super) fn boundary_of(repo: &Path, commits: &[Oid]) -> Result<Vec<Oid>> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["rev-list", "--ignore-missing", "--parents", "--no-walk"];
    args.extend(commits.iter().map(Oid::as_str));
    let output = cmd::run_ok(repo, &args)?;
    let inside: BTreeSet<&Oid> = commits.iter().collect();
    let mut found: Vec<Oid> = Vec::new();
    for line in output.lines()? {
        for field in line.split_whitespace().skip(1) {
            let parent = Oid::parse(field)?;
            if !inside.contains(&parent) {
                found.push(parent);
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

/// Commits of any of `revs` that none of `held` reaches, newest first.
///
/// The counterpart of [`commits`] for a caller asking about many revisions at once. One
/// process answers for all of them, and a revision that comes back in the answer is one
/// nothing in `held` reaches.
///
/// # Errors
/// As [`commits`].
pub(super) fn among(repo: &Path, revs: &[Oid], held: &[Oid]) -> Result<Vec<Oid>> {
    if revs.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["rev-list", "--ignore-missing"];
    args.extend(revs.iter().map(Oid::as_str));
    if !held.is_empty() {
        args.push("--not");
        args.extend(held.iter().map(Oid::as_str));
    }
    read(repo, &args)
}

/// Commits of `rev` that none of `held` reaches, newest first.
///
/// An id in `held` that this repository does not have is ignored rather than refused:
/// the caller collects tips from every copy on the machine, and a copy made later holds
/// commits this one never had. `--ignore-missing` comes first so Git has the rule before
/// it reads the ids.
///
/// # Errors
/// [`Error::Git`] when `rev` is unknown, [`Error::GitOid`] on unreadable output.
pub(super) fn commits(repo: &Path, rev: &str, held: &[Oid]) -> Result<Vec<Oid>> {
    read(repo, &args(rev, held))
}

/// How many commits of `rev` none of `held` reaches.
///
/// # Errors
/// As [`commits`], and [`Error::GitParse`] when the count could not be read.
pub(super) fn count(repo: &Path, rev: &str, held: &[Oid]) -> Result<usize> {
    let mut list = args(rev, held);
    list.insert(1, "--count");
    let output = cmd::run_ok(repo, &list)?;
    let text = output.text()?;
    text.trim()
        .parse()
        .map_err(|_| Error::GitParse { args: output.args.clone(), record: text.to_owned() })
}

/// Run one `rev-list` and read its lines as commit identifiers.
fn read(repo: &Path, args: &[&str]) -> Result<Vec<Oid>> {
    let output = cmd::run_ok(repo, args)?;
    output.lines()?.iter().map(|line| Oid::parse(line)).collect()
}

/// The argument list both questions about one revision share.
fn args<'a>(rev: &'a str, held: &'a [Oid]) -> Vec<&'a str> {
    let mut args = vec!["rev-list", "--ignore-missing", rev];
    if !held.is_empty() {
        args.push("--not");
        args.extend(held.iter().map(Oid::as_str));
    }
    args
}

/// The argument list that asks which identifiers the object store has.
fn stores_args(wanted: &[Oid]) -> Vec<&str> {
    let mut args = vec!["rev-list", "--ignore-missing", "--no-walk"];
    args.extend(wanted.iter().map(Oid::as_str));
    args
}

/// The argument list that asks which of them no ref of the store's own reaches.
///
/// `--exclude` applies to the `--all` that follows it, so the exclusion is every ref
/// under `refs/` and `HEAD`, less this repository's reading of somewhere else.
fn owned_args(stored: &[Oid]) -> Vec<&str> {
    let mut args = vec!["rev-list", "--ignore-missing", "--no-walk"];
    args.extend(stored.iter().map(Oid::as_str));
    args.extend(["--not", "--exclude=refs/remotes/*", "--all"]);
    args
}

/// The argument list that asks which objects behind these commits are not here.
fn missing_args<'a>(commits: &'a [Oid], boundary: &'a [Oid]) -> Vec<&'a str> {
    let mut args = vec!["rev-list", "--ignore-missing", "--objects", "--missing=print"];
    args.extend(commits.iter().map(Oid::as_str));
    if !boundary.is_empty() {
        args.push("--not");
        args.extend(boundary.iter().map(Oid::as_str));
    }
    args
}

/// The argument list that asks which of them no ref reaches.
fn stranded_args(stored: &[Oid]) -> Vec<&str> {
    let mut args = vec!["rev-list", "--ignore-missing", "--no-walk"];
    args.extend(stored.iter().map(Oid::as_str));
    args.push("--not");
    args.push("--all");
    args
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use super::{args, missing_args, owned_args, stores_args, stranded_args};
    use crate::git::oid::Oid;

    const ONE: &str = "1e2f3a4b5c6d7e8f90112233445566778899aabb";

    /// A well formed identifier for the argument-shape tests.
    fn one() -> Oid {
        Oid::parse(ONE).expect("a well formed id")
    }

    #[test]
    fn nothing_held_asks_for_the_whole_revision() {
        assert_eq!(args("HEAD", &[]), ["rev-list", "--ignore-missing", "HEAD"]);
    }

    #[test]
    fn held_commits_follow_a_single_not() {
        let held = [one()];
        assert_eq!(args("HEAD", &held), ["rev-list", "--ignore-missing", "HEAD", "--not", ONE]);
    }

    #[test]
    fn a_store_reading_walks_nothing_and_drops_what_is_missing() {
        let wanted = [one()];
        assert_eq!(stores_args(&wanted), ["rev-list", "--ignore-missing", "--no-walk", ONE]);
    }

    /// A repository's own reading of a remote is not a ref it keeps of its own accord,
    /// and `--exclude` is what takes it out of the exclusion the answer rests on.
    #[test]
    fn a_durable_copy_excludes_the_stores_reading_of_a_remote() {
        let stored = [one()];
        let asked = [
            "rev-list",
            "--ignore-missing",
            "--no-walk",
            ONE,
            "--not",
            "--exclude=refs/remotes/*",
            "--all",
        ];
        assert_eq!(owned_args(&stored), asked);
    }

    /// The object walk is bounded by the parents the commits share with the history
    /// behind them, so what it asks for is what those commits add.
    #[test]
    fn an_object_walk_stops_at_the_boundary_it_is_given() {
        let commits = [one()];
        let bounded = missing_args(&commits, &commits);
        assert_eq!(bounded[..4], ["rev-list", "--ignore-missing", "--objects", "--missing=print"]);
        assert_eq!(bounded[4..], [ONE, "--not", ONE]);
        assert_eq!(missing_args(&commits, &[]).len(), 5, "no boundary asks for no exclusion");
    }

    #[test]
    fn reachability_excludes_every_ref_of_the_repository() {
        let stored = [one()];
        let asked = ["rev-list", "--ignore-missing", "--no-walk", ONE, "--not", "--all"];
        assert_eq!(stranded_args(&stored), asked);
    }
}
