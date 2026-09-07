//! The one uniqueness check: does this home hold work that exists nowhere else?
//!
//! Every destructive path calls [`check`] before it removes anything, and there is
//! deliberately only one of them. A second implementation of "is this safe to delete"
//! is a second answer to a question that has to have one, and the difference between
//! the two is the day somebody loses a morning's work.
//!
//! Three things count as work that is only here, and each is a different kind of
//! loss:
//!
//! - **uncommitted changes**: tracked files that differ from the index or from `HEAD`,
//!   and paths with unresolved merge stages;
//! - **untracked files no ignore rule covers**: a new file a person made and has not
//!   added yet is work, and an ignored one is a build artefact;
//! - **commits no other tree has**: commits reachable from the unit's branch that no
//!   remote-tracking ref has, and that the project's own checkout does not have either.
//!
//! The third one is why the check takes two repositories. `git rev-list <branch> --not
//! --remotes` is the whole answer only in a repository that has a remote. A project
//! with no remote — `git init` and nothing since, a private scratch tree, a clone
//! nobody can push to — has *every* commit uncontained, including the ones the
//! home inherited from the base it was cloned from. Those commits are in the person's
//! own checkout, so removing the home does not remove them. Asking the checkout as well
//! turns the answer from "every commit in the project" into "the commits this unit
//! made", which is the question that was being asked.
//!
//! One class of path is never work: the files Nodal itself writes into a home — the
//! activation, the manifest and the marker. They are normally invisible to `git status`
//! anyway, because a create puts them in the home's `.git/info/exclude`
//! ([`crate::env::files::hide`]). Leaving them out here as well is what stops a home
//! whose exclude file somebody edited from refusing every reclaim for ever, over files
//! Nodal put there and nobody wrote.
//!
//! The check reads and never writes. What is done about a finding — refuse, or take a
//! snapshot and go on — belongs to the operation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::git::status::{Entry, State, Summary};
use crate::git::{Git, Oid};

/// How many paths or commits one finding names before it says how many more there are.
///
/// A refusal is read by a person who has to decide what to do next, and forty file
/// names is not a decision aid. The count is exact; the list is a sample.
pub const SAMPLE: usize = 10;

/// One reason a home is not safe to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    /// Tracked paths that differ from `HEAD` or the index, or that are unmerged.
    Uncommitted {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them, for the message.
        sample: Vec<PathBuf>,
    },
    /// Paths Git does not track and no ignore rule covers.
    Untracked {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them.
        sample: Vec<PathBuf>,
    },
    /// Commits that no remote and no other tree on this machine has.
    Unpushed {
        /// How many there are.
        count: usize,
        /// The first [`SAMPLE`] of them, newest first.
        sample: Vec<Oid>,
        /// The remotes that were asked. Empty means the repository has none, which is
        /// worth printing: nothing has been pushed because there is nowhere to push.
        remotes: Vec<String>,
    },
}

impl Finding {
    /// The word this finding is reported under.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Uncommitted { .. } => "uncommitted changes",
            Self::Untracked { .. } => "untracked files",
            Self::Unpushed { .. } => "commits on no remote",
        }
    }

    /// How many things this finding is about.
    #[must_use]
    pub const fn count(&self) -> usize {
        match self {
            Self::Uncommitted { count, .. }
            | Self::Untracked { count, .. }
            | Self::Unpushed { count, .. } => *count,
        }
    }

    /// The finding as one line: what it is, how many, and a sample of them.
    #[must_use]
    pub fn describe(&self) -> String {
        let listed = match self {
            Self::Uncommitted { sample, .. } | Self::Untracked { sample, .. } => {
                names(sample.iter().map(|path| path.display().to_string()))
            }
            Self::Unpushed { sample, .. } => {
                names(sample.iter().map(|oid| oid.as_str().chars().take(8).collect()))
            }
        };
        let more = self.count().saturating_sub(SAMPLE);
        let tail = if more == 0 { String::new() } else { format!(" and {more} more") };
        format!("{} ({}): {listed}{tail}", self.label(), self.count())
    }

    /// Every finding as one line, for a message that has to name the reason.
    #[must_use]
    pub fn summarise(findings: &[Self]) -> String {
        names(findings.iter().map(Self::describe))
    }
}

/// What the check found about one home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uniqueness {
    /// The home that was read.
    pub home: PathBuf,
    /// What is only here, in the order the check makes it. Empty means the home holds
    /// nothing that removing it would lose.
    pub findings: Vec<Finding>,
}

