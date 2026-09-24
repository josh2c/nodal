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
//! | unpushed | this checkout has not seen its commits on any remote | one row each, with the count, the age and the upstream |
//! | seen on a remote | not merged, and this checkout has seen every commit of it out there | one summary line |
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
//! The unpushed count is read first and it wins. A branch with commits nothing proves a
//! remote has is in the loud bucket whatever else is true of it, including a branch the
//! default branch holds — because a default branch that has not been pushed either does not
//! make its commits safe. The order is the conservative one: a branch is only ever quiet
//! when the reading that says it is safe is the reading that was taken.
//!
//! ## Which reading that is
//!
//! It is not `refs/remotes/` inside this repository. A repository writes those refs when it
//! fetches or pushes and never corrects them, so one that pushed a branch once reported the
//! branch as pushed for the rest of its life — after the branch was deleted on the remote
//! and its commits lived in one directory. Reproduced on the binary: after a push, a merge
//! and a remote branch deletion without a prune, this table called the branch pushed.
//!
//! So the count names the refs it rested on ([`crate::git::Git::seen_on_remotes`]) rather
//! than asking for the `--remotes` namespace, and **every word this table prints says what
//! that reading is worth.** A branch is "seen on a remote" and never "on a remote": this
//! checkout saw those commits out there at a fetch or a push it made, which is a fact about
//! this checkout and not a claim about any server.
//!
//! The reading that earns the stronger word is what a witness vouches for — the clone beside
//! this one that heard from the same remote more recently
//! ([`crate::doctor::unique::believed`]) — and the destructive paths ask that one. A branch
//! audit cannot: a person's machine usually holds one clone of a remote, so a witness would
//! vouch for nothing and every branch of every repository would read "not checked". A report
//! that says one thing about every row has told a person nothing.
//!
//! ## Report-only, like everything else here
//!
//! One `for-each-ref` for the branches, one for the merged set, one for the tips this
//! checkout has seen on a remote, and one `rev-list` per branch
//! ([`crate::git::branches`]). Nothing is fetched, nothing is pruned and no ref is
//! written. What to do about a branch is a person's decision.

use std::collections::BTreeSet;
use std::path::Path;

use crate::git::{Git, Oid};
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
    // Two readings, each named. What this checkout has seen on a remote is a fact about the
    // checkout and not about each branch, so it is read once for the whole table: asking for
    // it per branch would multiply one `for-each-ref` by three hundred.
    let locals = git.local_branches()?;
    let seen = git.seen_on_remotes()?;
    let mut rows = Vec::new();
    for local in locals {
        if held.contains(&local.name) {
            continue;
        }
        rows.push(row(&git, &local, &merged, &seen, now)?);
    }
    loudest_first(&mut rows);
    Ok(Branches { base, rows, expand: false })
}

/// One branch as a row: the bucket it is in, and the facts that bucket prints.
///
/// `seen` is what this checkout last saw on a remote, read once for the whole repository.
///
/// The commits of the branch that none of those tips reaches are the unpushed count. The refs
/// are named rather than asked for as a namespace, which is the whole of the difference
/// between this and the reading it replaced ([`crate::git::outside`]).
fn row(
    git: &Git,
    local: &git::branches::Local,
    merged: &BTreeSet<String>,
    seen: &[Oid],
    now: Timestamp,
) -> Result<BranchRow> {
    let unpushed = git.count_outside(&format!("refs/heads/{}", local.name), seen)?;
    let standing = if unpushed > 0 {
        Standing::Unpushed
    } else if merged.contains(&local.name) {
        Standing::Merged
    } else {
        Standing::SeenOnRemote
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
            standing: if unpushed > 0 { Standing::Unpushed } else { Standing::SeenOnRemote },
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
