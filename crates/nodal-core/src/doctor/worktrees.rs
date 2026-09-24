//! The other checkouts of a checkout: what another tool made, and what state each is in.
//!
//! This is the item that costs the most on a real machine. One measured checkout held
//! four worktrees inside it and 7.85 GB of them, and none of the four said what it was
//! for. Another held twenty-eight, and not one of them was inside it.
//!
//! A worktree is found from the repository's own record, `git worktree list`, and the
//! record is the whole of the question. **Where the directory sits is not.** A tool that
//! makes worktrees may put them under the checkout, beside it, or under a directory of
//! its own somewhere else, and all three are equally the repository's. A report that
//! only counted the ones underneath said a machine was clean while twenty-eight
//! checkouts of that same repository sat next to it. So the only path this module tests
//! is whether a row is the checkout itself, which is the one row that is not a leftover.
//!
//! A row is named the way it can be found: relative to the checkout when it is inside
//! one, and by its whole path when it is not.
//!
//! That record is also what states a lock, and the lock is the rule this module is built
//! around: **a locked worktree is read no further.** Not its Git state and not its size.
//! A lock means a tool says it is working in that directory, and a size would mean
//! walking it. The row says `locked` and the reason the tool gave.
//!
//! The record states one more verdict, and this module repeats it in git's own word:
//! **a worktree git calls prunable is reported as `prunable`.** Git decides that, from
//! its own record, and doctor states it and adds nothing. A softer phrase would be
//! doctor disagreeing with git about git's data.
//!
//! The verdict is not a question about the directory. A reaper of temporary directories
//! removes the files of a worktree and leaves the directories behind, so the path still
//! exists while the worktree is prunable. A check for whether the path is there answers
//! a different question and answers this one wrongly, which is why this module asks
//! git.
//!
//! For every other worktree the report carries four facts and one recovered one:
//!
//! | fact | read from |
//! |---|---|
//! | on the remote, only here, or no remote | the remote itself where it is a directory here, and what this checkout has seen where it is not |
//! | dirty | `git status`: paths a commit would capture |
//! | behind | `git status`: commits the upstream has and this does not |
//! | size | a walk of the directory ([`super::size`]) |
//! | intent | the first prompt of the session that ran there ([`super::intent`]) |
//!
//! Which section a row goes in is a question about the repository that named it, not
//! about the directory it sits in: a worktree of the surveyed project is the surveyed
//! project's wherever it lives. The caller passes that section in, and every row of one
//! call is in it.
//!
//! Only the section for this project carries the state and the intent. For another
//! project the size is read and nothing else is, which is both the rule of that section
//! and less work.

use std::path::Path;

use crate::doctor::{Section, intent, size, unique};
use crate::git::{Git, Oid};
use crate::output::view::doctor::{Finding, Kind};
use crate::paths;
use crate::{Result, git};

/// Git's word for a worktree whose record points at a location that is not there.
///
/// It is git's word and not doctor's, so it is stated once and used as it is.
pub const PRUNABLE: &str = "prunable";

/// Every worktree the repository at `root` names, other than that checkout itself.
///
/// A `root` that is not a repository has none, which is the answer for a directory a
/// person points doctor at that Git does not know.
///
/// A row is skipped only when it is `root`. Nothing here asks whether a worktree is
/// underneath the checkout, because a repository names its worktrees wherever their
/// directories are and every one of them is disk this project holds.
///
/// Both ends of the one comparison here are resolved paths. Git prints the path it
/// resolved, and `root` may be the path a registry row holds, which nothing resolved.
///
/// # Errors
/// [`crate::Error::Git`] when the repository's own record could not be read.
pub fn find(root: &Path, sessions: Option<&Path>, section: Section) -> Result<Vec<Finding>> {
    let Ok(git) = Git::open(root) else {
        return Ok(Vec::new());
    };
    let root = paths::resolve(root);
    let mut registered: Vec<git::worktree::Registered> = git
        .worktrees()?
        .into_iter()
        .map(|mut one| {
            one.path = paths::resolve(&one.path);
            one
        })
        .filter(|one| one.path != root)
        .collect();
    if registered.is_empty() {
        // A checkout with no worktree of its own asks nothing further. The reading below is one
        // invocation, and a survey of a machine's repositories pays it once per repository, so
        // it is not taken where there is no row for it to be about.
        return Ok(Vec::new());
    }
    // Read once for every worktree of this checkout. A worktree shares the object store and
    // the remote-tracking refs of the repository it belongs to, so what this checkout has seen
    // on a remote is one fact about the repository rather than one per worktree.
    // The strong reading where there is one: a remote that is a directory on this machine
    // is read directly, and what it has not got it has not got. A remote on a server
    // cannot be read at all, and the row then says what this checkout has seen.
    let proved = unique::read_directly(&root);
    let seen = match &proved {
        Some(trusted) => trusted.tips.clone(),
        None => git.seen_on_remotes().unwrap_or_default(),
    };
    let read = Read { tips: &seen, checked: proved.is_some() };
    let findings =
        registered.drain(..).map(|one_of| one(&root, &one_of, sessions, (section, read))).collect();
    Ok(findings)
}

