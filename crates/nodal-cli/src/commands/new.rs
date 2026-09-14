//! `nodal new`: make a unit, its branch and a home to work in.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use nodal_core::lifecycle::ops::new::{self, Request};
use nodal_core::model::{BranchName, Epistemic, Objective, Slug};
use nodal_core::output::{self, Format};
use nodal_core::runtime::entry;
use nodal_core::store::Store;
use nodal_core::substrate::{self, Reporter};

use crate::commands::context;

/// Arguments of `nodal new`.
#[derive(Debug, Args)]
pub struct New {
    /// What the unit is for. The handle is derived from it unless `--name` says.
    #[arg(value_name = "OBJECTIVE")]
    pub objective: Option<String>,

    /// The handle to give the unit, instead of deriving one.
    #[arg(long, value_name = "SLUG")]
    pub name: Option<String>,

    /// The branch the work starts from.
    #[arg(long, value_name = "BRANCH")]
    pub from: Option<String>,

    /// Start the unit at this checkout's HEAD with its uncommitted work copied in:
    /// staged as staged, unstaged as unstaged, untracked as untracked. The checkout is
    /// left exactly as it was.
    #[arg(long, conflicts_with = "from")]
    pub carry: bool,

    /// A directory in the project. Defaults to the working directory.
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl New {
    /// Create the unit, report what it was given, and offer the home to the shell.
    ///
    /// The offer is the last thing and it changes no output: a shell that installed the
    /// function enters the new home, and a shell that did not reads the path in the
    /// report, as a script does.
    ///
    /// The first create of a workspace builds the base the home is cloned from, which
    /// takes as long as a clone and an install take. It says so line by line on
    /// standard error while it works, so that the wait is accounted for rather than
    /// silent.
    ///
    /// `--carry` adds one thing and changes nothing else: the unit starts at this
    /// checkout's `HEAD` with the work the person had not committed copied into it. The
    /// checkout is read and left as it was, and the unit starts dirty rather than with a
    /// commit Nodal wrote. It refuses rather than guess — an unmerged index, a `HEAD`
    /// that is not a branch with a commit, a set over the ceiling, or a file it would
    /// have to overwrite — and every refusal is made before a base can be built.
    ///
    /// # Errors
    ///
    /// Propagates a branch another open unit holds, a home that would overlap a tree
    /// Nodal knows, whatever `--carry` refused, and whatever Git, the filesystem or the
    /// registry reported.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let (text, home) = self.made(store, hooks, Format::from_json_flag(self.json))?;
        crate::commands::emit(&text)?;
        if let Some(home) = home {
            entry::ask_to_enter(&home)?;
        }
        Ok(ExitCode::SUCCESS)
    }

    /// Make the unit and render the report, without offering to enter the home.
    ///
    /// The offer is the one part of this command that speaks to a person at a terminal,
    /// so it stays in [`Self::run`]. Everything else — the operation, the memory, and
    /// the one rendering both surfaces print — is here, and the tool surface
    /// `nodal mcp` answers on asks for the JSON form of it.
    ///
    /// The home comes back with the text so that the caller which does offer to enter it
    /// has the path without reading the report again.
    ///
    /// # Errors
    ///
    /// Whatever the create reported.
    pub fn made(
        &self,
        store: &mut Store,
        hooks: bool,
        format: Format,
    ) -> nodal_core::Result<(String, Option<PathBuf>)> {
        let progress: Arc<dyn Reporter> = substrate::sink(self.json);
        let report = new::create(store, &self.request(hooks)?, &progress)?;
        let home = report.unit.environment.as_ref().map(|environment| environment.home.clone());
        if let Some(home) = &home {
            context::refresh_at(store, home);
        }
        Ok((output::render(&report, format)?, home))
    }

    /// The arguments as the values the operation takes.
    fn request(&self, hooks: bool) -> nodal_core::Result<Request> {
        Ok(Request {
            source: self.path.clone().unwrap_or_else(|| PathBuf::from(".")),
            objective: self.objective.as_deref().map(Objective::parse).transpose()?,
            objective_epistemic: Epistemic::Stated,
            name: self.name.as_deref().map(Slug::parse).transpose()?,
            parent_branch: self.from.as_deref().map(BranchName::parse).transpose()?,
            hooks,
            carry: self.carry,
        })
    }
}
