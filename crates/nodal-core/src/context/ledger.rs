//! The project ledger: what every other open unit is doing, and what the base gained.
//!
//! This is the section a unit's memory exists for. An agent that knows only its own
//! branch gives answers about code a sibling has already changed, which is the failure
//! this project was started to remove: the measured cause was staleness rather than
//! edit overlap, so the ledger states both halves of it. What a sibling has touched,
//! from the diff of its own branch against its own base; and what the branch everybody
//! merges into has gained since this unit left it.
//!
//! Every line comes from Git in the sibling's home. No unit reports itself here, and
//! nothing a person wrote in a sibling's memory is copied into this one. A unit is
//! named while its work is off the base — open or under review
//! ([`Snapshot::is_in_flight`]) — because that is the work this unit can collide with.
//!
//! Each sibling is capped at [`CAP`] lines, and the cap states what it dropped. A
//! ledger that silently kept the first forty lines would read exactly like a ledger of
//! a sibling that changed forty files, which is the one thing a memory must never do.

use crate::context::survey::Snapshot;

/// How many lines one sibling may take. Forty is about a screen: enough for the shape
/// of a piece of work, short enough that ten siblings are still one readable file.
pub const CAP: usize = 40;

/// One sibling, before the cap is applied to it.
///
/// Counts rather than sentences: how a count is said is the renderer's, and the number
/// of commits a branch holds is not the number of commit lines read from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The sibling's handle and branch.
    pub heading: String,
    /// The revision it is measured against, when it could be measured.
    pub base: Option<String>,
    /// How many commits its branch holds that the base does not. Git's count, which is
    /// not the length of `commits`: only the readable few of them are read.
    pub ahead: u32,
    /// How many files its working tree holds that no commit does.
    pub uncommitted: usize,
    /// Its commits, newest first, as many as were read.
    pub commits: Vec<String>,
    /// Every file those commits changed.
    pub files: Vec<String>,
    /// Why there is nothing to say about it, when there is nothing.
    pub trouble: Option<&'static str>,
}

/// What the branch this unit merges into has gained since the unit left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gained {
    /// The revision the unit is measured against, as it was named.
    pub base: String,
    /// How many commits it gained. Counted by Git, not by the lines below.
    pub total: u32,
    /// The newest of them.
    pub commits: Vec<String>,
}

/// The whole ledger of one unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// What main gained under this unit, when the unit has a home to ask.
    pub gained: Option<Gained>,
    /// Every other open unit of the project.
    pub siblings: Vec<Entry>,
}

/// Compile the ledger one unit reads, from the survey of the whole project.
#[must_use]
pub fn of(subject: &Snapshot, project: &[Snapshot]) -> Ledger {
    Ledger {
        gained: gained(subject),
        siblings: project
            .iter()
            .filter(|other| other.unit.id != subject.unit.id && other.is_in_flight())
            .map(entry)
            .collect(),
    }
}

/// What the base has gained since the subject's base commit.
fn gained(subject: &Snapshot) -> Option<Gained> {
    let work = subject.work.as_ref()?;
    Some(Gained {
        base: work.base.clone(),
        total: work.divergence.behind,
        commits: work.gained.iter().map(commit_line).collect(),
    })
}

/// One sibling's entry, uncapped.
fn entry(other: &Snapshot) -> Entry {
    let heading = format!("{} · {}", other.unit.slug, other.unit.branch);
    let Some(work) = &other.work else {
        return Entry {
            heading,
            base: None,
            ahead: 0,
            uncommitted: 0,
            commits: Vec::new(),
            files: Vec::new(),
            trouble: Some(no_home(other)),
        };
    };
    Entry {
        heading,
        base: Some(work.base.clone()),
        ahead: work.divergence.ahead,
        uncommitted: work.uncommitted.len(),
        commits: work.commits.iter().map(commit_line).collect(),
        files: work.touched.iter().map(file_line).collect(),
        trouble: None,
    }
}

/// What is said about a unit with nothing to read.
fn no_home(other: &Snapshot) -> &'static str {
    if other.home.is_some() {
        "its home is here but Git could not be asked about it"
    } else {
        "no home on this machine"
    }
}

/// One commit, as a line a person can paste into `git show`.
fn commit_line(commit: &crate::git::history::Commit) -> String {
    format!("{} {}", commit.short(), commit.subject)
}

/// One changed file, with the letter Git gives the change.
fn file_line(change: &crate::git::history::FileChange) -> String {
    match &change.origin {
        Some(origin) => {
            format!("{} {} (from {})", change.letter(), change.path.display(), origin.display())
        }
        None => format!("{} {}", change.letter(), change.path.display()),
    }
}
