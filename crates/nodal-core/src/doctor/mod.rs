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
//! Which project a thing belongs to is usually a question a path answers. A Docker
//! resource often has no path to answer with, and it carries a project's name in its
//! own name instead. So a name that positively matches another project doctor knows is
//! evidence, and that row goes in the second section ([`attribution`]).
//!
//! ## A third section: the branches
//!
//! Everything above is anchored to a directory. [`branches`] is not, and that is why it
//! is there. A measured machine held 317 local branches, 306 of them with no worktree,
//! and 22 of those held commits that exist on no remote. Nothing anchored to a
//! directory could report one of them, so an all-clear about the worktrees was true and
//! said nothing about the only work on that machine that was not backed up anywhere.
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
//!
//! ## A registry a later Nodal wrote
//!
//! The same rule, one source further in. A registry written by a later Nodal is refused
//! by the store (`Error::StoreTooNew`), and this is the one command where that must not
//! end the answer: doctor is what a person runs when something is wrong. So the
//! registry is a parameter with two shapes ([`Registry`]). A registry that is too new
//! reads as no registry at all — the worktrees and the caches of the checkout are still
//! reported — and the mismatch becomes a note that names both schema versions and the
//! one command that upgrades this copy of Nodal (DL-034). Nothing is fetched to say it.

pub mod attribution;
pub mod branches;
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
use crate::output::view::doctor::{Branches, Checkout, Doctor, Finding, Note};
use crate::services::docker::Docker;
use crate::store::projects;
use crate::workspace::home;
use crate::workspace::sharing::Sharing;

/// A registry a later Nodal wrote, and what a person does about it.
///
/// Every field is given by the caller, including the upgrade command, so that the note
/// this makes is a function of its inputs like every other answer in this module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The registry file that was refused.
    pub path: PathBuf,
    /// The schema version the file carries.
    pub found: u32,
    /// The schema version this binary knows.
    pub supported: u32,
    /// The one command that upgrades this copy of Nodal
    /// ([`crate::setup::channel`]).
    pub upgrade: String,
}

impl Mismatch {
    /// The note the report carries: what was read, and what to do about it.
    #[must_use]
    pub fn note(&self) -> Note {
        Note {
            source: String::from("store"),
            why: format!(
                "{} is at schema {}. this nodal knows schema {}. nothing in the registry was \
                 read. upgrade this nodal with: {}",
                self.path.display(),
                self.found,
                self.supported,
                self.upgrade
            ),
        }
    }
}

/// What doctor reads the registry with.
///
/// Two shapes, because a registry this binary cannot open is a fact about the machine
/// and not a reason to print nothing.
#[derive(Debug, Clone)]
pub enum Registry<'a> {
    /// The registry, at a schema this binary knows.
    Open(&'a Connection),
    /// The registry was written by a later Nodal, so none of it was read.
    TooNew(Mismatch),
}

impl Registry<'_> {
    /// The connection, when there is one to read.
    #[must_use]
    pub const fn connection(&self) -> Option<&Connection> {
        match self {
            Self::Open(conn) => Some(conn),
            Self::TooNew(_) => None,
        }
    }

    /// The note this registry adds to the report, when it adds one.
    #[must_use]
    pub fn note(&self) -> Option<Note> {
        match self {
            Self::Open(_) => None,
            Self::TooNew(mismatch) => Some(mismatch.note()),
        }
    }
}

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

    /// Whether a path is inside the checkout the command was run in.
    ///
    /// [`Scope::section`] cannot answer this. It says `Here` both for a path this
    /// project owns and for a path nothing claims, which is what makes it safe. This
    /// says only the first, and it is what stops a name from moving a container that is
    /// standing in this project's own tree ([`containers`]).
    #[must_use]
    pub fn owns(&self, path: &Path) -> bool {
        let path = guard::resolve(path);
        self.root.as_ref().is_some_and(|root| path.starts_with(root))
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
    /// What Nodal recorded about sharing file blocks under the state root, and `None`
    /// where nothing recorded anything.
    ///
    /// The caller reads the record ([`Sharing::read`]); it never probes. A probe writes
    /// a file into the state root, and doctor writes nothing to the machine it reports
    /// on. A machine with no record is reported as having none.
    pub sharing: Option<&'a Sharing>,
}

