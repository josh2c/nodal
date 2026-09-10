//! `nodal adopt`: make a unit of a checkout that is already there, or of a branch.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use nodal_core::doctor;
use nodal_core::lifecycle::ops::adopt::{self, Request};
use nodal_core::model::{Objective, Slug};
use nodal_core::output::{self, Format};
use nodal_core::runtime::entry;
use nodal_core::store::Store;
use nodal_core::substrate::{self, Reporter};

/// Arguments of `nodal adopt`.
#[derive(Debug, Args)]
pub struct Adopt {
    /// A branch of the project, or the directory of a checkout.
    #[arg(value_name = "BRANCH-OR-PATH", required_unless_present = "all")]
    pub target: Option<String>,

    /// Adopt every worktree of the project. Skip the main checkout and any already a
    /// unit, and say so.
    #[arg(long, conflicts_with_all = ["target", "name", "objective"])]
    pub all: bool,

    /// Make the checkout a unit where it stands, writing only `.nodal/` and `.envrc`
    /// and hiding both from Git. Required for a directory: Nodal does not move a
    /// checkout somebody is working in.
    #[arg(long)]
    pub in_place: bool,

    /// What the unit is for. Without it, a checkout an agent tool made is asked what
    /// its first session was started to do.
    #[arg(short = 'm', long, value_name = "TEXT")]
    pub objective: Option<String>,

    /// The handle to give the unit, instead of deriving one from the branch.
    #[arg(long, value_name = "SLUG")]
    pub name: Option<String>,

    /// A directory in the project. Defaults to the working directory.
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Adopt {
    /// Adopt the target, report the unit it became, and offer its home to the shell.
    ///
    /// # Errors
    ///
    /// Propagates a target that cannot become a unit, a branch another open unit holds,
    /// and whatever Git, the filesystem or the registry reported.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let progress: Arc<dyn Reporter> = substrate::sink(self.json);
        let request = self.request(hooks)?;
        let format = Format::from_json_flag(self.json);
        if self.all {
            let report = adopt::adopt_all(store, &request, &progress)?;
            output::write(&report, format, &mut std::io::stdout())?;
            return Ok(if report.failed() { ExitCode::FAILURE } else { ExitCode::SUCCESS });
        }
        let report = adopt::adopt(store, &request, &progress)?;
        output::write(&report, format, &mut std::io::stdout())?;
        if let Some(environment) = &report.unit.environment {
            entry::ask_to_enter(&environment.home)?;
        }
        Ok(ExitCode::SUCCESS)
    }

    /// The arguments as the values the operation takes.
    ///
    /// Where the session records are is asked here rather than in the operation, for
    /// the reason `nodal doctor` asks it here: reading an environment variable is the
    /// command's business, and a test that points the recovery at a directory of its own
    /// needs the operation to take the answer rather than find it.
    fn request(&self, hooks: bool) -> nodal_core::Result<Request> {
        Ok(Request {
            target: self.target.clone().unwrap_or_default(),
            cwd: self.path.clone().unwrap_or_else(|| PathBuf::from(".")),
            in_place: self.in_place,
            objective: self.objective.as_deref().map(Objective::parse).transpose()?,
            name: self.name.as_deref().map(Slug::parse).transpose()?,
            sessions: doctor::intent::config_directory(),
            hooks,
        })
    }
}
