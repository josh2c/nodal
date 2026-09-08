//! `nodal doctor`: what this machine has left behind.
//!
//! Doctor is the first command a person runs, before any unit exists. A machine that has
//! been worked on for a year holds other checkouts another tool made, build caches
//! nothing has read for a month, containers that stopped weeks ago, volumes nothing
//! refers to, and directories named like databases that no registry row knows. Doctor
//! finds each of them, says how big it is, and stops there.
//!
//! **It removes nothing** (`decisions/DL-015`). Not as a default, and not with a flag
//! this module has. Every function here reads: a directory walk, `git worktree list`
//! and `git status`, `docker ps` and `docker system df`, and a SQL select. Git is run
//! with `GIT_OPTIONAL_LOCKS=0` (`git::cmd`), so not even the index is refreshed. The
//! report ends with a line that says so, the renderer is tested against the words it
//! must not use ([`crate::output::view::doctor`]), and the acceptance test compares the
//! name, size and modification time of every path of a machine before and after a
//! report.
//!
//! ## Two sections
//!
//! What belongs to the project the command was run in is one section. What belongs to
//! another project is the other, and it carries a name and a size and nothing else. A
//! person cleaning up one project must not be handed another project's unfinished work
//! to act on, so the second section states no branch, no dirty count and no intent.
//!
//! Which project a thing belongs to is answered by what named it, and not by where it
//! sits. A worktree this project's repository names is this project's whether its
//! directory is under the checkout, beside it, or on the other side of the machine, and
//! it goes in the first section with the branch, the state and the intent of any other.
//! Each source is therefore asked once per project root and every row of that answer
//! carries that project's section ([`worktrees::find`] takes it as an argument).
//!
//! ## What is never touched
//!
//! A worktree another tool holds a lock on is reported as locked, and nothing else is
//! read about it: not its Git state, not its size. A lock is a statement that a tool is
//! working in that directory, and doctor is not a tool that argues with one.
//!
//! ## One name per path
//!
//! One comparison is left in the worktree survey, and it is the reason this still
//! matters: a row is dropped when it is the checkout being surveyed. Doctor also
//! compares paths from four sources: the directory the command was run in, the roots
//! the registry holds, what `git worktree list` prints, and what Docker says a
//! container mounts. Git and Docker resolve every link before they answer; a person's
//! shell and a registry row do not. On a host whose temporary directory is a link —
//! macOS names `/var/folders` and means `/private/var/folders` — the same directory
//! therefore arrives under two names, and a comparison between them is false.
//!
//! So every path is resolved once, at the edge: on the way into [`Scope`], and in
//! [`Scope::section`] on the way in from a tool. Past that edge every path in this
//! module is the name the filesystem itself uses, and `starts_with` means what it reads
//! as. The resolver is [`guard::resolve`], which is the one place in Nodal a path is
//! normalised before it is compared with another; doctor does not have a rule of its
//! own about this.
//!
//! ## A tool that is not there
//!
//! Docker absent, or a daemon this account may not reach, is a note and not a failure
//! (`services::docker`, T1.10's pattern). A machine with no Docker still gets an answer
//! about its worktrees, its caches and its databases.

pub mod caches;
pub mod containers;
pub mod databases;
pub mod intent;
pub mod size;
pub mod units;
pub mod worktrees;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::Result;
use crate::git::Git;
use crate::lifecycle::guard;
use crate::model::{Project, Timestamp};
use crate::output::view::doctor::{Checkout, Doctor, Finding, Note};
use crate::services::docker::Docker;
use crate::store::projects;
use crate::workspace::home;

/// Where a finding belongs: to the project the command was run in, or to another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// This project.
    Here,
    /// Another project. Names and sizes only.
    Elsewhere,
}

/// One project the registry knows, and where its root really is.
#[derive(Debug, Clone)]
pub struct Known {
    /// The row, for its name and its identifier.
    pub project: Project,
    /// Its root, resolved by [`guard::resolve`], which is what every comparison uses.
    pub root: PathBuf,
}

/// What doctor reads, gathered once so that each source is a function of its inputs.
///
/// Every path here is resolved. Nothing in this type is the name a caller happened to
/// use for a directory; it is the name the filesystem uses.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// The top of the checkout the command was run in, when it was run in one.
    pub root: Option<PathBuf>,
    /// Every project the registry holds, with its root resolved.
    pub projects: Vec<Known>,
    /// The roots of every other project the registry knows.
    pub others: Vec<PathBuf>,
    /// Every path that belongs to another project: those roots, and the part of the
    /// state directory each of them owns.
    pub owned_elsewhere: Vec<PathBuf>,
    /// Where Nodal keeps its state, which is where databases and homes sit.
    pub state_dir: PathBuf,
    /// Where Claude Code keeps its session records, when this machine has them.
    pub sessions: Option<PathBuf>,
}

impl Scope {
    /// Which section a path belongs to.
    ///
    /// A path is another project's only when it is inside something that project owns.
    /// Everything else is this project's section, including a path nothing claims: "I
    /// cannot say whose this is" is not the same claim as "this is another project's",
    /// and the second section must hold only the second.
    ///
    /// The path is resolved first. This is the edge a tool's answer comes in at, and a
    /// tool that resolved links and a registry row that did not must not be able to
    /// name one directory two ways.
    #[must_use]
    pub fn section(&self, path: &Path) -> Section {
        let path = guard::resolve(path);
        if self.owned_elsewhere.iter().any(|owned| path.starts_with(owned)) {
            Section::Elsewhere
        } else {
            Section::Here
        }
    }
}

