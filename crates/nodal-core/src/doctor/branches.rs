//! Local branches with no worktree: the work a path-anchored report cannot see.
//!
//! Every other source in this module answers a question about a directory. This one
//! does not, and that is the reason it exists. A measured machine held 317 local
//! branches and 306 of them had no worktree anywhere, so nothing about the machine's
//! directories could report them. Twenty-two of those 306 held fifty commits that exist
//! on no remote. A doctor that said the worktrees were clean was telling the truth and
//! leaving out the only unbacked-up work on the machine.
//!
//! Branch sprawl costs about two megabytes. This is not a disk problem and the report
//! does not present it as one: it is a visibility problem, and the whole of the fix is
//! that the commits nothing else holds are named.
//!
//! ## Three buckets, and only one of them is loud
//!
//! | bucket | what it means | how it prints |
//! |---|---|---|
//! | unpushed | commits exist on no remote-tracking ref | one row each, with the count, the age and the upstream |
//! | on a remote | not merged, but every commit is on a remote | one summary line |
//! | merged | the default branch already holds it | one summary line |
//!
//! `--all` prints the two quiet buckets row by row. It is a rendering choice and not a
//! different answer: `--json` carries every row either way.
//!
//! The proportions are the reason for the split. Two hundred and ninety-five safe rows
//! printed at the same weight as twenty-two dangerous ones is a report that hides the
//! twenty-two inside itself.
//!
//! ## Which bucket a branch goes in
//!
//! The unpushed count is read first and it wins. A branch with commits on no remote is
//! in the loud bucket whatever else is true of it, including a branch the default
//! branch holds — because a default branch that has not been pushed either does not
//! make its commits safe. The order is the conservative one: a branch is only ever
//! quiet when the reading that says it is safe is the reading that was taken.
//!
//! A repository that names no remote has nothing to contain its commits, so every
//! branch of it that the default branch does not hold is unpushed. That is what
//! `remote_containment` means and this module does not soften it.
//!
//! ## Report-only, like everything else here
//!
//! One `for-each-ref` for the branches, one for the merged set, and one `rev-list` per
//! branch ([`crate::git::branches`]). Nothing is fetched, nothing is pruned and no ref
//! is written. What to do about a branch is a person's decision.

use std::collections::BTreeSet;
use std::path::Path;

use crate::git::Git;
use crate::model::Timestamp;
use crate::output::view::doctor::{BranchRow, Branches, Standing};
use crate::{Result, git};

/// Where `refs/remotes/origin/HEAD` is read from: what a clone recorded as the
/// project's own default branch.
const ORIGIN_HEAD: &str = "refs/remotes/origin/HEAD";

/// The branch names tried when `origin/HEAD` says nothing, in order.
const FALLBACK: [&str; 2] = ["main", "master"];

/// Audit the local branches of the repository at `root`.
///
/// A `root` that is not a repository has no branches, which is the answer for a
/// directory a person points doctor at that Git does not know.
///
/// # Errors
/// [`crate::Error::Git`] when `git for-each-ref` failed, [`crate::Error::GitParse`] on
/// a record that could not be read.
pub fn find(root: &Path, now: Timestamp) -> Result<Branches> {
    let Ok(git) = Git::open(root) else {
        return Ok(Branches::default());
    };
    let held = checked_out(&git)?;
    let base = base_of(&git)?;
    let merged = match &base {
        Some(base) => git.merged_into(base)?,
        None => BTreeSet::new(),
    };
    let mut rows = Vec::new();
    for local in git.local_branches()? {
        if held.contains(&local.name) {
            continue;
        }
        rows.push(row(&git, &local, &merged, now)?);
    }
    loudest_first(&mut rows);
    Ok(Branches { base, rows, expand: false })
}

/// One branch as a row: the bucket it is in, and the facts that bucket prints.
fn row(
    git: &Git,
    local: &git::branches::Local,
    merged: &BTreeSet<String>,
    now: Timestamp,
) -> Result<BranchRow> {
    let unpushed = git.unpushed_count(&format!("refs/heads/{}", local.name))?;
    let standing = if unpushed > 0 {
        Standing::Unpushed
    } else if merged.contains(&local.name) {
        Standing::Merged
    } else {
        Standing::OnRemote
    };
    Ok(BranchRow {
        name: local.name.clone(),
        standing,
        unpushed,
        upstream: local.upstream.clone(),
        committed: Timestamp::from_unix_seconds(local.committed).unwrap_or(now),
        upstream_gone: local.upstream_gone,
    })
}

/// Every branch some worktree of this repository has checked out.
///
/// These are the branches the worktree section already reports, with their state and
/// their size. A branch reported twice is a report that counts its own rows twice.
fn checked_out(git: &Git) -> Result<BTreeSet<String>> {
    Ok(git.worktrees()?.into_iter().filter_map(|registered| registered.branch).collect())
}

/// The branch this checkout treats as its default, `None` when it has none.
///
/// `origin/HEAD` first, because a clone recorded what the remote said its default
/// branch was. Then the two conventional names, and only where the repository really
/// has one: a name that is not a ref would make every branch unmerged.
///
/// Public because the verdict measures against the same revision this audit does
/// ([`crate::runtime::verdict`]). Two readings of one checkout that disagreed about
/// which branch is the default would disagree about which work is finished, and a
/// person would have no way to tell which of the two was right.
///
/// # Errors
/// [`crate::Error::Git`] when a ref of this repository could not be read.
pub fn base_of(git: &Git) -> Result<Option<String>> {
    if let Some(short) = git.symbolic_ref(ORIGIN_HEAD)?.and_then(|reference| short_name(&reference))
    {
        return Ok(Some(short));
    }
    for name in FALLBACK {
        if git.branch_exists(name)? {
            return Ok(Some(name.to_owned()));
        }
    }
    Ok(None)
}

/// A remote-tracking ref as `--merged` names it: `refs/remotes/origin/main` is
/// `origin/main`.
fn short_name(reference: &str) -> Option<String> {
    reference.strip_prefix("refs/remotes/").map(ToOwned::to_owned)
}

/// Order rows so that the loudest is first: most unpushed commits, then oldest, then by
/// name so that two runs over one machine print the same list.
fn loudest_first(rows: &mut [BranchRow]) {
    rows.sort_by(|left, right| {
        right
            .unpushed
            .cmp(&left.unpushed)
            .then_with(|| left.committed.unix_seconds().cmp(&right.committed.unix_seconds()))
            .then_with(|| left.name.cmp(&right.name))
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{BranchRow, Standing, loudest_first};
    use crate::model::Timestamp;

    fn row(name: &str, unpushed: usize, committed: i64) -> BranchRow {
        BranchRow {
            name: String::from(name),
            standing: if unpushed > 0 { Standing::Unpushed } else { Standing::OnRemote },
            unpushed,
            upstream: None,
            committed: Timestamp::from_unix_seconds(committed).unwrap(),
            upstream_gone: false,
        }
    }

    #[test]
    fn the_branch_holding_the_most_commits_nobody_else_has_is_first() {
        let mut rows = vec![
            row("quiet", 0, 1_756_000_000),
            row("small", 3, 1_756_000_000),
            row("importer", 33, 1_756_000_000),
        ];
        loudest_first(&mut rows);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["importer", "small", "quiet"]);
    }

    #[test]
    fn branches_holding_the_same_count_are_ordered_oldest_first_then_by_name() {
        let mut rows = vec![
            row("new", 2, 1_756_900_000),
            row("old", 2, 1_750_000_000),
            row("also-old", 2, 1_750_000_000),
        ];
        loudest_first(&mut rows);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["also-old", "old", "new"]);
    }
}
