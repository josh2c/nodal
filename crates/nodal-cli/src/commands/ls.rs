//! `nodal ls`: every unit of the project, and what each one needs next.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use nodal_core::context::survey::{self, Snapshot};
use nodal_core::doctor::intent;
use nodal_core::lifecycle::guard;
use nodal_core::lifecycle::ops::new;
use nodal_core::lifecycle::states;
use nodal_core::model::{Project, ProjectName, Timestamp};
use nodal_core::output::view::{UnitList, Verdict, WorktreeRow};
use nodal_core::output::{self, Format};
use nodal_core::runtime::{entry, lock, ls, processes, verdict};
use nodal_core::store::Store;

use crate::commands::context;

/// Arguments of `nodal ls`.
#[derive(Debug, Default, Args)]
pub struct Ls {
    /// A directory in the project. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

/// One reading of a project: the survey every home was asked for, and the list built
/// from it.
///
/// The two are kept together because they are one pass. The rows are what the person
/// asked for; the survey is what the memories are compiled from, and compiling from a
/// second reading would ask every home the same questions again.
pub struct Listing {
    /// The project the units belong to.
    pub project: Project,
    /// What every unit's home answered, once.
    pub surveyed: Vec<Snapshot>,
    /// The rows, ordered as the list prints them.
    pub list: UnitList,
}

impl Listing {
    /// Record the units the reading found merged, and show them so.
    ///
    /// The first of the two things the command layer records. It is here rather than in
    /// the reading because `docs/contracts.md` puts it here, and because a reading that
    /// writes is a reading nothing else can safely reuse.
    pub fn settle(&mut self, store: &Store) {
        let notes = states::settle(store.conn(), &mut self.list.units, self.list.now);
        self.list.notes.extend(notes);
    }

    /// Write every unit's memory again, from the survey the rows were built from.
    ///
    /// The second. The list is the command a person types most, so it is the one that
    /// keeps the memories current for the units nobody has touched today.
    pub fn compile(&self) {
        context::compile(&self.project, &self.surveyed);
    }
}

/// What a directory turned out to be.
///
/// Three answers, not two. A directory Nodal has recorded units of is a [`Self::Listed`]
/// project. A directory that holds a `nodal.toml` and no units is [`Self::Declared`]:
/// `nodal init` has been run there and `nodal new` has not, which is the ordinary state
/// of a project in the minute after it was set up. Everything else is
/// [`Self::Unknown`].
///
/// The middle answer exists because the field reported the wrong one being given for
/// it. `nodal ls` straight after `nodal init` said the directory was in no project
/// Nodal knows and told the person to run `nodal new` — while the recipe that command
/// had just written lay in the directory. The registry was right and the sentence was
/// not: nothing had been recorded, but the project was there to see.
pub enum Reading {
    /// A project the registry holds units of.
    Listed(Box<Listing>),
    /// A project declared by a `nodal.toml` and holding no units yet, with whatever
    /// worktrees its repository names.
    ///
    /// The worktrees come with it because they are the reason the answer is worth
    /// printing. A person who has just run `nodal init` in a checkout with nine
    /// worktrees is told there are no units yet, and told what the nine are.
    Declared(Box<UnitList>),
    /// A checkout Nodal holds nothing about, and the verdict on its worktrees.
    ///
    /// The fourth answer, and the one a person meets first. A repository with no recipe
    /// and no registry row is not a directory Nodal has nothing to say about: it is a
    /// repository whose other worktrees Git already knows, and every fact the verdict
    /// prints is one a read can reach. So the answer is that table, and reaching it
    /// initialises nothing ([`verdict`]).
    Checkout(Box<Verdict>),
    /// A directory that is no project of Nodal's and no checkout either.
    Unknown,
}

impl Ls {
    /// Print every unit of the project the directory is in.
    ///
    /// Three steps, and the order is the contract: the reading, then the two things the
    /// command layer records from it, then the answer. `runtime::ls` writes nothing, so
    /// both writes are here — the flip to merged, which the reading found, and every
    /// unit's memory, which is compiled from the same survey the rows were built from.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::ProjectNotFound`] when the directory is in no project Nodal
    /// records, and whatever the registry or Git reported.
    pub fn run(&self, store: Option<&Store>) -> nodal_core::Result<ExitCode> {
        let path = self.directory()?;
        match self.read(store)? {
            Reading::Listed(mut listing) => {
                // A listed project came from a registry, so there is one to settle
                // into. The two writes the list makes are the command layer's, and
                // `docs/contracts.md` names both.
                if let Some(store) = store {
                    listing.settle(store);
                    listing.compile();
                }
                self.print(&listing.list)
            }
            Reading::Declared(empty) => self.print(empty.as_ref()),
            Reading::Checkout(seen) => self.print(seen.as_ref()),
            Reading::Unknown => Err(verdict::nowhere(&path)),
        }
    }

