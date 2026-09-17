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
//! else, took the second copy away. The third proof measured that: three units went from
//! safe to refuse over a command that changed no work.
//!
//! So `--not --all` is the exclusion, and `--all` is every ref under `refs/` plus `HEAD`.
//! That covers branches, tags, the stash and Nodal's own `refs/nodal/*` records, and it
//! covers a detached `HEAD`, which is a checked-out commit and a real copy.
//!
//! A repository's own `refs/remotes/*` is inside `--all` as well, and that is not the
//! reading PR 63 refuses. A remote-tracking ref of a third repository may not prove that
//! the *remote* still holds a commit. It does prove that the third repository holds the
//! commit, because the objects behind it are in that repository's store and that store
//! is what survives the removal.
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
    use super::{args, stores_args, stranded_args};
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

    #[test]
    fn reachability_excludes_every_ref_of_the_repository() {
        let stored = [one()];
        let asked = ["rev-list", "--ignore-missing", "--no-walk", ONE, "--not", "--all"];
        assert_eq!(stranded_args(&stored), asked);
    }
}
