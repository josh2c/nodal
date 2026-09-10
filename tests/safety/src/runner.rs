//! The one place a test builds a command for the `nodal` binary.
//!
//! Nodal's state directory is `~/.nodal` unless `NODAL_HOME` says otherwise, and the
//! first command that opens the registry makes the directory and the file. A test that
//! built a command without naming a state directory therefore ran against the registry
//! of whoever was running it. Three files are shared by every unit on a machine — the
//! state directory, the per-machine secrets file and the hook approvals — and a test
//! must never read or create the ones belonging to that person.
//!
//! So [`nodal`] names the state directory, and [`isolated`] names all three.
//! `crates/nodal-cli/tests/state_directory.rs` is what keeps every command in the CLI
//! suite coming from here.
//!
//! There is a fourth, and it is a directory rather than a file. `nodal init
//! --claude-hooks` writes the settings Claude Code reads for every project, and
//! `CLAUDE_CONFIG_DIR` is what says where those live. Removing the variable is not
//! enough: with nothing set, the path falls back to `$HOME/.claude`, which is the real
//! one for any fixture that does not move `HOME`. So [`Runner`] **names** it, at a
//! directory of the test's own ([`Runner::config`]), and a fixture cannot forget to.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The name of the configuration directory the kit puts beside every state directory.
const CONFIG_DIR_NAME: &str = "claude-config";

/// A command for the binary, with `state` as its state directory.
///
/// Two variables are removed as well. `NODAL_CD_FILE` names a file a waiting shell
/// reads a path from, and a test run from inside an activated home would otherwise
/// inherit the one belonging to that shell. `CLAUDE_CONFIG_DIR` names the directory
/// Claude Code keeps the settings file in, and `nodal init --claude-hooks` writes that
/// file: a test run by a person who has moved that directory would otherwise install
/// hooks into it.
///
/// Removing it is the floor and not the ceiling. Nothing set means `$HOME/.claude`, so
/// a caller that wants the write to land somewhere of its own names the directory;
/// [`Runner`] does.
#[must_use]
pub fn nodal(binary: impl AsRef<Path>, state: impl AsRef<Path>) -> Command {
    let mut command = Command::new(binary.as_ref());
    command.env(nodal_core::workspace::home::DIRECTORY_VAR, state.as_ref());
    command.env_remove("NODAL_CD_FILE");
    command.env_remove(nodal_core::adapters::settings::CONFIG_DIR_VAR);
    command
}

/// The same, with the per-machine secrets file and the hook approvals moved as well.
///
/// This is what a test that makes units asks for. Both files sit in the state directory
/// by default (`env::secrets::path_in`, `lifecycle::hooks::path_in`), so naming the
/// state directory moves them; naming them outright says so where a reader can see it.
#[must_use]
pub fn isolated(binary: impl AsRef<Path>, state: impl AsRef<Path>) -> Command {
    let state = state.as_ref();
    let mut command = nodal(binary, state);
    command.env("NODAL_SECRETS_FILE", state.join("secrets.env"));
    command.env("NODAL_HOOKS_FILE", state.join("hooks.toml"));
    command
}

/// The binary, the state directory it writes into, and the project it runs in.
///
/// Both fixtures in this crate own one of these, and every command either of them makes
/// comes from here. What differs between the two is what they put in the environment
/// beyond the three files, so that is the one thing a caller adds.
pub struct Runner {
    /// The binary under test.
    binary: PathBuf,
    /// The state directory every command is given.
    state: PathBuf,
    /// Where the settings Claude Code reads for every project go, for this test.
    config: PathBuf,
    /// The directory a command runs in unless the caller names another.
    cwd: PathBuf,
    /// What this fixture puts in the environment beyond the three files.
    extra: Vec<(OsString, OsString)>,
}

impl Runner {
    /// A runner for `binary`, writing into `state`, running in `cwd`.
    ///
    /// The Claude Code configuration directory is named here rather than asked for,
    /// because a fixture that forgot would send a `--claude-hooks` install into the
    /// settings of whoever is running the tests. It goes **beside** the state directory
    /// and never inside it: `tests/doctor_writes_nothing.rs` reads the state root byte
    /// for byte, and a directory of the kit's own in there would be a write it reports.
    #[must_use]
    pub fn new(binary: impl AsRef<Path>, state: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Self {
        let state = state.as_ref().to_path_buf();
        let config = config_beside(&state);
        Self {
            binary: binary.as_ref().to_path_buf(),
            state,
            config,
            cwd: cwd.as_ref().to_path_buf(),
            extra: Vec::new(),
        }
    }

    /// Where this runner's commands keep the settings Claude Code reads.
    ///
    /// A test that installs the hooks reads the file back from here. It is not
    /// `$HOME/.claude`: the point of naming it is that no test depends on `HOME`.
    #[must_use]
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// The same, with one more variable in the environment of every command it makes.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.extra.push((name.into(), value.into()));
        self
    }

    /// The invocation itself, not yet run.
    ///
    /// The configuration directory is set after [`isolated`] has removed whatever the
    /// person running the tests had: the removal is the floor, and this is the value.
    #[must_use]
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = isolated(&self.binary, &self.state);
        command.env(nodal_core::adapters::settings::CONFIG_DIR_VAR, &self.config);
        command.args(args).current_dir(&self.cwd);
        for (name, value) in &self.extra {
            command.env(name, value);
        }
        command
    }

    /// The invocation, run in the project.
    ///
    /// # Panics
    ///
    /// If the binary could not be started. A command that ran and failed is an answer.
    #[must_use]
    pub fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the binary runs")
    }

    /// The invocation, to be run somewhere other than the project, not yet run.
    #[must_use]
    pub fn command_in(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut command = self.command(args);
        command.current_dir(cwd);
        command
    }

    /// The same invocation, run somewhere other than the project.
    ///
    /// # Panics
    ///
    /// As [`Runner::nodal`].
    #[must_use]
    pub fn nodal_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command_in(cwd, args).output().expect("the binary runs")
    }
}

/// The configuration directory that goes beside `state`, made ready to be written in.
///
/// Beside, not inside: the state root is a directory one suite reads byte for byte. A
/// state directory with no parent at all is a shape no fixture makes; it answers with a
/// directory beside nothing rather than panicking, because a test kit that panicked
/// while building a command would say nothing useful about why.
fn config_beside(state: &Path) -> PathBuf {
    let directory = state
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| state.with_extension("claude"), |parent| parent.join(CONFIG_DIR_NAME));
    drop(std::fs::create_dir_all(&directory));
    directory
}
