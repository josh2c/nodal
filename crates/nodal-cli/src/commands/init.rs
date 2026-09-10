//! `nodal init`: write the project's `nodal.toml`, with a line for every gap.
//!
//! It installs the Claude Code hooks when `--claude-hooks` asks for it, and it asks
//! nothing. A person who runs `nodal init` is writing a recipe, and a question about a
//! second tool in the middle of that is a question they did not come for.
//!
//! The four hooks go in the person's own `~/.claude/settings.json` by default, so they
//! apply in every project on the machine and are committed nowhere.
//! `--claude-hooks=project` writes the project's file instead, and says what that costs:
//! a clone of that file on a machine with no `nodal` answers `WorktreeCreate` with a
//! refusal, and Claude Code ends the session over it. `nodal uninstall` takes either
//! back.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use nodal_core::adapters::settings::Scope;
use nodal_core::adapters::{claude_code, settings};
use nodal_core::lifecycle::hooks;
use nodal_core::output::view::InitReport;
use nodal_core::output::{self, Format};
use nodal_core::recipe;
use nodal_core::workspace::home;
use nodal_core::workspace::sharing::Sharing;

/// Arguments of `nodal init`.
#[derive(Debug, Args)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a flag is a bool, and this is the list of the command's flags"
)]
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

    /// Ask the state root again whether nodal can share file blocks there, and record
    /// the answer. What a person runs after they move the state root, or after they fix
    /// what stopped the question being put.
    #[arg(long)]
    pub reprobe: bool,

    /// What to do about the Claude Code integration.
    #[command(flatten)]
    pub claude: Claude,
}

/// The two flags about the Claude Code hooks, in their own group because they are the
/// only pair that writes anywhere but `nodal.toml`.
#[derive(Debug, Args)]
pub struct Claude {
    /// Install the Claude Code hooks. `user` writes your own settings file, `project`
    /// writes this project's.
    #[arg(long = "claude-hooks", value_name = "SCOPE", num_args = 0..=1,
          default_missing_value = "user", value_enum)]
    pub install: Option<Where>,

    /// Accepted and does nothing. `nodal init` installs no hook unless asked to.
    #[arg(long = "no-claude-hooks", conflicts_with = "install", hide = true)]
    pub none: bool,
}

/// Which settings file `--claude-hooks` writes.
///
/// The library states the same two ([`Scope`]). This is the reading of them clap does,
/// so that `nodal-core` declares no command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Where {
    /// The person's own settings file, which applies in every project on this machine.
    User,
    /// This project's settings file, which a person may commit.
    Project,
}

impl From<Where> for Scope {
    fn from(chosen: Where) -> Self {
        match chosen {
            Where::User => Self::User,
            Where::Project => Self::Project,
        }
    }
}

/// What a person is told before the hooks go into a file they may commit.
///
/// The provider hook is the reason. A clone of the project on a machine with no `nodal`
/// runs it, and it answers `WorktreeCreate` with a refusal, which ends the session.
const PROJECT_COST: &str = "these hooks end a claude code session on a machine that has no nodal \
                            on its PATH; --claude-hooks alone writes your own settings instead";

impl Init {
    /// Say that this machine makes a full copy for every home, where it does.
    ///
    /// A home is cheap only where the filesystem can give two files the same blocks. On
    /// a state root that cannot, every home costs its own disk, and the person who set
    /// that up learns it at the first `nodal new` and not before. This is the moment to
    /// say so: the state root is chosen and the project is being set up, so the fix is
    /// one mount away rather than a migration.
    ///
    /// The question is put once, when the state root is made, and the answer is kept
    /// beside the registry. `init` reads that record, and takes one where there is
    /// none. `--reprobe` puts the question again whatever is recorded.
    ///
    /// The line goes to standard error, for the same reason the approval line does. A
    /// state root that shares blocks prints nothing; there is nothing to do about good
    /// news.
    fn say_what_the_state_root_can_do(&self) -> nodal_core::Result<()> {
        let root = home::directory()?;
        let record = if self.reprobe { Sharing::reprobe(&root) } else { Sharing::ensure(&root) };
        if let Some(line) = record.advice() {
            eprintln!("nodal: {line}");
        }
        Ok(())
    }

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
        self.say_what_the_state_root_can_do()?;
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
    /// Install the Claude Code hooks, in the file the flag named.
    ///
    /// No flag installs nothing and says nothing. There is no question here: see the
    /// module note.
    ///
    /// Everything goes to standard error, because the command's answer on standard
    /// output is one document.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::NoHomeDirectory`] when nothing says where the person's own
    /// directory is, and whatever reading or writing the settings file reported.
    fn offer(&self, root: &Path) -> nodal_core::Result<()> {
        let Some(chosen) = self.claude.install else { return Ok(()) };
        let scope = Scope::from(chosen);
        let file = match scope {
            Scope::User => settings::user_path(&home::user()?),
            Scope::Project => {
                eprintln!("nodal: {PROJECT_COST}");
                settings::path(root)
            }
        };
        let Some(done) = claude_code::install(&file, scope)? else {
            eprintln!("nodal: {} already declares the hooks", file.display());
            return Ok(());
        };
        eprintln!(
            "nodal: wrote {} Claude Code hooks into {} ({} scope)",
            done.events.len(),
            done.path.display(),
            done.scope.name()
        );
        Ok(())
    }
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
