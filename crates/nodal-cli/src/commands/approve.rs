//! `nodal approve`: accept, on this machine, the hook commands this project declares.
//!
//! Approval is its own verb because it is its own decision. Until it was one, the only
//! command that wrote the approval record was `nodal init`, and `nodal init` writes
//! `nodal.toml`. A project that already had a recipe therefore had one route to
//! approving a hook: `nodal init --force`, which rewrites the recipe from the merged
//! model and replaces every comment in it with the template's own. A person who wanted
//! to say "yes, run this line" lost the notes they had written around it.
//!
//! So this verb writes one file and it is not that one. It reads the project's recipe,
//! prints every command it is about to accept, and records the digest of each. It never
//! writes `nodal.toml`, installs nothing, and asks nothing else of the machine.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::hooks;
use nodal_core::output::view::Approval;
use nodal_core::output::{self, Format};
use nodal_core::recipe;
use nodal_core::workspace::home;

/// Arguments of `nodal approve`.
#[derive(Debug, Args)]
pub struct Approve {
    /// The project root. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the commands and approve nothing.
    #[arg(long)]
    pub print: bool,

    /// Print the answer as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Approve {
    /// Record the digest of every hook command the project declares.
    ///
    /// The hooks come from the effective recipe, so a command a person has just edited
    /// into `nodal.toml` is the command that is approved. The set is re-made from
    /// scratch, so a hook the project has removed stops being approved
    /// (`nodal_core::lifecycle::hooks`).
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Recipe`] when `nodal.toml` is not a recipe,
    /// [`nodal_core::Error::NoHomeDirectory`] when nothing says where the person's own
    /// directory is, and [`nodal_core::Error::Io`] when the record could not be written.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        // Resolved, because the record is keyed by the resolved path
        // (`nodal_core::lifecycle::hooks`) and a report that named `.` would not say
        // which project the person just accepted a command for.
        let root =
            nodal_core::paths::resolve(&self.path.clone().unwrap_or_else(|| PathBuf::from(".")));
        let hooks = recipe::load(&root)?.recipe.hooks;
        let state = home::directory()?;
        let record = hooks::path_in(&state);
        if !self.print {
            hooks::approve(&state, &root, &hooks)?;
        }
        let report = Approval::of(root, record, &hooks);
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }
}