    /// The list of a project that has no units: the empty list, and the command that
    /// makes the first one.
    ///
    /// It is the ordinary empty list and not a special answer, so a tool reading
    /// `--json` gets the shape it gets everywhere else, with no units in it. The line
    /// under it is a note for the same reason every other note is one: it is something
    /// the person should read, and it is not a row.
    pub(crate) fn nothing_yet(project: ProjectName, root: &Path) -> UnitList {
        let now = Timestamp::now();
        let sessions = intent::config_directory();
        let worktrees = verdict::read(root, sessions.as_deref(), Some(project.clone()), now)
            .map(|seen| seen.rows)
            .unwrap_or_default();
        UnitList {
            project,
            now,
            units: Vec::new(),
            worktrees,
            notes: vec![String::from(
                "nodal.toml is here and no unit has been made yet; run `nodal new \"<what \
                 the work is>\"` to make the first",
            )],
        }
    }

    /// What the directory is: a recorded project, a declared one, or neither.
    ///
    /// The registry is asked first, because a project with units is recorded whatever
    /// else is on the disk. The disk is asked second, and only about one file.
    ///
    /// A bare `nodal` is both the list and the first command a person ever types, so a
    /// directory that is neither falls back to the help rather than to an error. That
    /// fallback is the caller's, because the help belongs to the argument parser.
    ///
    /// # Errors
    ///
    /// Whatever the registry or Git reported.
    pub fn read(&self, store: Option<&Store>) -> nodal_core::Result<Reading> {
        let path = self.directory()?;
        // A machine with no registry has no projects, and asking one that is not there
        // is the same question with the same answer. The file is not made to ask it.
        let recorded = match store {
            Some(store) => entry::project_at(store.conn(), &path)?,
            None => None,
        };
        let Some(project) = recorded else {
            if let Some(root) = entry::declared_at(&path) {
                let named = new::name_of(&root);
                return Ok(Reading::Declared(Box::new(Self::nothing_yet(named, &root))));
            }
            return Ok(Self::checkout(&path));
        };
        let now = Timestamp::now();
        let Some(store) = store else { return Ok(Self::checkout(&path)) };
        let surveyed = survey::project(store.conn(), &project)?;
        let held = Self::holders(store, &project, now)?;
        let mut list = ls::rows(&surveyed, &processes::Live, &project, &held, now);
        list.worktrees = Self::foreign(&project, &surveyed, now);
        Ok(Reading::Listed(Box::new(Listing { project, surveyed, list })))
    }

    /// Who holds the write on each of a project's units, with lapsed holds left out.
    ///
    /// Read once for the whole list. WHO puts this before the process table because a
    /// process scan reads `/proc`, which does not cross Linux accounts: on a host two
    /// engineers share, a lock row is the only signal that sees the other person.
    fn holders(
        store: &Store,
        project: &nodal_core::model::Project,
        now: Timestamp,
    ) -> nodal_core::Result<ls::Held> {
        let held = lock::live(store.conn(), &project.root, now)?;
        Ok(ls::Held::of(&held, lock::idle_hours(&project.root)))
    }

    /// The verdict on a checkout the registry holds nothing about.
    ///
    /// A directory Git does not know is [`Reading::Unknown`], and so is a checkout
    /// whose own record of its worktrees could not be read: both are "Nodal cannot say
    /// what is here", and the caller turns that into the one sentence or the help.
    fn checkout(path: &Path) -> Reading {
        let Some(root) = verdict::checkout_at(path) else { return Reading::Unknown };
        let sessions = intent::config_directory();
        match verdict::read(&root, sessions.as_deref(), None, Timestamp::now()) {
            Ok(seen) => Reading::Checkout(Box::new(seen)),
            Err(error) => {
                tracing::debug!(%error, "the checkout's record of its worktrees could not be read");
                Reading::Unknown
            }
        }
    }

    /// The worktrees of a registered project's repository that are not units.
    ///
    /// A unit is a clone and not a worktree, so the two sets rarely overlap; an adopted
    /// unit is the case where they do, because `nodal adopt` takes a checkout somebody
    /// else made and that checkout may be a worktree of this repository. A row for one
    /// of those would be the same directory twice, once under the word `unit` and once
    /// under the word `worktree`, so the homes the registry holds are taken out.
    ///
    /// A repository that could not be read costs the foreign rows and nothing else. The
    /// units are what a person asked for and they are already in hand.
    fn foreign(project: &Project, surveyed: &[Snapshot], now: Timestamp) -> Vec<WorktreeRow> {
        let sessions = intent::config_directory();
        let Ok(seen) = verdict::read(&project.root, sessions.as_deref(), None, now) else {
            return Vec::new();
        };
        let homes: HashSet<PathBuf> = surveyed
            .iter()
            .filter_map(|subject| subject.home.as_ref())
            .map(|environment| guard::resolve(&environment.home))
            .collect();
        seen.rows.into_iter().filter(|row| !homes.contains(&row.path)).collect()
    }

    /// Render the list in the format the arguments asked for.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Render`] when the list cannot be encoded as JSON.
    pub fn print<A: output::Render>(&self, answer: &A) -> nodal_core::Result<ExitCode> {
        output::write(answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// The directory the project is read from.
    fn directory(&self) -> nodal_core::Result<PathBuf> {
        match &self.path {
            Some(given) => Ok(given.clone()),
            None => std::env::current_dir().map_err(nodal_core::Error::io(".")),
        }
    }
}
