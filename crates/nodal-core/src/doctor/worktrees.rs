//! The checkouts inside a checkout: what another tool made, and what state each is in.
//!
//! This is the item that costs the most on a real machine. One measured checkout held
//! four nested worktrees and 7.85 GB of them, and none of the four said what it was for.
//!
//! A worktree is found from the repository's own record, `git worktree list`. That
//! record is what states a lock, and the lock is the rule this module is built around:
//! **a locked worktree is read no further.** Not its Git state and not its size. A lock
//! means a tool says it is working in that directory, and a size would mean walking it.
//! The row says `locked` and the reason the tool gave.
//!
//! For every other nested worktree the report carries four facts and one recovered one:
//!
//! | fact | read from |
//! |---|---|
//! | merged, unmerged or no remote | `git rev-list <head> --not --remotes`: commits no remote has |
//! | dirty | `git status`: paths a commit would capture |
//! | behind | `git status`: commits the upstream has and this does not |
//! | size | a walk of the directory ([`super::size`]) |
//! | intent | the first prompt of the session that ran there ([`super::intent`]) |
//!
//! Only the section for this project carries the state and the intent. For another
//! project the size is read and nothing else is, which is both the rule of that section
//! and less work.

use std::path::Path;

use crate::doctor::{Section, intent, size};
use crate::git::Git;
use crate::output::view::doctor::{Finding, Kind};
use crate::{Result, git};

/// Every worktree inside the checkout at `root`, other than the checkout itself.
///
/// A `root` that is not a repository has none, which is the answer for a directory a
/// person points doctor at that Git does not know.
///
/// # Errors
/// [`crate::Error::Git`] when the repository's own record could not be read.
pub fn find(root: &Path, sessions: Option<&Path>, section: Section) -> Result<Vec<Finding>> {
    let Ok(git) = Git::open(root) else {
        return Ok(Vec::new());
    };
    let mut findings = Vec::new();
    for registered in git.worktrees()? {
        if !registered.path.starts_with(root) || registered.path == root {
            continue;
        }
        findings.push(one(root, &registered, sessions, section));
    }
    Ok(findings)
}

/// One nested worktree as a row.
fn one(
    root: &Path,
    registered: &git::worktree::Registered,
    sessions: Option<&Path>,
    section: Section,
) -> Finding {
    let name = registered.path.strip_prefix(root).unwrap_or(&registered.path);
    let finding = Finding::new(Kind::NestedWorktree, name.display().to_string());
    if let Some(reason) = &registered.locked {
        return locked(finding, reason);
    }
    let measured = size::measure(&registered.path);
    let finding = finding.sized(measured.bytes, measured.complete);
    if section == Section::Elsewhere {
        return finding;
    }
    let finding = state(finding, &registered.path, registered.branch.as_deref());
    match sessions.and_then(|config| intent::recover(config, &registered.path)) {
        Some(prompt) => Finding { intent: Some(prompt), ..finding },
        None => finding,
    }
}

/// A locked worktree: the lock, the reason, and nothing that would need a look inside.
fn locked(finding: Finding, reason: &str) -> Finding {
    let finding = finding.says("locked");
    if reason.trim().is_empty() {
        return finding.says("not inspected");
    }
    finding.says(reason.trim().to_owned()).says("not inspected")
}

/// What Git says about a worktree: whether its work exists anywhere else, whether it
/// has uncommitted changes, and how far behind its upstream it is.
///
/// A read that fails leaves its fact off the row rather than failing the report. Doctor
/// runs on a machine that is in a mess, and one broken checkout must not stop the answer
/// about the other eleven.
fn state(finding: Finding, path: &Path, branch: Option<&str>) -> Finding {
    let Ok(git) = Git::open(path) else {
        return finding.says("not a checkout");
    };
    let mut finding = match branch {
        Some(branch) => finding.says(branch.to_owned()),
        None => finding.says("detached"),
    };
    if let Ok(containment) = git.remote_containment("HEAD") {
        finding = if containment.remotes.is_empty() {
            // Nothing to be contained by. "unmerged" would read as a judgement about
            // the work, and the only fact here is that this repository has no remote.
            finding.says("no remote")
        } else if containment.unpushed.is_empty() {
            finding.says("merged")
        } else {
            finding.says(format!("unmerged {}", containment.unpushed.len()))
        };
    }
    if let Ok(status) = git.status() {
        let dirty = status.uncommitted().count();
        if dirty > 0 {
            finding = finding.says(format!("dirty {dirty}"));
        }
        if status.behind > 0 {
            finding = finding.says(format!("behind {}", status.behind));
        }
    }
    finding
}