/// What every row of one repository is counted against, and what that counting is worth.
///
/// A worktree shares the object store and the refs of the repository it belongs to, so
/// both halves are one fact about the repository rather than one per worktree.
#[derive(Debug, Clone, Copy)]
struct Read<'a> {
    /// The tips a commit of the worktree is counted against.
    tips: &'a [Oid],
    /// Whether a reading of the remote itself stood behind those tips.
    checked: bool,
}

/// One worktree as a row, named relative to the checkout when it is inside it and by
/// its whole path when it is not.
///
/// The whole path is what makes a worktree beside the checkout findable. A relative
/// name for one would be a walk back out of the checkout, and a bare directory name
/// would not say where to look at all.
/// `read` pairs the section this worktree belongs to with what the checkout has seen on a
/// remote, which [`find`] read once for the whole repository. They travel together because
/// they are asked together: a worktree of another project is not read at all, and one of this
/// project is counted against exactly those tips.
fn one(
    root: &Path,
    registered: &git::worktree::Registered,
    sessions: Option<&Path>,
    read: (Section, Read<'_>),
) -> Finding {
    let (section, seen) = read;
    let name = registered.path.strip_prefix(root).unwrap_or(&registered.path);
    let finding = Finding::new(Kind::Worktree, name.display().to_string());
    if let Some(reason) = &registered.locked {
        return locked(finding, reason);
    }
    let measured = size::measure(&registered.path);
    let finding = finding.sized(measured.bytes, measured.complete);
    if let Some(reason) = &registered.prunable {
        return prunable(finding, reason);
    }
    if section == Section::Elsewhere {
        return finding;
    }
    let finding = state(finding, &registered.path, registered.branch.as_deref(), seen);
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

/// A worktree git calls prunable: git's word, and the reason git gave for it.
///
/// The size stays on the row. A hollow shell holds directories, and a directory tree a
/// reaper left behind is still disk this machine is carrying.
///
/// Nothing further is read. The record git holds points at a location that is not
/// there, so there is no checkout here to ask about a branch or a status.
fn prunable(finding: Finding, reason: &str) -> Finding {
    let finding = finding.says(PRUNABLE);
    if reason.trim().is_empty() {
        return finding;
    }
    finding.says(reason.trim().to_owned())
}

/// What Git says about a worktree: whether its work exists anywhere else, whether it
/// has uncommitted changes, and how far behind its upstream it is.
///
/// A read that fails leaves its fact off the row rather than failing the report. Doctor
/// runs on a machine that is in a mess, and one broken checkout must not stop the answer
/// about the other eleven.
///
/// The word for the work used to be "pushed", read off `rev-list HEAD --not --remotes`. Two
/// things were wrong with it. The construction asks for a namespace rather than naming the
/// refs it rested on ([`crate::git::outside`]); and the word is a claim about a server, which
/// the reading behind it cannot make — those refs are the checkout's own record of its own
/// pushes, and nothing corrects them when the remote drops a branch. Reproduced on the
/// binary: after a push, a merge and a remote branch deletion without a prune, this row said
/// "pushed" about work the remote no longer held.
///
/// The row asks the strong reading first. A remote that is a directory on this machine is
/// read directly ([`crate::doctor::unique::read_directly`]), and what it has not got it has
/// not got, so the row says **on the remote** or **only here**. A remote on a server cannot
/// be read at all, and the row then states what this checkout has seen on one, in those
/// words. A reading that failed says so rather than saying nothing.
fn state(finding: Finding, path: &Path, branch: Option<&str>, seen: Read<'_>) -> Finding {
    let Ok(git) = Git::open(path) else {
        return finding.says("not a checkout");
    };
    let mut finding = match branch {
        Some(branch) => finding.says(branch.to_owned()),
        None => finding.says("detached"),
    };
    if let Ok(remotes) = git.remotes() {
        finding = if remotes.is_empty() {
            // Nowhere it could have pushed to, and the only fact here is that this
            // repository has no remote.
            finding.says("no remote")
        } else {
            match (git.count_outside("HEAD", seen.tips), seen.checked) {
                (Ok(0), true) => finding.says("on the remote"),
                (Ok(0), false) => finding.says("seen on a remote"),
                (Ok(kept), true) => finding.says(format!("only here {kept}")),
                (Ok(kept), false) => finding.says(format!("unpushed {kept}")),
                (Err(_), _) => finding.says("not read"),
            }
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
