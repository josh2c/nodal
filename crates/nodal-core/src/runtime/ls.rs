//! The list of a project's units: what `nodal ls`, and a bare `nodal`, answer with.
//!
//! The list is the command a person types most, so it is the one with a startup budget
//! and the one that must never write. It reads the registry in a pass, asks Git about
//! each unit's home, and reads the process table once for who is attached. Nothing here
//! opens a transaction, records an event or reconciles a session row.
//!
//! Git is asked twice per home, and never more than four times. `git status` gives the
//! working tree and the upstream. `git rev-list` gives how far the branch has moved from
//! the branch it merges into. After those two, a branch that is not ahead of the base is
//! in the base's history and nothing more is asked of it. A branch that is ahead is
//! merged in memory, and only a merge that succeeded needs the base's tree to compare
//! its result against (`crate::git::integration`).
//!
//! A home that Git cannot answer for is a note under the table, not a failure. A list
//! that refuses to print because one directory was removed is worth less than a list
//! that prints nine rows and says which one it could not read.
//!
//! A reclaimed materialisation is not one of those. It is `absent` because a reclaim
//! moved it away on purpose, so there is no directory to ask Git about and no note to
//! make; the unit is listed with no home, the way it is before it is materialised. This
//! is the rule [`crate::runtime::ps::scope`] already applies, for the same reason.
//!
//! **What the list writes** (`docs/contracts.md`, The list): "The list's reading is
//! pure. After reading, the command layer records at most two things it learned or
//! derived: a unit's flip to merged, and each touched unit's recomputed `WORKUNIT.md`.
//! It records no event, reconciles no session, and never contacts the network."
//!
//! The first of the two is here, because it is the reading that finds it. A unit whose
//! work the base carries and whose branch is on a remote has been merged somewhere
//! else, and the list is where Nodal first sees it ([`crate::lifecycle::states`]). That
//! flip is recorded rather than rendered: the retention `nodal gc` measures runs from
//! it, so it has to be an instant the registry holds and not a verdict recomputed on
//! every read. The extra Git call the second signal costs is paid only by a unit whose
//! work already reads as integrated.
//!
//! The second is not here. Compiling a unit's memory ([`crate::context`]) reads every
//! home of the project again, and it is the command that asks for it, after this
//! answer is in hand. Nothing else in this module writes: no transaction, no event, no
//! session row.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::git::status::{Change, Head, State, Summary};
use crate::git::{Divergence, Git, Integration, Standing};
use crate::lifecycle::states;
use crate::model::{ActorName, EnvState, Environment, Project, Timestamp, Unit, UnitStatus};
use crate::output::view::{EnvLine, Remote, ToolSessions, UnitList, UnitRow, WorkTree};
use crate::runtime::processes::Processes;
use crate::runtime::sessions;
use crate::store::{environments, units};
use crate::{Error, Result};

/// The revisions a unit's base is looked for under, in order, once the unit's own parent
/// branch has been tried. `origin/HEAD` is what a clone recorded as the project's own
/// default; the two names after it are what a repository without that ref uses.
const FALLBACK_BRANCHES: [&str; 2] = ["main", "master"];

/// Every unit of a project, with what Git and the process table say about each.
///
/// # Errors
/// [`Error::Store`] when the registry could not be read.
pub fn list(
    conn: &Connection,
    processes: &dyn Processes,
    project: &Project,
    now: Timestamp,
) -> Result<UnitList> {
    let mut notes = Vec::new();
    let attached = attached_by_home(processes, &mut notes);
    let homes = latest_homes(&environments::list_for_project(conn, project.id)?);
    let mut bases = Bases::default();
    let mut rows = Vec::new();
    for unit in units::list(conn, project.id)? {
        let home = homes.get(&unit.id.to_string()).cloned();
        let mut built = row(&unit, home.as_ref(), &attached, &mut bases, &mut notes);
        landed(conn, &unit, &mut built, now, &mut notes);
        rows.push(built);
    }
    order(&mut rows);
    Ok(UnitList { project: project.name.clone(), now, units: rows, notes })
}

/// Put the units a person should look at first at the top.
///
/// Two rules, in this order. A unit whose work is already on the base is finished, so it
/// sinks. Everything else is sorted by how far the base has moved under it, most behind
/// first, because that is the unit whose next command is a sync. Units that tie are
/// ordered by slug, so one list of one registry is always the same list.
pub fn order(rows: &mut [UnitRow]) {
    rows.sort_by(|left, right| {
        let key = |row: &UnitRow| {
            let work = row.work.as_ref();
            (
                work.is_some_and(|work| work.integration.is_integrated()),
                std::cmp::Reverse(work.map_or(0, |work| work.main.behind)),
            )
        };
        key(left).cmp(&key(right)).then_with(|| left.slug.as_str().cmp(right.slug.as_str()))
    });
}