/// The three places doctor reads, named by the caller rather than by this module.
///
/// Every one of them is a parameter and not a lookup, so a test gives doctor a machine
/// it built and the report is a function of its inputs. [`Machine::here`] is what the
/// command line passes.
#[derive(Debug, Clone, Copy)]
pub struct Machine<'a> {
    /// The directory the command was run in.
    pub cwd: &'a Path,
    /// Where Nodal keeps its state on this machine.
    pub state_dir: &'a Path,
    /// Where Claude Code keeps its session records, when this machine has them.
    pub sessions: Option<&'a Path>,
}

impl<'a> Machine<'a> {
    /// This machine: the two paths the caller knows, and the session records where the
    /// tool that writes them puts them.
    #[must_use]
    pub fn here(cwd: &'a Path, state_dir: &'a Path, sessions: Option<&'a Path>) -> Self {
        Self { cwd, state_dir, sessions }
    }
}

/// Read this machine and say what it has left behind.
///
/// `now` dates the answer and is what staleness is measured from, so a report is a
/// function of its inputs.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read, and whatever a Git or
/// Docker read reported that is not a condition of the machine.
pub fn survey(
    conn: &Connection,
    docker: &dyn Docker,
    machine: &Machine<'_>,
    now: Timestamp,
) -> Result<Doctor> {
    let scope = scope_of(conn, machine)?;
    let mut here = Vec::new();
    let mut elsewhere = Vec::new();
    let mut notes = Vec::new();

    for root in scope.root.iter().chain(&scope.others) {
        let section = scope.section(root);
        let found = worktrees::find(root, scope.sessions.as_deref(), section)?;
        push(&mut here, &mut elsewhere, section, found);
        push(&mut here, &mut elsewhere, section, caches::find(root, now));
    }
    let (found, note) = containers::find(conn, docker, &scope)?;
    notes.extend(note);
    for (section, finding) in found {
        push(&mut here, &mut elsewhere, section, vec![finding]);
    }
    for (section, finding) in databases::find(conn, &scope)? {
        push(&mut here, &mut elsewhere, section, vec![finding]);
    }
    for (section, finding) in units::find(conn, &scope)? {
        push(&mut here, &mut elsewhere, section, vec![finding]);
    }

    largest_first(&mut here);
    largest_first(&mut elsewhere);
    Ok(Doctor { now, checkout: checkout_of(&scope), here, elsewhere, notes })
}

/// Add findings to the section they belong to.
fn push(
    here: &mut Vec<Finding>,
    elsewhere: &mut Vec<Finding>,
    section: Section,
    findings: Vec<Finding>,
) {
    match section {
        Section::Here => here.extend(findings),
        Section::Elsewhere => elsewhere.extend(plain(findings)),
    }
}

/// The same findings with everything but the name, the kind and the size taken off.
///
/// This is the rule of the second section, applied in one place rather than trusted to
/// every source: another project's state and another project's intent are not shown.
fn plain(findings: Vec<Finding>) -> Vec<Finding> {
    findings
        .into_iter()
        .map(|finding| Finding { state: Vec::new(), intent: None, ..finding })
        .collect()
}

/// Order findings by what they cost, largest first, then by name so that two runs over
/// one machine print the same list.
fn largest_first(findings: &mut [Finding]) {
    findings.sort_by(|left, right| {
        right
            .bytes
            .unwrap_or(0)
            .cmp(&left.bytes.unwrap_or(0))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.what.cmp(&right.what))
    });
}

/// What doctor is going to read, gathered from the registry and the machine.
fn scope_of(conn: &Connection, machine: &Machine<'_>) -> Result<Scope> {
    let state_dir = guard::resolve(machine.state_dir);
    let root =
        Git::open(machine.cwd).and_then(|git| git.toplevel()).ok().map(|top| guard::resolve(&top));
    let projects: Vec<Known> = projects::list(conn)?
        .into_iter()
        .map(|project| Known { root: guard::resolve(&project.root), project })
        .collect();
    let mut others = Vec::new();
    let mut owned_elsewhere = Vec::new();
    for known in &projects {
        if root.as_ref() == Some(&known.root) {
            continue;
        }
        owned_elsewhere.push(state_dir.join(home::project_segment(&known.project.name)));
        owned_elsewhere.push(known.root.clone());
        others.push(known.root.clone());
    }
    Ok(Scope {
        root,
        projects,
        others,
        owned_elsewhere,
        state_dir,
        sessions: machine.sessions.map(guard::resolve),
    })
}

/// The checkout line of the report: where the command ran, and what the registry calls
/// the project there.
///
/// The project is found among the roots the scope already resolved, and not by a query
/// on the path. A registry row holds the path whoever wrote it used, which on a host
/// with a linked temporary directory is not the path this command resolved.
fn checkout_of(scope: &Scope) -> Option<Checkout> {
    let root = scope.root.as_ref()?;
    let project = scope
        .projects
        .iter()
        .find(|known| &known.root == root)
        .map(|known| known.project.name.to_string());
    Some(Checkout { root: root.clone(), project })
}

/// A note about a source that could not answer.
#[must_use]
pub fn note(source: &str, why: String) -> Note {
    Note { source: source.to_owned(), why }
}
