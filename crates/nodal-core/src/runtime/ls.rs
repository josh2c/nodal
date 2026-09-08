//! The list of a project's units: what `nodal ls`, and a bare `nodal`, answer with.
//!
//! The list is the command a person types most, so it is the one with a startup budget
//! and the one that must never write. Nothing here opens a transaction, records an
//! event or reconciles a session row.
//!
//! **Each home is asked each question once.** The reading is
//! [`crate::context::survey`] and this module turns its answer into rows. The survey is
//! taken anyway, for the memories the command writes afterwards, and it already reads
//! everything a row shows; taking a second reading here made every home answer
//! `status`, `rev-list`, `merge-tree` and `rev-parse` twice for one `nodal ls`. One
//! pass is the rule the survey's own module doc states, for the same reason: a fact
//! read twice is a fact that can be read two ways.
//!
//! A home that Git cannot answer for is a note under the table, not a failure. A list
//! that refuses to print because one directory was removed is worth less than a list
//! that prints nine rows and says which one it could not read. The survey makes those
//! notes; this module carries them to the table.
//!
//! A reclaimed materialisation is not one of those. It is `absent` because a reclaim
//! moved it away on purpose, so there is no directory to ask Git about and no note to
//! make; the unit is listed with no home, the way it is before it is materialised. This
//! is the rule [`crate::runtime::ps::scope`] already applies, for the same reason.
//!
//! What is read here rather than surveyed is the process table, once, for who is
//! attached to each home. It is not a question about a repository and no home is asked
//! it.
//!
//! **What the list writes** (`docs/contracts.md`, The list): "The list's reading is
//! pure. After reading, the command layer records at most two things it learned or
//! derived: a unit's flip to merged, and each touched unit's recomputed `WORKUNIT.md`.
//! It records no event, reconciles no session, and never contacts the network."
//!
//! Neither of the two is here. Both belong to the command, after this answer is in
//! hand: the flip to merged is recorded by [`crate::lifecycle::states::settle`], which
//! the `ls` and `show` commands call, and the memories are compiled by
//! [`crate::context::compile`]. This module writes nothing at all, which is what makes
//! that sentence true of module boundaries and not only of what `nodal ls` does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::Result;
use crate::context::survey::{self, Snapshot, Work};
use crate::model::{ActorName, Project, Timestamp};
use crate::output::notice::{self, Notice};
use crate::output::view::{EnvLine, ToolSessions, UnitList, UnitRow, WorkTree};
use crate::runtime::processes::Processes;
use crate::runtime::sessions;

/// Every unit of a project, with what Git and the process table say about each.
///
/// One survey of the project, turned into rows. A caller that needs the survey itself —
/// the `ls` command, which compiles the memories from it — takes it with
/// [`survey::project`] and calls [`rows`] instead, so every home is read once for the
/// whole command rather than once for each reader of the answer.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn list(
    conn: &Connection,
    processes: &dyn Processes,
    project: &Project,
    now: Timestamp,
) -> Result<UnitList> {
    Ok(rows(&survey::project(conn, project)?, processes, project, now))
}

/// The same list, built from a survey the caller has already taken.
#[must_use]
pub fn rows(
    surveyed: &[Snapshot],
    processes: &dyn Processes,
    project: &Project,
    now: Timestamp,
) -> UnitList {
    let mut notices = Vec::new();
    let attached = attached_by_home(processes, &mut notices);
    let mut units = Vec::new();
    for subject in surveyed {
        notices.extend(
            subject.notes.iter().map(|cause| Notice::about(subject.unit.slug.to_string(), cause)),
        );
        units.push(row(subject, &attached));
    }
    order(&mut units);
    // One line per cause, however many units reported it. A list of eight units whose
    // project tracks its own `CLAUDE.md` is eight units with one thing wrong, not eight
    // things (`crate::output::notice`).
    let notes = notice::collapse(&notices, "units");
    UnitList { project: project.name.clone(), now, units, notes }
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

/// One row: the registry's facts about a unit, and the survey's.
fn row(subject: &Snapshot, attached: &Attached) -> UnitRow {
    let mut row = UnitRow::from_unit(&subject.unit);
    let Some(environment) = subject.home.as_ref() else { return row };
    row.sessions = attached.of(&environment.home);
    row.last_active = Some(environment.last_active);
    row.environment = Some(EnvLine::from_environment(environment));
    row.work = subject.work.as_ref().map(work_tree);
    row
}

/// What the survey read of one home, as the list shows it.
///
/// The counts and the upstream are the survey's own reading of the one `git status` it
/// runs; the logs it read beside them are the memory's, and a row has no room for them.
fn work_tree(work: &Work) -> WorkTree {
    WorkTree {
        dirty: work.dirty,
        staged: work.staged,
        untracked: work.untracked,
        detached: work.detached,
        base: work.base.clone(),
        main: work.divergence,
        integration: work.integration,
        remote: work.remote.clone(),
    }
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
fn attached_by_home(processes: &dyn Processes, notices: &mut Vec<Notice>) -> Attached {
    let derived = processes.scan().and_then(|running| sessions::derive(&running));
    let running = match derived {
        Ok(running) => running,
        Err(error) => {
            // About the run, not about a unit: the table is read once for the whole
            // list, so the reason it could not be read is one line whatever the list
            // holds.
            notices.push(Notice::general(format!("who: {error}")));
            return Attached::default();
        }
    };
    let mut homes: BTreeMap<PathBuf, BTreeMap<ActorName, u32>> = BTreeMap::new();
    for process in running {
        *homes.entry(process.root).or_default().entry(process.actor.name).or_default() += 1;
    }
    Attached(homes)
}
