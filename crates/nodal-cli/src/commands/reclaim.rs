//! `nodal reclaim`: end a unit, move its home to the trash, and say what is left.
//!
//! `--check` is the same command asked what it would do. It performs no part of a
//! reclaim: no hook, no signal, no container action, no port release, no snapshot, no
//! trash move, no registry write and no remote. It prints the reading and stops, and its
//! exit code is the verdict, so a script can gate on it without parsing anything.
//!
//! It refuses `--force` and `--yes` for the reason it exists. Both of those are answers
//! to a question a person has already asked, and a command that both describes what
//! would happen and overrides the objection to it is two commands.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::reclaim::{self, Request};
use nodal_core::output::view::Reclaimed;
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

use crate::commands::context;

/// What a person types to agree to `git worktree remove`.
const AGREED: [&str; 2] = ["y", "yes"];

/// Arguments of `nodal reclaim`.
#[derive(Debug, Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a flag is a bool, and this is the list of the command's flags"
)]
pub struct Reclaim {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// Report what a reclaim would do and do none of it: what is only here, what has
    /// another copy on this machine, what a reading of the remote proves, what a tool
    /// writes again, what would be stopped, and whether it would go ahead. It changes
    /// nothing. The exit code is the verdict.
    #[arg(long, conflicts_with_all = ["force", "yes"])]
    pub check: bool,

    /// Reclaim a unit whose home holds work that exists nowhere else, or whose home
    /// something Nodal did not start is standing in. The work is committed to a
    /// snapshot ref inside the home first, and the home goes to the trash rather than
    /// being deleted, so nothing here is a way to lose a commit.
    #[arg(long)]
    pub force: bool,

    /// Run `git worktree remove` for a done adopted worktree without being asked.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Reclaim {
    /// Reclaim the unit and print what was done, including what was left.
    ///
    /// The exit code says whether the verification found anything. A reclaim that left
    /// something behind is not a failure — it did what it could and reported the rest —
    /// but a script that reclaims a hundred units needs to know which of them to look
    /// at, and the report on standard output is where it reads what.
    ///
    /// # Errors
    ///
    /// Propagates a home that holds work that is only there, a home something Nodal
    /// did not start is standing in, a unit that was reclaimed already, a hook this
    /// machine has not approved, and whatever Git, the filesystem or the registry
    /// reported.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let request = Request {
            target: self.unit.clone(),
            force: self.force,
            hooks,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        if self.check {
            return self.preflight(store, &request);
        }
        let project = context::project_of(store, self.unit.as_deref(), &request.cwd);
        let report = reclaim::reclaim(store, &request)?;
        if let Some(project) = &project {
            context::refresh(store, project);
        }
        let left = !report.leftovers.is_empty();
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        self.remove_if_agreed(&report)?;
        Ok(if left { ExitCode::FAILURE } else { ExitCode::SUCCESS })
    }

    /// Print what a reclaim would do, and do none of it.
    ///
    /// Nothing after the reading. The memories are not recompiled and the refs are not
    /// refreshed, although both are ordinary things for a command to do afterwards,
    /// because both write and this command's whole promise is that it does not.
    ///
    /// The exit code is the verdict. Success means a reclaim would go ahead; failure
    /// means it would refuse, and the report says why. A script that reclaims a hundred
    /// units reads it to decide which of them to look at first, and reads standard
    /// output to find out what about.
    ///
    /// # Errors
    ///
    /// Propagates a unit that was reclaimed already, a directory that belongs to
    /// another unit, and whatever Git or the registry reported.
    fn preflight(&self, store: &Store, request: &Request) -> nodal_core::Result<ExitCode> {
        let report = reclaim::check(store, request)?;
        let safe = report.safe_to_reclaim;
        output::write(&report, Format::from_json_flag(self.json), &mut std::io::stdout())?;
        Ok(if safe { ExitCode::SUCCESS } else { ExitCode::FAILURE })
    }

    /// Run `git worktree remove` when the report offered it and the person agreed.
    ///
    /// Default is no. A terminal that is not watched is not waited on: the command is
    /// already in the report, and they can run it themselves.
    fn remove_if_agreed(&self, report: &Reclaimed) -> nodal_core::Result<()> {
        let Some(path) = &report.worktree_remove else {
            return Ok(());
        };
        if !self.agreed()? {
            return Ok(());
        }
        reclaim::remove_worktree(path)
    }

    /// Whether the person agreed to remove the worktree.
    fn agreed(&self) -> nodal_core::Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            return Ok(false);
        }
        eprint!("remove this worktree? [y/N] ");
        std::io::stderr().flush().map_err(nodal_core::Error::io("<stderr>"))?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).map_err(nodal_core::Error::io("<stdin>"))?;
        Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
    }
}