impl Uniqueness {
    /// Whether the home holds nothing that exists only here.
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Read `home` and report everything in it that exists nowhere else.
///
/// `elsewhere` is the project's own checkout, when this machine still has one. Commits
/// it already has are not unique to the home, whatever the remotes say. A checkout that
/// is not there, or is no longer a repository, simply is not asked: the answer is then
/// the stricter one, which is the safe direction to be wrong in.
///
/// # Errors
/// [`crate::Error::Git`] when the status or the revision could not be read, and
/// [`crate::Error::NotARepository`] when `home` is not one.
pub fn check(home: &Path, elsewhere: Option<&Path>) -> Result<Uniqueness> {
    let git = Git::open(home)?;
    let status = git.status()?;
    let mut findings = Vec::new();
    findings.extend(uncommitted(&status));
    findings.extend(untracked(&status));
    findings.extend(unpushed(&git, elsewhere)?);
    Ok(Uniqueness { home: home.to_path_buf(), findings })
}

/// The tracked paths that carry work a commit would capture.
fn uncommitted(status: &Summary) -> Option<Finding> {
    let paths =
        paths_where(status, |entry| matches!(entry.state, State::Tracked { .. } | State::Unmerged));
    finding(paths, |count, sample| Finding::Uncommitted { count, sample })
}

/// The paths Git does not track and no ignore rule covers.
fn untracked(status: &Summary) -> Option<Finding> {
    let paths = paths_where(status, |entry| entry.state == State::Untracked);
    finding(paths, |count, sample| Finding::Untracked { count, sample })
}

/// The paths of the entries a rule selects, in the order Git listed them, leaving out
/// the files Nodal wrote itself.
fn paths_where(status: &Summary, wanted: impl Fn(&Entry) -> bool) -> Vec<PathBuf> {
    status
        .entries
        .iter()
        .filter(|entry| wanted(entry) && !is_nodals_own(&entry.path))
        .map(|entry| entry.path.clone())
        .collect()
}

/// Whether a path is one Nodal writes into a home rather than one a person wrote.
fn is_nodals_own(path: &Path) -> bool {
    let own =
        crate::env::files::PATHS.iter().chain(std::iter::once(&crate::lifecycle::marker::FILE));
    own.map(Path::new).any(|mine| mine == path)
}

/// A finding over a list of paths, or nothing when the list is empty.
fn finding(paths: Vec<PathBuf>, build: impl Fn(usize, Vec<PathBuf>) -> Finding) -> Option<Finding> {
    if paths.is_empty() {
        return None;
    }
    let count = paths.len();
    Some(build(count, paths.into_iter().take(SAMPLE).collect()))
}

/// The commits of this home that no remote has and no other tree on this machine has.
fn unpushed(git: &Git, elsewhere: Option<&Path>) -> Result<Option<Finding>> {
    let containment = git.remote_containment("HEAD")?;
    if containment.is_contained() {
        return Ok(None);
    }
    let known = elsewhere.and_then(|path| Git::open(path).ok());
    let mut only_here = Vec::new();
    for commit in containment.unpushed {
        if has(known.as_ref(), &commit)? {
            continue;
        }
        only_here.push(commit);
    }
    if only_here.is_empty() {
        return Ok(None);
    }
    let count = only_here.len();
    Ok(Some(Finding::Unpushed {
        count,
        sample: only_here.into_iter().take(SAMPLE).collect(),
        remotes: containment.remotes,
    }))
}

/// Whether another repository on this machine holds the commit.
fn has(other: Option<&Git>, commit: &Oid) -> Result<bool> {
    match other {
        Some(git) => git.has_commit(commit.as_str()),
        None => Ok(false),
    }
}

/// Join what a finding lists, in the one form every message here uses.
fn names(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<String>>().join(", ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Finding, SAMPLE, Uniqueness};

    fn sample(count: usize) -> Finding {
        Finding::Untracked {
            count,
            sample: (0..count.min(SAMPLE)).map(|n| PathBuf::from(format!("f{n}"))).collect(),
        }
    }

    #[test]
    fn the_files_nodal_writes_into_a_home_are_not_a_persons_work() {
        for own in [".nodal/id", ".nodal/env", ".nodal/manifest.toml", ".envrc"] {
            assert!(super::is_nodals_own(std::path::Path::new(own)), "{own}");
        }
        assert!(!super::is_nodals_own(std::path::Path::new("app/main.txt")));
        assert!(!super::is_nodals_own(std::path::Path::new(".nodal-notes")));
    }

    #[test]
    fn a_home_with_no_finding_is_clear() {
        let clear = Uniqueness { home: PathBuf::from("/h"), findings: Vec::new() };
        assert!(clear.is_clear());
        assert!(!Uniqueness { home: PathBuf::from("/h"), findings: vec![sample(1)] }.is_clear());
    }

    #[test]
    fn a_long_finding_says_how_many_more_it_did_not_name() {
        let described = sample(SAMPLE + 3).describe();
        assert!(described.starts_with("untracked files (13): f0, f1"), "{described}");
        assert!(described.ends_with("and 3 more"), "{described}");
    }

    #[test]
    fn a_short_finding_names_all_of_it_and_promises_no_more() {
        let described = sample(2).describe();
        assert_eq!(described, "untracked files (2): f0, f1");
    }

    #[test]
    fn a_summary_names_every_reason_the_check_gave() {
        let summary = Finding::summarise(&[
            Finding::Uncommitted { count: 1, sample: vec![PathBuf::from("a.rs")] },
            sample(1),
        ]);
        assert!(summary.contains("uncommitted changes (1): a.rs"), "{summary}");
        assert!(summary.contains("untracked files (1): f0"), "{summary}");
    }
}
