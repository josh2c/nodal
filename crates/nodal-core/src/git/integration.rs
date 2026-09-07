//! Where a branch stands against the branch it merges into.
//!
//! Two questions are asked of every unit in a list, and Git answers both from the same
//! pair of plumbing commands. How far has the branch moved: how many commits it has that
//! the base does not, and how many the base has that it does not. And what would merging
//! it do: nothing, a clean merge, or a conflict.
//!
//! The second question is asked with `git merge-tree`, not with `git branch --merged`,
//! because a squash merge and a rebase both put a branch's changes on the base without
//! putting its commits there. A branch whose work is on the base is finished however it
//! got there, so this module reads the trees rather than the history.
//!
//! Nothing here writes a ref or a file. `git merge-tree --write-tree` writes one tree
//! object into the object database and prints its identifier; that object is unreachable
//! and the next `git gc` removes it.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::cmd;
use super::oid::Oid;
use crate::error::{Error, Result};

/// The exit code `git merge-tree` uses for a merge that leaves conflicts. Any other
/// non-zero code is a failure of the command itself.
const CONFLICT_CODE: i32 = 1;

/// How far a branch has moved from the branch it merges into, in commits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Divergence {
    /// Commits the branch has that the base does not.
    pub ahead: u32,
    /// Commits the base has that the branch does not.
    pub behind: u32,
}

impl Divergence {
    /// Whether the base has moved under the branch.
    #[must_use]
    pub const fn is_behind(&self) -> bool {
        self.behind > 0
    }
}

/// Why a branch counts as integrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The branch tip is in the base's history already: an ordinary merge, or a branch
    /// that never left the base.
    Ancestor,
    /// The base carries the branch's changes without carrying its commits: a squash
    /// merge, or a rebase. Merging the branch again would change no file.
    Absorbed,
}

impl Reason {
    /// The word this reason carries in output.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ancestor => "ancestor",
            Self::Absorbed => "absorbed",
        }
    }
}

/// What merging a branch into its base would do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "reason")]
pub enum Integration {
    /// Not computed for this answer, or Git could not answer it.
    #[default]
    Unknown,
    /// The base carries every change of the branch. The work is done.
    Integrated(Reason),
    /// The branch carries changes the base does not, and the merge is clean.
    Open,
    /// The merge would leave conflicts.
    Conflict,
}

impl Integration {
    /// Whether the base carries every change of the branch.
    #[must_use]
    pub const fn is_integrated(&self) -> bool {
        matches!(self, Self::Integrated(_))
    }

    /// Whether merging the branch into its base would leave conflicts.
    #[must_use]
    pub const fn would_conflict(&self) -> bool {
        matches!(self, Self::Conflict)
    }

    /// The words this verdict carries in output.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Unknown => String::from("unknown"),
            Self::Integrated(reason) => format!("done ({})", reason.label()),
            Self::Open => String::from("open"),
            Self::Conflict => String::from("conflict"),
        }
    }
}

/// Where the checked-out branch stands against one base.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// The revision the branch was measured against, as it was named.
    pub base: String,
    /// How far the branch has moved from it.
    pub divergence: Divergence,
    /// What merging the branch into it would do.
    pub integration: Integration,
}

/// Where `HEAD` stands against `base`.
///
/// Two Git calls in the common case, and never more than three. The counts come first;
/// a branch that is not ahead is in the base's history already, so nothing else is
/// asked of it. Only a branch that is ahead is merged, and only a merge that succeeds
/// needs the base's tree to compare against.
///
/// # Errors
/// [`Error::Git`] when `base` is not a revision this repository has,
/// [`Error::GitParse`] when a count or a tree identifier could not be read.
pub(super) fn standing(repo: &Path, base: &str) -> Result<Standing> {
    let divergence = counts(repo, base)?;
    let integration = if divergence.ahead == 0 {
        Integration::Integrated(Reason::Ancestor)
    } else {
        verdict(repo, base)?
    };
    Ok(Standing { base: base.to_owned(), divergence, integration })
}

/// How many commits each side of `base...HEAD` has that the other does not.
fn counts(repo: &Path, base: &str) -> Result<Divergence> {
    let range = format!("{base}...HEAD");
    let output =
        cmd::run_ok(repo, &["rev-list", "--left-right", "--count", "--end-of-options", &range])?;
    let text = output.text()?;
    let malformed = || Error::GitParse { args: output.args.clone(), record: text.to_owned() };
    let (left, right) = text.split_once('\t').ok_or_else(malformed)?;
    Ok(Divergence {
        behind: left.trim().parse().map_err(|_| malformed())?,
        ahead: right.trim().parse().map_err(|_| malformed())?,
    })
}

/// What merging `HEAD` into `base` would do, for a branch that is ahead of it.
fn verdict(repo: &Path, base: &str) -> Result<Integration> {
    let merged = cmd::run(
        repo,
        &["merge-tree", "--write-tree", "--no-messages", "--end-of-options", base, "HEAD"],
    )?;
    if merged.code == Some(CONFLICT_CODE) {
        return Ok(Integration::Conflict);
    }
    if !merged.ok() {
        tracing::debug!(code = ?merged.code, stderr = %merged.stderr, "git merge-tree did not run");
        return Ok(Integration::Unknown);
    }
    let produced = Oid::parse(first_line(merged.text()?))?;
    if produced == tree_of(repo, base)? {
        return Ok(Integration::Integrated(Reason::Absorbed));
    }
    Ok(Integration::Open)
}

/// The tree a revision points at.
fn tree_of(repo: &Path, revision: &str) -> Result<Oid> {
    let peeled = format!("{revision}^{{tree}}");
    let output =
        cmd::run_ok(repo, &["rev-parse", "--verify", "--end-of-options", peeled.as_str()])?;
    Oid::parse(output.text()?)
}

/// The first line of a command's output, which is where `merge-tree` puts the tree.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::{Integration, Reason};

    #[test]
    fn a_verdict_says_whether_the_work_is_done_and_whether_it_would_conflict() {
        let done = Integration::Integrated(Reason::Absorbed);
        assert!(done.is_integrated());
        assert!(!done.would_conflict());
        assert_eq!(done.label(), "done (absorbed)");
        assert!(Integration::Conflict.would_conflict());
        assert!(!Integration::Unknown.is_integrated());
        assert_eq!(Integration::Open.label(), "open");
    }
}
