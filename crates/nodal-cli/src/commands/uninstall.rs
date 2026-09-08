//! `nodal uninstall`: take back what Nodal put on this machine.

use std::io::{BufRead, IsTerminal, Write};
use std::process::ExitCode;

use clap::Args;
use nodal_core::output::{self, Format};
use nodal_core::setup::plan::{self, Request};
use nodal_core::workspace::home;

/// What a person types to agree.
const AGREED: [&str; 2] = ["y", "yes"];

/// The two flags about Nodal's state directory.
///
/// Their own group because they are the only pair that can lose work: the block in a
/// start-up file and the script in the state directory are text Nodal wrote, and
/// nothing of a person's is in either.
#[derive(Debug, Args)]
pub struct State {
    /// Remove Nodal's state directory as well: the registry and every unit home.
    #[arg(long)]
    pub state: bool,

    /// Remove the state directory even when a home holds work nothing else has.
    #[arg(long, requires = "state")]
    pub force: bool,
}

/// Arguments of `nodal uninstall`.
#[derive(Debug, Args)]
pub struct Uninstall {
    /// Whether the state directory goes, and what may be lost with it.
    #[command(flatten)]
    pub removal: State,

    /// Remove what the summary lists without being asked. What a script uses.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Print the summary and remove nothing.
    #[arg(long)]
    pub dry_run: bool,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Uninstall {
    /// Print what would go, ask, and then take it away.
    ///
    /// The summary goes to standard error and the answer to standard output, as
    /// `nodal merge` does, so what a tool reads is one document.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::NoHomeDirectory`] when nothing says where the state
    /// directory or the start-up files are, [`nodal_core::Error::InvalidValue`] when a
    /// home holds work nothing else has and `--force` was not given, and whatever
    /// reading or writing a file reported.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let request = self.request()?;
        let format = Format::from_json_flag(self.json);
        let plan = plan::survey(&request)?;
        if plan.is_empty() {
            output::write(&plan, format, &mut std::io::stdout())?;
            return Ok(ExitCode::SUCCESS);
        }
        output::write(&plan, format, &mut std::io::stderr())?;
        if self.dry_run {
            eprintln!("nodal: --dry-run, so nothing was done");
            return Ok(ExitCode::SUCCESS);
        }
        if !plan.is_permitted() {
            return Err(nodal_core::Error::InvalidValue {
                kind: "uninstall",
                value: String::from("a home holds work that exists nowhere else"),
            });
        }
        if !self.agreed()? {
            eprintln!("nodal: nothing was done");
            return Ok(ExitCode::FAILURE);
        }
        output::write(&plan::apply(&plan)?, format, &mut std::io::stdout())?;
        Ok(ExitCode::SUCCESS)
    }

    /// What was asked for, against this machine's directories.
    fn request(&self) -> nodal_core::Result<Request> {
        Ok(Request {
            state: home::directory()?,
            home: home::user()?,
            state_too: self.removal.state,
            force: self.removal.force,
        })
    }

    /// Whether the person agreed to the summary.
    ///
    /// One question and no other, and the same rule `nodal merge` uses: a terminal is
    /// asked, and anything else is told to pass `--yes` rather than waited on.
    fn agreed(&self) -> nodal_core::Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            return Err(nodal_core::Error::InvalidValue {
                kind: "agreement",
                value: String::from("nothing is watching this terminal; pass --yes to remove it"),
            });
        }
        eprint!("remove all of this? [y/N] ");
        std::io::stderr().flush().map_err(nodal_core::Error::io("<stderr>"))?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).map_err(nodal_core::Error::io("<stdin>"))?;
        Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
    }
}
