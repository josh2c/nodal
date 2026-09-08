//! What is true of every unit of a project, read from the project rather than told.
//!
//! One pass over the registry and the homes on disk. Every unit is surveyed once, and
//! the answer serves twice: it is the facts section of that unit's own memory, and it
//! is that unit's line in every sibling's ledger. Surveying per unit would ask Git the
//! same question once for each reader of the answer.
//!
//! Nothing here reads a `WORKUNIT.md`. The memory is a rendering of this survey and is
//! never an input to it, which is the whole of the rule that a unit's memory is
//! recomputed from reality and never from what the last one said.
//!
//! A home Git cannot answer for is a note on that unit and nothing more. The other
//! units are surveyed, the memory is still written, and the note says which fact is
//! missing and why — the rule [`crate::runtime::ls`] already applies to a list.
//!
//! The list is the other reader. [`crate::runtime::ls`] builds its rows from this
//! survey rather than taking its own reading, so each home is asked each question once.
//! That is why [`Work`] carries the counts and the upstream a row shows as well as the
//! logs a memory shows: both come from the one `git status` this module already runs,
//! and reading the home twice made a list of eight units pay for four Git invocations
//! per unit that it had already paid for.

use std::collections::BTreeSet;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::git::history::{Commit, FileChange};
use crate::git::status::{Change, Head, State, Summary};
use crate::git::{Divergence, Git, Integration, Oid, Standing};
use crate::model::{EnvState, Environment, Epistemic, Event, EventKind, Project, Unit, UnitStatus};
use crate::output::view::Remote;
use crate::store::{environments, events, units};
use crate::{Error, Result};

/// How many of a unit's own commands the memory carries. Enough to see what the last
/// session was doing; not a transcript.
const COMMANDS: u32 = 10;

/// How many stated notes and handoffs it carries.
const STATED: u32 = 20;

/// How many test results are read. One: the memory states the last one.
const TESTS: u32 = 1;

/// How many commits are read from a range. One more than a ledger block can print, so
/// nothing is read that could never be shown.
const COMMITS: u32 = 64;

/// The revisions a unit's base is looked for under, in order, once the unit's own parent
/// branch has been tried. `origin/HEAD` is what a clone recorded as the project's own
/// default; the two names after it are what a repository without that ref uses.
const FALLBACK_BRANCHES: [&str; 2] = ["main", "master"];

/// Where a unit's branch stands, and what it and the base have done since they parted.
#[derive(Debug, Clone)]
pub struct Work {
    /// The revision the branch is measured against, as it was named.
    pub base: String,
    /// The commit the branch and the base last agreed on: where the unit started.
    pub base_commit: Option<Oid>,
    /// How far the branch has moved from the base, in commits, both ways.
    pub divergence: Divergence,
    /// What merging the branch into the base would do.
    pub integration: Integration,
    /// Paths changed in the working tree and not staged.
    pub dirty: u32,
    /// Paths staged and not committed.
    pub staged: u32,
    /// Paths Git does not track and no ignore rule covers.
    pub untracked: u32,
    /// Whether HEAD names a commit rather than a branch.
    pub detached: bool,
    /// How the branch stands against its upstream, when it has one.
    pub remote: Option<Remote>,
    /// What the working tree holds that no commit does.
    pub uncommitted: Vec<FileChange>,
    /// Which files the branch's commits changed, against the base commit.
    pub touched: Vec<FileChange>,
    /// The branch's own commits, newest first, and never more than a page of them.
    pub commits: Vec<Commit>,
    /// What the base gained since the base commit, newest first, and bounded the same way.
    pub gained: Vec<Commit>,
}

/// One unit as the project holds it now.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The registry's row for the unit.
    pub unit: Unit,
    /// Its newest materialisation that is not absent, when it has one on this machine.
    pub home: Option<Environment>,
    /// What Git says about that home, when Git could be asked.
    pub work: Option<Work>,
    /// The commands run in it, newest first.
    pub commands: Vec<Event>,
    /// The last test result the log carries, when it carries one.
    pub tests: Vec<Event>,
    /// The notes and handoffs somebody stated, newest first.
    pub stated: Vec<Event>,
    /// What could not be read, and why.
    pub notes: Vec<String>,
}

