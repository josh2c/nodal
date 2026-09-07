//! `nodal base`: the warm bases this machine keeps for a project.
//!
//! Nothing here builds anything of its own. `nodal base build` is `nodal new`'s first
//! step made early, so a person who knows they are about to want a unit can pay for
//! the clone and the install while they are still doing something else.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Subcommand};
use nodal_core::git::Git;
use nodal_core::lifecycle::ops::new::ensure_project;
use nodal_core::model::recipe::Recipe;
use nodal_core::model::{Project, Timestamp};
use nodal_core::output::view::{BaseBuild, BaseList, BaseRow, BaseSweep};
use nodal_core::output::{self, Format, Render};
use nodal_core::store::Store;
use nodal_core::substrate::{self, Reporter};
use nodal_core::workspace::home;
use nodal_core::{Result, recipe};

/// Arguments of `nodal base`.
#[derive(Debug, Args)]
pub struct Base {
    /// What to do with the project's bases.
    #[command(subcommand)]
    pub action: Action,
}

/// The things `nodal base` does.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// List the bases this machine holds for the project.
    Ls(Common),
    /// Build the base this workspace needs, if there is not one already.
    Build(Build),
    /// Remove the bases the project no longer needs.
    Gc(Gc),
}

/// What every action takes.
#[derive(Debug, Args)]
pub struct Common {
    /// A directory in the project. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Common {
    /// The directory to read the project from.
    fn root(&self) -> PathBuf {
        self.path.clone().unwrap_or_else(|| PathBuf::from("."))
    }

    /// Where a build says what it is doing.
    fn progress(&self) -> Arc<dyn Reporter> {
        substrate::sink(self.json)
    }

    /// The project this action is about, recorded if this is the first time Nodal has
    /// seen it, with the recipe it was read against.
    ///
    /// The root is the repository's top level, never the directory the command was run
    /// in. A project is one row however deep in it a person is standing, and the base
    /// `nodal base build` warms is only of use if it is the one `nodal new` then finds.
    fn project(&self, store: &mut Store) -> Result<(Project, Recipe)> {
        let root = Git::open(self.root())?.top_level()?;
        let recipe = recipe::load(&root)?.recipe;
        let project = ensure_project(store, &root, &recipe)?;
        Ok((project, recipe))
    }
}

/// Arguments of `nodal base build`.
#[derive(Debug, Args)]
pub struct Build {
    /// Where and how to answer.
    #[command(flatten)]
    pub common: Common,

    /// Run the project's build command once dependencies are installed.
    #[arg(long)]
    pub warm: bool,
}

/// Arguments of `nodal base gc`.
#[derive(Debug, Args)]
pub struct Gc {
    /// Where and how to answer.
    #[command(flatten)]
    pub common: Common,

    /// One base to remove, by the identifier `nodal base ls` prints.
    #[arg(value_name = "BASE")]
    pub base: Option<String>,

    /// How many idle bases to keep. Ignored when a base is named.
    #[arg(long, value_name = "N", default_value_t = substrate::DEFAULT_KEEP)]
    pub keep: usize,
}

impl Base {
    /// Run the action that was asked for.
    ///
    /// # Errors
    ///
    /// Propagates whatever the registry, Git or a build step reports.
    pub fn run(&self, store: &mut Store) -> Result<ExitCode> {
        match &self.action {
            Action::Ls(common) => ls(common, store),
            Action::Build(build) => build_one(build, store),
            Action::Gc(gc) => collect(gc, store),
        }
    }
}

/// Every base of the project, with the units holding each one.
fn ls(common: &Common, store: &mut Store) -> Result<ExitCode> {
    let (project, _) = common.project(store)?;
    let bases = substrate::list(store, project.id)?;
    write(&BaseList { now: Timestamp::now(), bases }, common.json)
}

/// The base this workspace needs, built if it is not already there.
fn build_one(args: &Build, store: &mut Store) -> Result<ExitCode> {
    let common = &args.common;
    let (project, recipe) = common.project(store)?;
    let request = substrate::Request {
        source: project.root.clone(),
        project,
        recipe,
        state_dir: home::directory()?,
        warm: args.warm,
    };
    let outcome = substrate::ensure(store, &request, &common.progress())?;
    let pins = substrate::pins(store, outcome.base.id)?;
    let answer = BaseBuild {
        now: Timestamp::now(),
        built: outcome.built(),
        origin: outcome.origin.as_ref().map(substrate::Origin::describe),
        base: BaseRow { base: outcome.base, pins, disk_bytes: None },
    };
    write(&answer, common.json)
}

/// Remove the base that was named, or the idle ones beyond the number kept.
fn collect(args: &Gc, store: &mut Store) -> Result<ExitCode> {
    let common = &args.common;
    let (project, _) = common.project(store)?;
    let progress = common.progress();
    let removed = match &args.base {
        Some(name) => {
            let base = substrate::resolve(store, project.id, name)?;
            vec![substrate::evict(store, base.id, progress.as_ref())?]
        }
        None => substrate::gc(store, project.id, args.keep, progress.as_ref())?,
    };
    write(&BaseSweep { now: Timestamp::now(), keep: args.keep, removed }, common.json)
}

/// Print a read type in the format asked for.
fn write<T: Render>(answer: &T, json: bool) -> Result<ExitCode> {
    output::write(answer, Format::from_json_flag(json), &mut std::io::stdout())?;
    Ok(ExitCode::SUCCESS)
}
