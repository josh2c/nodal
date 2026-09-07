//! The one place in `nodal-core` that spawns `git`.
//!
//! Every other function in this module builds an argument list and hands it here, so
//! mocking or swapping the backing implementation is a single seam
//! (`docs/code-structure.md`). Nothing here interprets output beyond decoding it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// The environment every invocation runs with: never prompt, never take optional locks,
/// never page. Values are constant so behaviour does not depend on the caller's shell.
const ENV: &[(&str, &str)] = &[("GIT_TERMINAL_PROMPT", "0"), ("GIT_OPTIONAL_LOCKS", "0")];

/// What one `git` invocation produced.
#[derive(Debug, Clone)]
pub struct Output {
    /// The arguments the invocation was given, without the leading `git`.
    pub args: Vec<String>,
    /// Raw standard output; Git emits paths as bytes, so decoding is the caller's step.
    pub stdout: Vec<u8>,
    /// Standard error, lossily decoded, trailing whitespace removed.
    pub stderr: String,
    /// Exit code, or `None` when a signal ended the process.
    pub code: Option<i32>,
}

impl Output {
    /// Whether the invocation exited zero.
    #[must_use]
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// Standard output decoded as UTF-8, with the trailing newline removed.
    ///
    /// # Errors
    /// [`Error::GitEncoding`] when the output is not valid UTF-8.
    pub fn text(&self) -> Result<&str> {
        let text = std::str::from_utf8(&self.stdout)
            .map_err(|_| Error::GitEncoding { args: self.args.clone() })?;
        Ok(text.strip_suffix('\n').unwrap_or(text))
    }

    /// Standard output split on NUL, empty records dropped.
    ///
    /// # Errors
    /// [`Error::GitEncoding`] when the output is not valid UTF-8.
    pub fn records(&self) -> Result<Vec<&str>> {
        Ok(self.text()?.split('\0').filter(|record| !record.is_empty()).collect())
    }

    /// Standard output split into non-empty lines.
    ///
    /// # Errors
    /// [`Error::GitEncoding`] when the output is not valid UTF-8.
    pub fn lines(&self) -> Result<Vec<&str>> {
        Ok(self.text()?.lines().filter(|line| !line.is_empty()).collect())
    }
}

/// Run `git` in `repo` and return its output whatever the exit code.
///
/// Use this only where a non-zero exit is an answer rather than a failure (a missing
/// ref, an unknown branch); everything else wants [`run_ok`].
///
/// # Errors
/// [`Error::GitSpawn`] when the `git` binary could not be started.
pub fn run(repo: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo).arg("--no-pager").args(args);
    for (key, value) in ENV {
        command.env(key, value);
    }
    let output = command.output().map_err(|source| Error::GitSpawn { source })?;
    Ok(Output {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_owned(),
        code: output.status.code(),
    })
}

/// Run `git` in `repo` and fail on a non-zero exit.
///
/// # Errors
/// [`Error::GitSpawn`] when `git` could not be started, [`Error::Git`] when it exited
/// non-zero.
pub fn run_ok(repo: &Path, args: &[&str]) -> Result<Output> {
    let output = run(repo, args)?;
    if output.ok() {
        return Ok(output);
    }
    Err(Error::Git {
        repo: PathBuf::from(repo),
        args: output.args,
        code: output.code,
        stderr: output.stderr,
    })
}