impl Snapshot {
    /// Whether a sibling's ledger names this unit.
    ///
    /// Open and under review, and nothing else. The ledger answers one question — what
    /// has another unit changed that this one has not seen — so the test is whether the
    /// work is still off the base. A unit under review has changed files that no base
    /// carries yet, and leaving it out would make the ledger wrong in the one direction
    /// that costs something. A merged unit's work is on the base, where the same file
    /// reports it as what the base gained; an archived unit's is nobody's to collide
    /// with.
    #[must_use]
    pub fn is_in_flight(&self) -> bool {
        matches!(self.unit.status, UnitStatus::Open | UnitStatus::Review)
    }
}

/// Survey every unit of a project, in the order the registry lists them.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn project(conn: &Connection, project: &Project) -> Result<Vec<Snapshot>> {
    let homes = live_homes(&environments::list_for_project(conn, project.id)?);
    let mut bases = Bases::default();
    let mut snapshots = Vec::new();
    for unit in units::list(conn, project.id)? {
        let home = homes.iter().find(|environment| environment.unit_id == unit.id).cloned();
        snapshots.push(one(conn, unit, home, &mut bases)?);
    }
    Ok(snapshots)
}

/// Survey one unit: its log from the registry, its work from its home.
fn one(
    conn: &Connection,
    unit: Unit,
    home: Option<Environment>,
    bases: &mut Bases,
) -> Result<Snapshot> {
    let mut notes = Vec::new();
    let work = match &home {
        Some(environment) => match work(&Git::at(&environment.home), &unit, bases) {
            Ok(work) => Some(work),
            Err(error) => {
                notes.push(format!("{}: {error}", unit.slug));
                None
            }
        },
        None => None,
    };
    let stated = events::list_recent_of_kinds(
        conn,
        unit.id,
        &[EventKind::Note, EventKind::Handoff],
        STATED,
    )?;
    Ok(Snapshot {
        commands: events::list_recent_of_kinds(conn, unit.id, &[EventKind::Command], COMMANDS)?,
        tests: events::list_recent_of_kinds(conn, unit.id, &[EventKind::TestResult], TESTS)?,
        stated: stated.into_iter().filter(|event| event.epistemic == Epistemic::Stated).collect(),
        unit,
        home,
        work,
        notes,
    })
}

/// What Git says about one home.
///
/// The two counts a ledger reports are already here and are not counted again: `ahead`
/// is how many commits the branch has, and `behind` is how many the base gained since
/// the branch left it, because both are measured from the commit the two last agreed
/// on. The logs below are the readable few of exactly those two sets.
fn work(git: &Git, unit: &Unit, bases: &mut Bases) -> Result<Work> {
    let summary = git.status()?;
    let standing = bases.standing(git, unit)?;
    let base_commit = git.merge_base("HEAD", &standing.base).ok();
    let (touched, commits, gained) = match &base_commit {
        Some(commit) => since(git, commit.as_str(), &standing.base)?,
        None => (Vec::new(), Vec::new(), Vec::new()),
    };
    Ok(Work {
        base: standing.base,
        base_commit,
        divergence: standing.divergence,
        integration: standing.integration,
        dirty: count(&summary, side_worktree),
        staged: count(&summary, side_index),
        untracked: count(&summary, is_untracked),
        detached: matches!(summary.head, Head::Detached(_)),
        remote: summary.upstream.clone().map(|upstream| Remote {
            upstream,
            divergence: Divergence { ahead: summary.ahead, behind: summary.behind },
        }),
        uncommitted: uncommitted(&summary),
        touched,
        commits,
        gained,
    })
}

/// The three readings taken from the commit the branch and the base last agreed on.
fn since(
    git: &Git,
    commit: &str,
    base: &str,
) -> Result<(Vec<FileChange>, Vec<Commit>, Vec<Commit>)> {
    let touched = git.changed_files(&format!("{commit}..HEAD"))?;
    let commits = git.log(&format!("{commit}..HEAD"), COMMITS)?;
    let gained = git.log(&format!("{commit}..{base}"), COMMITS)?;
    Ok((touched, commits, gained))
}

/// How many entries of a status answer a question.
fn count(summary: &Summary, asks: fn(&State) -> bool) -> u32 {
    let counted = summary.entries.iter().filter(|entry| asks(&entry.state)).count();
    u32::try_from(counted).unwrap_or(u32::MAX)
}

