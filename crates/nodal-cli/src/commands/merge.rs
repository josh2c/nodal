//! `nodal merge`: commit, squash, rebase, fast-forward and remove, in one command.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use nodal_core::lifecycle::ops::merge::{self, Request, STAGES, Stages};
use nodal_core::output::{self, Format};
use nodal_core::store::Store;

use crate::commands::context;

/// What a person types to agree to the plan.
const AGREED: [&str; 2] = ["y", "yes"];

/// The flags that drop one of the three stages which rewrite the unit's branch.
///
/// Their own group because they share a property the fourth stage does not: each of
/// them leaves the branch closer to the way the merge found it.
#[derive(Debug, Args)]
pub struct Rewriting {
    /// Leave what the home holds uncommitted.
    #[arg(long)]
    pub no_commit: bool,

    /// Merge the branch's commits as they are, without folding them into one.
    #[arg(long)]
    pub no_squash: bool,

    /// Do not rebase the branch onto the target first.
    #[arg(long)]
    pub no_rebase: bool,
}

/// What becomes of the unit once the target carries its work.
#[derive(Debug, Args)]
pub struct Removal {
    /// Leave the unit where it is instead of reclaiming it.
    #[arg(long)]
    pub no_remove: bool,
}

/// Arguments of `nodal merge`.
#[derive(Debug, Args)]
pub struct Merge {
    /// The unit's handle. Defaults to the unit the working directory is in.
    #[arg(value_name = "UNIT")]
    pub unit: Option<String>,

    /// The commit message. Defaults to the unit's objective, and then to its handle.
    #[arg(short, long, value_name = "MESSAGE")]
    pub message: Option<String>,

    /// Run the plan without being asked. What a script uses.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Which of the three rewriting stages to drop.
    #[command(flatten)]
    pub rewriting: Rewriting,

    /// Whether the unit is removed afterwards.
    #[command(flatten)]
    pub removal: Removal,

    /// Stop a rebase this unit is in and put its branch back where the merge found it.
    #[arg(long, conflicts_with_all = ["message", "no_commit", "no_squash", "no_rebase"])]
    pub abort: bool,

    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

impl Merge {
    /// Show the plan, run it once it is agreed to, and print what it did.
    ///
    /// The plan goes to standard error and the answer to standard output, so what a
    /// tool reads is one document whichever of the two formats it asked for.
    ///
    /// The exit code says whether everything the merge was asked to do happened. A
    /// rebase that stopped for a conflict and a removal the reclaim refused are both
    /// reported rather than raised, and both exit non-zero, because the person or the
    /// script that asked has something left to do.
    ///
    /// # Errors
    ///
    /// Propagates a target that has moved, a home that is not on the unit's branch, a
    /// hook this machine has not approved, and whatever Git, the filesystem or the
    /// registry reported.
    pub fn run(&self, store: &mut Store, hooks: bool) -> nodal_core::Result<ExitCode> {
        let request = self.request(hooks);
        let format = Format::from_json_flag(self.json);
        if self.abort {
            let stopped = merge::abort(store, &request)?;
            output::write(&stopped, format, &mut std::io::stdout())?;
            return Ok(ExitCode::SUCCESS);
        }
        output::write(&merge::preview(store, &request)?, format, &mut std::io::stderr())?;
        if !self.agreed()? {
            eprintln!("nodal: nothing was done");
            return Ok(ExitCode::FAILURE);
        }
        let project = context::project_of(store, self.unit.as_deref(), &request.cwd);
        let report = merge::merge(store, &request)?;
        if let Some(project) = &project {
            context::refresh(store, project);
        }
        output::write(&report, format, &mut std::io::stdout())?;
        Ok(if report.is_complete() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
    }

    /// What was asked for, with one stage dropped per `--no-` flag.
    fn request(&self, hooks: bool) -> Request {
        let dropped = [
            self.rewriting.no_commit,
            self.rewriting.no_squash,
            self.rewriting.no_rebase,
            self.removal.no_remove,
        ];
        let mut stages = Stages::all();
        for (stage, drop) in STAGES.iter().zip(dropped) {
            if drop {
                stages = stages.without(*stage);
            }
        }
        Request {
            target: self.unit.clone(),
            message: self.message.clone(),
            stages,
            hooks,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    /// Whether the person agreed to the plan.
    ///
    /// One question and no other. A terminal is asked; anything else is not, because a
    /// script that is not watched must never be waited on, and it says so rather than
    /// running a plan nobody saw.
    fn agreed(&self) -> nodal_core::Result<bool> {
        if self.yes {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            return Err(nodal_core::Error::InvalidValue {
                kind: "agreement",
                value: String::from(
                    "nothing is watching this terminal; pass --yes to run the plan",
                ),
            });
        }
        eprint!("run this plan? [y/N] ");
        std::io::stderr().flush().map_err(nodal_core::Error::io("<stderr>"))?;
        let mut answer = String::new();
        std::io::stdin().lock().read_line(&mut answer).map_err(nodal_core::Error::io("<stdin>"))?;
        Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
    }
}