/// The newest materialisation of each unit, by the unit's identifier as text.
fn latest_homes(environments: &[Environment]) -> BTreeMap<String, Environment> {
    let mut latest = BTreeMap::new();
    for environment in environments {
        if environment.state == EnvState::Absent {
            continue;
        }
        latest.insert(environment.unit_id.to_string(), environment.clone());
    }
    latest
}

/// One row: the registry's facts about a unit, and Git's.
fn row(
    unit: &Unit,
    home: Option<&Environment>,
    attached: &Attached,
    bases: &mut Bases,
    notes: &mut Vec<String>,
) -> UnitRow {
    let mut row = UnitRow::from_unit(unit);
    let Some(environment) = home else { return row };
    row.sessions = attached.of(&environment.home);
    row.last_active = Some(environment.last_active);
    row.environment = Some(EnvLine::from_environment(environment));
    match work(&environment.home, unit, bases) {
        Ok(work) => row.work = Some(work),
        Err(error) => notes.push(format!("{}: {error}", unit.slug)),
    }
    row
}

/// Record the unit as merged when both signals say it has been, and show it so.
///
/// The second signal costs one `git rev-list`, so it is asked for only when the first
/// has already said the base carries the work. A failure to read it, or to write the
/// row, is a note under the table: a list that refuses to print because one unit's
/// remote could not be read is worth less than a list that prints every row and says
/// which unit it could not settle.
fn landed(
    conn: &Connection,
    unit: &Unit,
    row: &mut UnitRow,
    now: Timestamp,
    notes: &mut Vec<String>,
) {
    if !row.work.as_ref().is_some_and(|work| work.integration.is_integrated()) {
        return;
    }
    let Some(home) = row.environment.as_ref().map(|environment| environment.home.clone()) else {
        return;
    };
    match flip(conn, unit, &home, row, now) {
        Ok(true) => row.status = UnitStatus::Merged,
        Ok(false) => {}
        Err(error) => notes.push(format!("{}: {error}", unit.slug)),
    }
}

/// Ask the remote signal and write the flip down, answering whether it happened.
fn flip(
    conn: &Connection,
    unit: &Unit,
    home: &Path,
    row: &UnitRow,
    now: Timestamp,
) -> Result<bool> {
    let integration = row.work.as_ref().map_or(Integration::Unknown, |work| work.integration);
    let contained = Git::at(home).remote_containment(unit.branch.as_str())?;
    if !states::is_merged(unit.status, integration, &contained) {
        return Ok(false);
    }
    states::record_merged(conn, unit, now)
}

/// What Git says about one home.
fn work(home: &Path, unit: &Unit, bases: &mut Bases) -> Result<WorkTree> {
    let git = Git::at(home);
    let summary = git.status()?;
    let standing = bases.standing(&git, unit)?;
    Ok(WorkTree {
        dirty: count(&summary, side_worktree),
        staged: count(&summary, side_index),
        untracked: count(&summary, is_untracked),
        detached: matches!(summary.head, Head::Detached(_)),
        base: standing.base,
        main: standing.divergence,
        integration: standing.integration,
        remote: summary.upstream.map(|upstream| Remote {
            upstream,
            divergence: Divergence { ahead: summary.ahead, behind: summary.behind },
        }),
    })
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

/// Who is attached to each home, counted by tool.
#[derive(Debug, Default)]
struct Attached(BTreeMap<PathBuf, BTreeMap<ActorName, u32>>);

impl Attached {
    /// The tools attached to one home, by name.
    fn of(&self, home: &Path) -> Vec<ToolSessions> {
        self.0
            .get(home)
            .map(|counts| {
                counts
                    .iter()
                    .map(|(tool, count)| ToolSessions { tool: tool.clone(), count: *count })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Read the process table once and count who is in each home.
///
/// A host whose process table Nodal cannot read gets a note and an empty answer, for the
/// reason `docs/contracts.md` gives: a note is the difference between "nothing is
/// attached" and "I could not see".
fn attached_by_home(processes: &dyn Processes, notes: &mut Vec<String>) -> Attached {
    let derived = processes.scan().and_then(|running| sessions::derive(&running));
    let running = match derived {
        Ok(running) => running,
        Err(error) => {
            notes.push(format!("who: {error}"));
            return Attached::default();
        }
    };
    let mut homes: BTreeMap<PathBuf, BTreeMap<ActorName, u32>> = BTreeMap::new();
    for process in running {
        *homes.entry(process.root).or_default().entry(process.actor.name).or_default() += 1;
    }
    Attached(homes)
}