impl<'a> Machine<'a> {
    /// This machine: the two paths the caller knows, the session records where the
    /// tool that writes them puts them, and the recorded answer about the state root.
    #[must_use]
    pub fn here(
        cwd: &'a Path,
        state_dir: &'a Path,
        sessions: Option<&'a Path>,
        sharing: Option<&'a Sharing>,
    ) -> Self {
        Self { cwd, state_dir, sessions, sharing }
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
    registry: &Registry<'_>,
    docker: &dyn Docker,
    machine: &Machine<'_>,
    now: Timestamp,
) -> Result<Doctor> {
    let conn = registry.connection();
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
    if let Some(conn) = conn {
        let (found, said) = registered(conn, docker, &scope)?;
        notes.extend(said);
        for (section, finding) in found {
            push(&mut here, &mut elsewhere, section, vec![finding]);
        }
    }
    notes.extend(registry.note());
    let branches = match &scope.root {
        Some(root) => branches::find(root, now)?,
        None => Branches::default(),
    };

    largest_first(&mut here);
    largest_first(&mut elsewhere);
    Ok(Doctor {
        now,
        checkout: checkout_of(&scope),
        state_root: machine.state_dir.to_path_buf(),
        sharing: machine.sharing.cloned(),
        here,
        elsewhere,
        branches,
        notes,
    })
}

/// What the registry-reading sources answered: the rows, and what could not be read.
type Registered = (Vec<(Section, Finding)>, Vec<Note>);

/// The three sources that read the registry: containers, databases and unit counts.
///
/// They are together because they share one condition. Each of them answers "whose is
/// this?" out of the registry, so a machine whose registry could not be opened has no
/// answer from any of them rather than a partial one.
fn registered(conn: &Connection, docker: &dyn Docker, scope: &Scope) -> Result<Registered> {
    let (mut found, note) = containers::find(conn, docker, scope)?;
    found.extend(databases::find(conn, scope)?);
    found.extend(units::find(conn, scope)?);
    Ok((found, note.into_iter().collect()))
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
/// A registry that is not there names no project, so every path the walk finds belongs
/// to the checkout the command was run in. That is the conservative direction and the
/// one [`Scope::section`] already documents: "I cannot say whose this is" is the first
/// section, never the second.
fn scope_of(conn: Option<&Connection>, machine: &Machine<'_>) -> Result<Scope> {
    let state_dir = guard::resolve(machine.state_dir);
    let root =
        Git::open(machine.cwd).and_then(|git| git.toplevel()).ok().map(|top| guard::resolve(&top));
    let listed = match conn {
        Some(conn) => projects::list(conn)?,
        None => Vec::new(),
    };
    let projects: Vec<Known> = listed
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Mismatch, Registry};

    fn mismatch() -> Mismatch {
        Mismatch {
            path: PathBuf::from("/home/dev/.nodal/registry.db"),
            found: 7,
            supported: 6,
            upgrade: String::from("brew upgrade nodal"),
        }
    }

    #[test]
    fn the_note_says_what_was_not_read_and_what_to_do_about_it() {
        let note = mismatch().note();
        assert_eq!(note.source, "store");
        assert!(note.why.contains("/home/dev/.nodal/registry.db"), "{}", note.why);
        assert!(note.why.contains("schema 7"), "{}", note.why);
        assert!(note.why.contains("schema 6"), "{}", note.why);
        assert!(note.why.contains("brew upgrade nodal"), "{}", note.why);
    }

    #[test]
    fn a_registry_that_is_too_new_reads_as_no_registry_at_all() {
        let refused = Registry::TooNew(mismatch());
        assert!(refused.connection().is_none());
        assert!(refused.note().is_some());
    }
}
