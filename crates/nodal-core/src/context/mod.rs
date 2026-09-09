//! The context compiler: a unit's memory, written from the project rather than told.
//!
//! `WORKUNIT.md` in a unit's home is what the next agent, the next terminal or the next
//! person reads to continue the work. It is compiled, not accumulated: every line in it
//! is computed from the registry and from Git at the moment it is written, and the last
//! copy of the file is never an input to the next one. A session that ended with a
//! closed laptop and no handoff therefore loses nothing that was ever a fact, because
//! no fact in the file depended on that session saying anything.
//!
//! Three sections, and the order is a contract (`docs/contracts.md`).
//!
//! **Facts** are what is true now: the objective, the branch, the commit the branch
//! left the base at, how far the base has moved under it, what the working tree holds,
//! the last commands out of the event log, and the last test result the log carries.
//!
//! **Stated** is the other tier ([`crate::model::Epistemic`]): the notes and handoffs
//! somebody wrote down. They are appended, attributed and dated, and they are never
//! mixed with the facts, because a fact Nodal computed and a claim somebody wrote down
//! are worth different amounts.
//!
//! **The project ledger** is why the file exists at all. For every other open unit it
//! states the branch, the files that unit's own branch changed against its own base,
//! and its commits; and it states what the branch everybody merges into has gained
//! since this unit left it. That is the measured cause of agents giving wrong answers
//! in parallel work — the sibling had already changed the code, or the base had moved —
//! written down where the agent will read it.
//!
//! Everything is bounded. A sibling takes at most [`ledger::CAP`] lines and the cap
//! states what it dropped, so a memory of a project with twenty units is still a file
//! somebody reads rather than a file somebody scrolls.
//!
//! The write is atomic ([`atomic`]), because the reader may be reading while the writer
//! writes. `CLAUDE.md` and `AGENTS.md` get one line each pointing here
//! ([`pointer::write`]).

pub mod atomic;
pub mod ledger;
pub mod pointer;
pub mod render;
pub mod survey;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::Result;
use crate::adapters::claude_code;
use crate::git::Git;
use crate::model::Project;
use crate::output::notice::{self, Notice};
use crate::recipe;
use crate::runtime::entry;

/// The memory itself, relative to a unit's home.
pub const FILE: &str = "WORKUNIT.md";

/// What one compile wrote, and what it could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Every memory written, by path.
    pub written: Vec<PathBuf>,
    /// What a signal could not answer. A note is not a failure: the memory is still
    /// written, and it states which fact is missing and why.
    ///
    /// A notice keeps the cause apart from the unit it is about, because a compile
    /// visits every unit of the project and most of what it cannot do it cannot do for
    /// all of them. One cause is one line, whatever the number of units
    /// ([`crate::output::notice`]).
    pub notes: Vec<Notice>,
}

/// Write the memory of every unit of `project` that has a home on this machine.
///
/// Every unit is compiled, not only the one a command names, because a ledger is a
/// statement about the others: a create, a merge or a reclaim changes what every
/// sibling should be told, and a memory that is only written for the unit somebody
/// touched is a memory that goes quietly out of date in all the rest.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read. A home that could not be
/// written is a note, not a failure: the command that called this did its own work, and
/// losing the memory of one unit is not a reason to report that the work failed.
pub fn refresh(conn: &Connection, project: &Project) -> Result<Report> {
    Ok(compile(project, &survey::project(conn, project)?))
}

/// The same, from a survey the caller has already taken.
///
/// The `ls` and `show` commands take the survey themselves, because they answer from it
/// as well as write from it ([`crate::runtime::ls::rows`]). Every other command calls
/// [`refresh`], which takes one and hands it here.
#[must_use]
pub fn compile(project: &Project, surveyed: &[survey::Snapshot]) -> Report {
    let command = test_command(&project.root);
    let mut report = Report::default();
    for subject in surveyed {
        report.notes.extend(
            subject.notes.iter().map(|cause| Notice::about(subject.unit.slug.to_string(), cause)),
        );
        let Some(home) = subject.home.as_ref().map(|environment| &environment.home) else {
            continue;
        };
        if !home.is_dir() {
            report
                .notes
                .push(Notice::about(subject.unit.slug.to_string(), "its home is not on this disk"));
            continue;
        }
        let ledger = ledger::of(subject, surveyed);
        let text = render::memory(subject, &ledger, command.as_deref());
        match write_home(home, &project.root, &text) {
            Ok(causes) => {
                report.written.push(home.join(FILE));
                report.notes.extend(
                    causes
                        .into_iter()
                        .map(|cause| Notice::about(subject.unit.slug.to_string(), cause)),
                );
            }
            Err(error) => {
                report.notes.push(Notice::about(subject.unit.slug.to_string(), error.to_string()));
            }
        }
    }
    report
}

/// The same, for the project a path is in. A path in no project writes nothing.
///
/// # Errors
/// As [`refresh`].
pub fn refresh_at(conn: &Connection, path: &Path) -> Result<Report> {
    match entry::project_at(conn, path)? {
        Some(project) => refresh(conn, &project),
        None => Ok(Report::default()),
    }
}

/// Furnish one home: the memory, the pointers that name it, and the Claude Code
/// settings a session in it reads.
///
/// A compile that produced the bytes the file already holds writes nothing. The memory
/// is written again by every command that touches the unit, and most of those change
/// nothing about it; rewriting it each time would tell every editor and agent watching
/// the file that something had happened, on every `nodal ls`.
///
/// The pointers are checked whichever way that goes. A person who removed one of them
/// gets it back on the next command, and a memory that did not change is no reason to
/// leave the file that names it missing.
///
/// The settings file is here rather than in the provider hook, and that is what makes
/// every home carry it: a unit made by `nodal new`, one adopted in place, and one the
/// `WorktreeCreate` hook made all pass through here. All three rules are the table's
/// ([`crate::env::files::WRITTEN`]), and Git is asked once for the whole of it.
fn write_home(home: &Path, root: &Path, text: &str) -> Result<Vec<String>> {
    let path = home.join(FILE);
    if !std::fs::read_to_string(&path).is_ok_and(|held| held == text) {
        atomic::write(&path, text)?;
    }
    let git = Git::at(home);
    let tracks = crate::env::files::tracked_of(home);
    let pointed = pointer::write(home, &git, &tracks)?;
    let mut notes = pointed.notes;
    notes.extend(claude_code::carry_settings(root, home, &git, &tracks)?);
    Ok(notes)
}

/// The command the project's own recipe calls its test suite, when it states one.
///
/// The file is read and inference is not run. What is wanted here is what the project
/// declared, and a compile happens on every command that touches a unit: a walk of the
/// project's files to guess at a test command would be paid for on all of them.
fn test_command(root: &Path) -> Option<String> {
    let path = root.join(recipe::FILE_NAME);
    let text = std::fs::read_to_string(&path).ok()?;
    let parsed = recipe::parse::parse(&text, &path).ok()?;
    parsed.commands.test.map(|command| command.to_string())
}

/// Report what a compile could not read, on standard error.
///
/// Where the caller is a command: the answer on standard output stays one document,
/// which is what `--json` needs, and a fact that is missing still says so.
pub fn report_notes(report: &Report) {
    for line in notice::collapse(&report.notes, "units") {
        eprintln!("nodal: context: {line}");
    }
}
