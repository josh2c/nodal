//! `nodal approve`: accept, on this machine, the hook commands this project declares.
//!
//! Approval is its own verb because it is its own decision. Until it was one, the only
//! command that wrote the approval record was `nodal init`, and `nodal init` writes
//! `nodal.toml`. A project that already had a recipe therefore had one route to
//! approving a hook: `nodal init --force`, which rewrites the recipe from the merged
//! model and replaces every comment in it with the template's own. A person who wanted
//! to say "yes, run this line" lost the notes they had written around it.
//!
//! So this verb writes one file and it is not that one. It never writes `nodal.toml`,
//! installs nothing, and asks nothing else of the machine.
//!
//! # A person reads the command before the record accepts it
//!
//! A hook command arrives with a `git pull`. Writing the record first and printing the
//! commands after would accept a line nobody had read, which is the whole of what the
//! approval is there to stop. So the commands go to standard error first, then the
//! question, and only an answer writes the record.
//!
//! `--yes` is that answer given in advance, for a person who has read the recipe in
//! their editor and for a script. A terminal is asked; anything else is refused and told
//! to pass `--yes`, because a pipe cannot read the commands and cannot answer.
//! `--print` shows the commands and records nothing at all.

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::hooks;
use nodal_core::output::view::Approval;
use nodal_core::output::{self, Format};
use nodal_core::recipe;
use nodal_core::workspace::home;

/// The answers that accept the commands, as `nodal reclaim` reads them.
const AGREED: [&str; 2] = ["y", "yes"];

/// Arguments of `nodal approve`.
#[derive(Debug, Args)]
pub struct Approve {
    /// The project root. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the commands and approve nothing.
    #[arg(long, conflicts_with = "yes")]
    pub print: bool,

    /// Accept every command the project declares without being asked.
    #[arg(long)]
    pub yes: bool,

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
    /// directory is, and [`nodal_core::Error::Io`] when the record could not be written
    /// or the question could not be put.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        // Resolved, because the record is keyed by the resolved path
        // (`nodal_core::lifecycle::hooks`) and a report that named `.` would not say
        // which project the person just accepted a command for.
        let root =
            nodal_core::paths::resolve(self.path.as_deref().unwrap_or_else(|| Path::new(".")));
        let hooks = recipe::load(&root)?.recipe.hooks;
        let state = home::directory()?;
        let report = Approval::of(root.clone(), hooks::path_in(&state), &hooks);

        if !self.print && !report.commands.is_empty() {
            Self::read_them(&report);
            if !self.agreed()? {
                eprintln!("nodal: nothing was approved");
                return Ok(ExitCode::FAILURE);
            }
            hooks::approve(&state, &root, &hooks)?;
        }
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// Put every command in front of the person, before anything accepts it.
    ///
    /// Standard error, because the command's answer on standard output is one document
    /// and this is the question rather than the answer.
    fn read_them(report: &Approval) {
        eprintln!("nodal: {} declares these hook commands:", report.project.display());
        for approved in &report.commands {
            eprintln!("  {phase}  {command}", phase = approved.phase, command = approved.command);
        }
        eprintln!("nodal: they run on your account, in this project, from now on");
    }

    /// Whether the person accepted the commands.
    ///
    /// A host with no terminal is refused rather than asked, because it cannot answer;
    /// the message names the flag that answers in advance.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Io`] when the question or the answer could not be read.
    fn agreed(&self) -> nodal_core::Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            eprintln!("nodal: nothing here can answer; read the commands and pass --yes");
            return Ok(false);
        }
        eprint!("approve them? [y/N] ");
        std::io::stderr().flush().map_err(nodal_core::Error::io("<stderr>"))?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).map_err(nodal_core::Error::io("<stdin>"))?;
        Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
    }
}
