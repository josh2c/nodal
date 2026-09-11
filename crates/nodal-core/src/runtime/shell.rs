//! `nodal shell`: the entry a script and a second machine use.
//!
//! The shell function from `nodal shell-init` is how a person enters a unit, because it
//! moves the shell they are already in. This command is the other case: a script, a
//! remote host, or a terminal with no integration installed. It replaces the running
//! process with the shell, so what a person ends up in is their shell and not a shell
//! inside a shell. There is nothing to exit twice and nothing to ask on the way out.
//!
//! Replacing the process is also why this module records nothing. A session is derived
//! from the process table ([`crate::runtime::sessions`]), and the shell that takes this
//! process over carries the home's `NODAL_ID`, so the next scan sees it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The variable naming the shell to start.
pub const SHELL_VAR: &str = "SHELL";

/// The shell to start when the environment names none.
pub const FALLBACK: &str = "/bin/sh";

/// What starting a shell in a home needs: the program, and the environment it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The shell to start.
    pub program: PathBuf,
    /// The variables to add to the environment it inherits.
    pub vars: BTreeMap<String, String>,
    /// The directory it starts in.
    pub home: PathBuf,
}

/// What `nodal shell` will do in `home`, without doing it.
///
/// # Errors
/// [`Error::Io`] when the home's `.nodal/env` cannot be read.
pub fn plan(home: &Path) -> Result<Entry> {
    let program = std::env::var_os(SHELL_VAR)
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(FALLBACK), PathBuf::from);
    let vars = crate::env::entering(home)?
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    Ok(Entry { program, vars, home: home.to_path_buf() })
}

/// Become the shell.
///
/// On success this function does not return: the process is the shell from then on.
///
/// # Errors
/// [`Error::Io`] when the shell cannot be started.
#[cfg(unix)]
pub fn enter(entry: &Entry) -> Result<std::convert::Infallible> {
    use std::os::unix::process::CommandExt as _;

    let error = std::process::Command::new(&entry.program)
        .envs(&entry.vars)
        .current_dir(&entry.home)
        .exec();
    Err(Error::io(&entry.program)(error))
}

/// Windows has no `exec`, so the shell is a child and this process waits for it.
///
/// # Errors
/// [`Error::Io`] when the shell cannot be started.
#[cfg(not(unix))]
pub fn enter(entry: &Entry) -> Result<std::convert::Infallible> {
    let status = std::process::Command::new(&entry.program)
        .envs(&entry.vars)
        .current_dir(&entry.home)
        .status()
        .map_err(Error::io(&entry.program))?;
    std::process::exit(status.code().unwrap_or(1));
}