/// Whether the working tree differs from the index, an unresolved merge included.
fn side_worktree(state: &State) -> bool {
    match state {
        State::Tracked { worktree, .. } => *worktree != Change::Unmodified,
        State::Unmerged => true,
        State::Untracked | State::Ignored => false,
    }
}

/// Whether the index differs from HEAD.
fn side_index(state: &State) -> bool {
    matches!(state, State::Tracked { index, .. } if *index != Change::Unmodified)
}

/// Whether Git tracks the path at all.
fn is_untracked(state: &State) -> bool {
    *state == State::Untracked
}

/// What the working tree holds that no commit does, as the same shape a diff reads as.
///
/// A path Git ignores is left out, because it is not the unit's work; an unresolved
/// merge is kept, because it is the most important thing in the tree when it is there.
fn uncommitted(summary: &Summary) -> Vec<FileChange> {
    summary
        .uncommitted()
        .map(|entry| FileChange {
            change: worst(&entry.state),
            path: entry.path.clone(),
            origin: entry.origin.clone(),
        })
        .collect()
}

/// The one letter that says most about a path with two sides to it.
///
/// The worktree side is preferred, because it is what a person would lose. A path that
/// differs only in the index reports the index's letter.
fn worst(state: &State) -> Change {
    match state {
        State::Tracked { index, worktree } => match worktree {
            Change::Unmodified => *index,
            other => *other,
        },
        State::Unmerged => Change::Other('U'),
        State::Untracked => Change::Other('?'),
        State::Ignored => Change::Other('!'),
    }
}

/// The homes that exist: the newest materialisation of each unit that is not absent.
///
/// A reclaimed materialisation is not a home Git can be asked about, and it is absent
/// because a reclaim moved it away on purpose. It is left out here rather than reported
/// as a home that would not answer.
fn live_homes(environments: &[Environment]) -> Vec<Environment> {
    let mut live: Vec<Environment> = Vec::new();
    for environment in environments {
        if environment.state == EnvState::Absent {
            continue;
        }
        match live.iter_mut().find(|held| held.unit_id == environment.unit_id) {
            Some(held) => *held = environment.clone(),
            None => live.push(environment.clone()),
        }
    }
    live
}

/// The revision each home is measured against, remembered once for the project.
///
/// Every home of a project is a clone of the same base, so the ref that names the branch
/// the work merges into is the same in all of them. The first home that answers decides
/// the name; the rest use it and pay one Git call rather than a search.
#[derive(Debug, Default)]
pub struct Bases {
    /// The full ref name that answered last, when one has.
    chosen: Option<String>,
}

impl Bases {
    /// Where a unit's branch stands against the branch it merges into.
    ///
    /// # Errors
    /// [`Error::GitUnknownBranch`] when no candidate is a revision the home has, and
    /// whatever Git reported for the last one tried.
    pub fn standing(&mut self, git: &Git, unit: &Unit) -> Result<Standing> {
        let mut last = None;
        for candidate in self.candidates(unit) {
            match git.standing(&candidate) {
                Ok(standing) => {
                    self.chosen = Some(candidate);
                    return Ok(standing);
                }
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| Error::GitUnknownBranch {
            repo: PathBuf::from(git.root()),
            branch: String::from("main"),
        }))
    }

    /// The revisions to try, best first, without repeats.
    fn candidates(&self, unit: &Unit) -> Vec<String> {
        let mut names: Vec<String> = self.chosen.iter().cloned().collect();
        if let Some(parent) = &unit.parent_branch {
            push_branch(&mut names, parent.as_str());
        }
        names.push(String::from("refs/remotes/origin/HEAD"));
        for fallback in FALLBACK_BRANCHES {
            push_branch(&mut names, fallback);
        }
        let mut seen = BTreeSet::new();
        names.retain(|name| seen.insert(name.clone()));
        names
    }
}

/// The two refs a branch name can be: the remote's copy, then this repository's own.
/// The remote's copy comes first, because integration is a question about the branch
/// everybody merges into rather than about a local copy of it.
fn push_branch(names: &mut Vec<String>, branch: &str) {
    names.push(format!("refs/remotes/origin/{branch}"));
    names.push(format!("refs/heads/{branch}"));
}

/// Read one of an event's references.
#[must_use]
pub fn reference<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event.refs.iter().find(|(key, _)| key.as_str() == name).map(|(_, value)| value.as_str())
}
