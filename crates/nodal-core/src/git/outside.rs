//! How much of a revision no named commit already holds.
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
//! Nothing here writes. `rev-list` reads objects, and the facade runs it with
//! `GIT_OPTIONAL_LOCKS=0`.

use std::path::Path;

use super::cmd;
use super::oid::Oid;
use crate::error::{Error, Result};

/// Which of `wanted` this repository has, without walking any history.
///
/// `--no-walk` makes `rev-list` print the revisions it was given rather than the history
/// behind them, and `--ignore-missing` drops the ones this repository does not have. So
/// what comes back is the subset it holds, in one process and at no traversal cost.
///
/// # Errors
/// [`Error::Git`] when `rev-list` failed, [`Error::GitOid`] on unreadable output.
pub(super) fn held(repo: &Path, wanted: &[Oid]) -> Result<Vec<Oid>> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let mut args = vec!["rev-list", "--ignore-missing", "--no-walk"];
    args.extend(wanted.iter().map(Oid::as_str));
    let output = cmd::run_ok(repo, &args)?;
    output.lines()?.iter().map(|line| Oid::parse(line)).collect()
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
    let output = cmd::run_ok(repo, &args)?;
    output.lines()?.iter().map(|line| Oid::parse(line)).collect()
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
    let output = cmd::run_ok(repo, &args(rev, held))?;
    output.lines()?.iter().map(|line| Oid::parse(line)).collect()
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

/// The argument list both questions share.
fn args<'a>(rev: &'a str, held: &'a [Oid]) -> Vec<&'a str> {
    let mut args = vec!["rev-list", "--ignore-missing", rev];
    if !held.is_empty() {
        args.push("--not");
        args.extend(held.iter().map(Oid::as_str));
    }
    args
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use super::args;
    use crate::git::oid::Oid;

    const ONE: &str = "1e2f3a4b5c6d7e8f90112233445566778899aabb";

    #[test]
    fn nothing_held_asks_for_the_whole_revision() {
        assert_eq!(args("HEAD", &[]), ["rev-list", "--ignore-missing", "HEAD"]);
    }

    #[test]
    fn held_commits_follow_a_single_not() {
        let held = [Oid::parse(ONE).expect("a well formed id")];
        assert_eq!(args("HEAD", &held), ["rev-list", "--ignore-missing", "HEAD", "--not", ONE]);
    }
}
