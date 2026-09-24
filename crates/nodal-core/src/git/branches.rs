//! What a repository's own refs say about its local branches.
//!
//! This is the reading half of the branch audit `nodal doctor` prints. It answers three
//! questions about every `refs/heads/` ref, and it answers them in as few processes as
//! the questions allow, because a machine that has been worked on for a year holds
//! hundreds of branches:
//!
//! | question | read from | processes |
//! |---|---|---|
//! | what branches are there, how old, what upstream | `git for-each-ref` | one, for all of them |
//! | which of them the default branch already holds | `git for-each-ref --merged` | one, for all of them |
//!
//! It answered a third — how many commits of one branch exist on no remote — from
//! `rev-list --count <rev> --not --remotes`, and that reading is gone. `--not --remotes` names
//! a namespace rather than the refs it rested on, and `refs/remotes/` inside a repository is
//! its record of its own pushes that nothing corrects, so the count was printed under a word
//! that claimed the remote. The audit takes the count against what this
//! checkout has seen on a remote instead, against the refs it names rather than a namespace
//! ([`crate::git::Git::seen_on_remotes`]), and the report's own word says what that reading is
//! worth. It is still one `rev-list` per branch ([`crate::git::Git::count_outside`]). A
//! measured machine answered all three for 317 refs in about twenty seconds.
//!
//! Nothing here writes. `for-each-ref` and `rev-list` read refs and objects, and the
//! facade runs every one of them with `GIT_OPTIONAL_LOCKS=0`, so not even the index is
//! refreshed.

use std::collections::BTreeSet;
use std::path::Path;

use super::cmd;
use crate::error::{Error, Result};

/// What `git for-each-ref` is asked for, in the order the fields are read back.
const FORMAT: &str =
    "%(refname:short)%00%(committerdate:unix)%00%(upstream:short)%00%(upstream:track)";

/// What Git prints in `upstream:track` for a branch whose upstream is not there.
const GONE: &str = "gone";

/// One local branch, as the repository's refs state it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    /// The short name, without `refs/heads/`.
    pub name: String,
    /// When the commit at its tip was made, in whole seconds since the epoch.
    pub committed: i64,
    /// The remote-tracking branch it is set to follow, `None` when it follows none.
    pub upstream: Option<String>,
    /// Whether the upstream it names is not there any more.
    ///
    /// A branch whose upstream was deleted is the shape a `fetch --prune` produces
    /// after somebody removed the remote branch. It is the one case where a person's
    /// own tool has already stopped telling them where the work is.
    pub upstream_gone: bool,
}

/// Every local branch, with the facts one `for-each-ref` can state about it.
///
/// # Errors
/// [`Error::Git`] when `git for-each-ref` failed, [`Error::GitParse`] on a record with
/// the wrong number of fields.
pub(super) fn locals(repo: &Path) -> Result<Vec<Local>> {
    let format = format!("--format={FORMAT}");
    let output = cmd::run_ok(repo, &["for-each-ref", "--sort=refname", &format, "refs/heads/"])?;
    output.lines()?.iter().map(|line| one(&output.args, line)).collect()
}

/// One record of [`FORMAT`] as a branch.
fn one(args: &[String], line: &str) -> Result<Local> {
    let fields: Vec<&str> = line.split('\0').collect();
    let [name, committed, upstream, track] = fields.as_slice() else {
        return Err(Error::GitParse { args: args.to_vec(), record: line.to_owned() });
    };
    Ok(Local {
        name: (*name).to_owned(),
        committed: committed.parse().unwrap_or_default(),
        upstream: (!upstream.is_empty()).then(|| (*upstream).to_owned()),
        upstream_gone: track.contains(GONE),
    })
}

/// The names of every local branch `base` already holds.
///
/// A `base` that is not a revision this repository has holds nothing, which is the
/// answer for a checkout with no default branch rather than a reason to stop.
///
/// # Errors
/// [`Error::GitEncoding`] when a name is not UTF-8.
pub(super) fn merged_into(repo: &Path, base: &str) -> Result<BTreeSet<String>> {
    let output = cmd::run(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "--merged", base, "refs/heads/"],
    )?;
    if !output.ok() {
        return Ok(BTreeSet::new());
    }
    Ok(output.lines()?.iter().map(|name| (*name).to_owned()).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::one;

    fn args() -> Vec<String> {
        vec![String::from("for-each-ref")]
    }

    #[test]
    fn a_branch_states_its_name_its_age_and_its_upstream() {
        let branch =
            one(&args(), "importer/retry\u{0}1756900000\u{0}origin/importer/retry\u{0}").unwrap();
        assert_eq!(branch.name, "importer/retry");
        assert_eq!(branch.committed, 1_756_900_000);
        assert_eq!(branch.upstream.as_deref(), Some("origin/importer/retry"));
        assert!(!branch.upstream_gone);
    }

    #[test]
    fn a_branch_that_follows_nothing_names_no_upstream() {
        let branch = one(&args(), "local-only\u{0}1756900000\u{0}\u{0}").unwrap();
        assert_eq!(branch.upstream, None);
        assert!(!branch.upstream_gone);
    }

    #[test]
    fn a_branch_whose_upstream_was_deleted_says_so() {
        let branch = one(&args(), "old\u{0}1756900000\u{0}origin/old\u{0}[gone]").unwrap();
        assert_eq!(branch.upstream.as_deref(), Some("origin/old"));
        assert!(branch.upstream_gone);
    }

    #[test]
    fn a_record_with_the_wrong_shape_is_a_parse_error() {
        assert!(one(&args(), "only-a-name").is_err());
    }
}
