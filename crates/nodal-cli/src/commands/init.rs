//! `nodal init`: write the project's `nodal.toml`, with a line for every gap.
//!
//! It also offers the Claude Code integration, because this is the moment a person is
//! deciding what Nodal does for this project and the moment they are at a terminal. The
//! offer is one question, the answer is four hooks in `.claude/settings.json`, and
//! `nodal uninstall` takes them out again. Nothing is installed without being asked
//! unless a flag said so, and a run nothing is watching installs nothing and says why.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use nodal_core::adapters::{claude_code, settings};
use nodal_core::lifecycle::hooks;
use nodal_core::output::view::InitReport;
use nodal_core::output::{self, Format};
use nodal_core::recipe;
use nodal_core::workspace::home;

/// Arguments of `nodal init`.
#[derive(Debug, Args)]
pub struct Init {
    /// The project root. Defaults to the working directory.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Rewrite an existing `nodal.toml`, keeping the keys it already sets.
    #[arg(long)]
    pub force: bool,

    /// Print what would be written and write nothing.
    #[arg(long)]
    pub print: bool,

    /// Print the plan as JSON.
    #[arg(long)]
    pub json: bool,

    /// What to do about the Claude Code integration.
    #[command(flatten)]
    pub claude: Claude,
}

/// The two flags about the Claude Code hooks, in their own group because they are the
/// only pair that writes anywhere but `nodal.toml`.
#[derive(Debug, Args)]
pub struct Claude {
    /// Install the Claude Code hooks without asking. What a script uses.
    #[arg(long = "claude-hooks")]
    pub install: bool,

    /// Do not ask about the Claude Code hooks, and install none.
    #[arg(long = "no-claude-hooks", conflicts_with = "install")]
    pub none: bool,
}

/// What a person types to agree.
const AGREED: [&str; 2] = ["y", "yes"];

impl Init {
    /// Infer the recipe, then print it or write it.
    ///
    /// # Errors
    ///
    /// Propagates a recipe that cannot be read, and a file that cannot be written.
    pub fn run(&self) -> nodal_core::Result<ExitCode> {
        let root = self.path.clone().unwrap_or_else(|| PathBuf::from("."));
        let plan = recipe::plan_init(&root)?;
        let report = InitReport::from_plan(&plan);
        if self.json {
            return write(&report, Format::Json);
        }
        if self.print {
            print!("{}", plan.contents);
            return Ok(ExitCode::SUCCESS);
        }
        recipe::apply_init(&plan, self.force)?;
        approve(&plan)?;
        self.offer(root_of(&plan))?;
        write(&report, Format::Human)
    }
}

/// Approve, on this machine, the hooks the written recipe declares.
///
/// This is the moment the approval is asked for: a person has just read the recipe they
/// are writing. What is approved is the exact text of each command, so one that changes
/// afterwards is refused until `nodal init` is run again
/// (`nodal_core::lifecycle::hooks`).
///
/// The line goes to standard error, because the command's answer on standard output is
/// one document.
fn approve(plan: &recipe::InitPlan) -> nodal_core::Result<()> {
    let count = hooks::approve(&home::directory()?, root_of(plan), &plan.hooks)?;
    if count > 0 {
        eprintln!("nodal: approved {count} hook command(s) declared by this project");
    }
    Ok(())
}

impl Init {
    /// Offer the Claude Code hooks, and install them when the answer is yes.
    ///
    /// The question is asked once, of a terminal. A run nothing is watching is not
    /// waited on: it is told that `--claude-hooks` installs them, which is the same rule
    /// `nodal uninstall` follows.
    ///
    /// Everything goes to standard error, because the command's answer on standard
    /// output is one document.
    fn offer(&self, root: &Path) -> nodal_core::Result<()> {
        if self.claude.none || !self.wanted(root)? {
            return Ok(());
        }
        let Some(done) = claude_code::install(root)? else { return Ok(()) };
        eprintln!(
            "nodal: wrote {} Claude Code hooks into {}; commit that file or do not, as you like",
            done.events.len(),
            done.path.display()
        );
        Ok(())
    }

    /// Whether the hooks are wanted: because a flag said so, or because a person did.
    ///
    /// A project that already has them is not asked about again, so a second
    /// `nodal init` is one question fewer rather than the same question twice.
    fn wanted(&self, root: &Path) -> nodal_core::Result<bool> {
        if self.claude.install {
            return Ok(true);
        }
        let file = settings::path(root);
        if settings::holds_hooks(&claude_code::read(&file)?) {
            return Ok(false);
        }
        if !std::io::stdin().is_terminal() {
            eprintln!(
                "nodal: pass --claude-hooks to install the Claude Code hooks for this project"
            );
            return Ok(false);
        }
        ask(&file)
    }
}

/// Ask the one question, and read the one answer.
fn ask(file: &Path) -> nodal_core::Result<bool> {
    eprintln!("nodal: Claude Code can make a unit for every session it starts on this project.");
    eprintln!("nodal: that writes four hooks into {}.", file.display());
    eprint!("install them? [y/N] ");
    std::io::stderr().flush().map_err(nodal_core::Error::io("<stderr>"))?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).map_err(nodal_core::Error::io("<stdin>"))?;
    Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
}

/// The project root a plan writes into.
fn root_of(plan: &recipe::InitPlan) -> &Path {
    plan.path.parent().unwrap_or_else(|| Path::new("."))
}

/// Print a read type in the format asked for. Every command ends this way, so the two
/// renderings stay two views of one value (`nodal_core::output`).
fn write(report: &InitReport, format: Format) -> nodal_core::Result<ExitCode> {
    output::write(report, format, &mut std::io::stdout())?;
    Ok(ExitCode::SUCCESS)
}
