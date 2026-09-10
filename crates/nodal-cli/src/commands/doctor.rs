//! `nodal doctor`: report what this machine has left behind, and remove nothing.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::doctor::{self, Mismatch};
use nodal_core::model::Timestamp;
use nodal_core::output::{self, Format};
use nodal_core::services::docker;
use nodal_core::setup::channel;
use nodal_core::store::Store;
use nodal_core::workspace::home;
use nodal_core::workspace::sharing::Sharing;

/// The registry, or the version mismatch that stands in its place.
enum Opened {
    /// The registry, at a schema this binary knows.
    Store(Store),
    /// The registry was written by a later Nodal, so none of it was read.
    TooNew(Mismatch),
}

/// Arguments of `nodal doctor`.
#[derive(Debug, Args)]
pub struct Doctor {
    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
    /// Print every branch, not only the ones holding commits no remote has.
    #[arg(long)]
    pub all: bool,
    /// Scan repositories under these roots. With no path, scan the home directory.
    #[arg(long, num_args = 0.., value_name = "ROOT")]
    pub machine: Option<Vec<PathBuf>>,
    /// How many directory levels `--machine` walks from each root.
    #[arg(long, default_value_t = doctor::machine::DEFAULT_DEPTH, value_name = "N")]
    pub depth: usize,
}

impl Doctor {
    /// Read this machine and print what it holds.
    ///
    /// The report writes nothing. It reads the registry, the checkout it was run in,
    /// Nodal's state directory and, where a daemon answers, Docker, and it leaves every
    /// one of them as it found them.
    ///
    /// One thing happens before the report that is not the report. Every command that
    /// opens the registry finishes or rolls back an operation an earlier run was killed
    /// in the middle of ([`crate::cli::Cli`]), and that preamble writes. It says what it
    /// did on standard error, and it is the same preamble `nodal ls` and `nodal ps` run.
    /// Nothing doctor itself does writes anything.
    ///
    /// `--machine [ROOT ...]` walks for `.git` under each root. With no path, it walks
    /// the home directory. `--depth` bounds the walk. The checkout survey stays the
    /// default of a bare `nodal doctor`.
    ///
    /// `--all` opens the branch section. The survey reads every branch either way and
    /// `--json` carries every row either way; the flag decides how many of the safe
    /// buckets the human rendering prints. A machine with 306 branches and 22 of them
    /// unbacked-up needs the 22 read, and 284 safe rows above them is how a person
    /// stops reading.
    ///
    /// `registry` is a `Result` on purpose. A registry a later Nodal wrote is refused
    /// by the store, and this is the one command where that must not end the answer:
    /// doctor is what a person runs when something is wrong. The refusal becomes a note
    /// naming both schema versions and the one command that upgrades this copy, and the
    /// worktrees and caches of the checkout are reported as usual.
    ///
    /// # Errors
    ///
    /// Propagates a registry that could not be read for any reason other than its
    /// version, and a state directory that nothing says the place of. A Docker daemon
    /// that is not there is a note in the answer, not a failure.
    pub fn run(&self, registry: nodal_core::Result<Store>) -> nodal_core::Result<ExitCode> {
        let opened = Self::open(registry)?;
        if self.machine.is_some() {
            return self.run_machine(&opened);
        }
        self.run_checkout(&opened)
    }

    /// The checkout survey: this project, other projects, and the branch audit.
    fn run_checkout(&self, opened: &Opened) -> nodal_core::Result<ExitCode> {
        let source = Self::source(opened);
        let cwd = std::env::current_dir().map_err(nodal_core::Error::io("<cwd>"))?;
        let state_dir = home::directory()?;
        let sessions = doctor::intent::config_directory();
        // Read, never probe: a probe writes a file into the state root, and this
        // command writes nothing to the machine it reports on. The record is taken when
        // the state root is made, and again when `nodal init --reprobe` asks.
        let sharing = Sharing::read(&state_dir);
        let machine =
            doctor::Machine::here(&cwd, &state_dir, sessions.as_deref(), sharing.as_ref());
        let mut answer = doctor::survey(&source, &docker::Cli, &machine, Timestamp::now())?;
        answer.branches.expand = self.all;
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// The machine survey: every repository under the roots, grouped by remote.
    fn run_machine(&self, opened: &Opened) -> nodal_core::Result<ExitCode> {
        let source = Self::source(opened);
        let roots = self.machine_roots()?;
        let state_dir = home::directory()?;
        let request =
            doctor::machine::Request { roots: &roots, depth: self.depth, state_dir: &state_dir };
        let answer = doctor::machine::survey(&source, &request, Timestamp::now())?;
        output::write(&answer, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// Roots to walk. With no path, the person's home directory.
    fn machine_roots(&self) -> nodal_core::Result<Vec<PathBuf>> {
        match &self.machine {
            Some(roots) if roots.is_empty() => Ok(vec![home::user()?]),
            Some(roots) => Ok(roots.clone()),
            None => Ok(vec![home::user()?]),
        }
    }

    /// The registry as the survey reads it.
    fn source(opened: &Opened) -> doctor::Registry<'_> {
        match opened {
            Opened::Store(store) => doctor::Registry::Open(store.conn()),
            Opened::TooNew(mismatch) => doctor::Registry::TooNew(mismatch.clone()),
        }
    }

    /// The registry as doctor reads it, with a version refusal turned into a fact.
    ///
    /// The upgrade command is read here rather than inside the survey, so that the
    /// survey stays a function of its inputs.
    fn open(registry: nodal_core::Result<Store>) -> nodal_core::Result<Opened> {
        match registry {
            Ok(store) => Ok(Opened::Store(store)),
            Err(nodal_core::Error::StoreTooNew { path, found, supported }) => {
                Ok(Opened::TooNew(Mismatch {
                    path,
                    found,
                    supported,
                    upgrade: channel::read().command,
                }))
            }
            Err(other) => Err(other),
        }
    }
}
