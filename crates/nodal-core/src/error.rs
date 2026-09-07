//! The one error type for `nodal-core`.
//!
//! Variants are added per module as modules arrive; a variant carries the values a
//! caller needs to act on, never a pre-formatted string.

use std::path::PathBuf;

use thiserror::Error as ThisError;

use crate::git::preflight;

/// Every failure `nodal-core` can return.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// A path could not be read, written or inspected.
    #[error("{path}: {source}")]
    Io {
        /// The path the operation was attempted on.
        path: PathBuf,
        /// The underlying operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A domain value did not have the shape its type requires. `kind` names the type
    /// in the words a user sees, so the message is the same wherever it is raised.
    #[error("{value:?} is not a valid {kind}")]
    InvalidValue {
        /// What the value was meant to be, for example `branch name`.
        kind: &'static str,
        /// The value as it was given.
        value: String,
    },

    /// A tracing subscriber was already installed in this process.
    #[error("logging is already initialised for this process")]
    LoggingAlreadyInitialised,

    /// The log filter given in the environment could not be parsed.
    #[error("invalid log filter {filter:?}: {source}")]
    LogFilter {
        /// The filter string as it was read from the environment.
        filter: String,
        /// Why the filter was rejected.
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },

    /// The `git` binary could not be started.
    #[error("could not run git: {source}")]
    GitSpawn {
        /// Why the process could not be spawned.
        #[source]
        source: std::io::Error,
    },

    /// A `git` invocation exited non-zero.
    #[error("git {args} in {repo}: {stderr}", args = args.join(" "), repo = repo.display())]
    Git {
        /// The repository the command ran in.
        repo: PathBuf,
        /// The arguments given to `git`, without the leading `git`.
        args: Vec<String>,
        /// The exit code, or `None` when a signal ended the process.
        code: Option<i32>,
        /// What `git` wrote to standard error.
        stderr: String,
    },

    /// `git` produced output that is not valid UTF-8.
    #[error("git {args} produced output that is not UTF-8", args = args.join(" "))]
    GitEncoding {
        /// The arguments of the invocation whose output could not be decoded.
        args: Vec<String>,
    },

    /// A record of `git` output did not have its documented shape.
    #[error("git {args} produced an unreadable record {record:?}", args = args.join(" "))]
    GitParse {
        /// The arguments of the invocation whose output could not be parsed.
        args: Vec<String>,
        /// The record as it was read.
        record: String,
    },

    /// Text that should have been an object id was not one.
    #[error("{text:?} is not a Git object id")]
    GitOid {
        /// The text that was rejected.
        text: String,
    },

    /// A directory is not inside a Git repository.
    #[error("{path} is not a Git repository")]
    NotARepository {
        /// The directory that was opened.
        path: PathBuf,
    },

    /// A Git operation is in progress, so the repository must not be cloned or adopted.
    #[error("{repo} has Git operations in progress: {states:?}", repo = repo.display())]
    GitInProgress {
        /// The repository that was inspected.
        repo: PathBuf,
        /// Every in-progress state found.
        states: Vec<preflight::State>,
    },

    /// An operation that is only safe in an independent repository met a linked worktree.
    #[error("{repo} is a linked worktree of {git_dir}", repo = repo.display(), git_dir = git_dir.display())]
    GitLinkedWorktree {
        /// The checkout that was operated on.
        repo: PathBuf,
        /// Its per-worktree Git directory.
        git_dir: PathBuf,
    },

    /// A branch that had to exist did not.
    #[error("{repo} has no branch {branch:?}", repo = repo.display())]
    GitUnknownBranch {
        /// The repository that was inspected.
        repo: PathBuf,
        /// The branch that was expected.
        branch: String,
    },
}

impl Error {
    /// Attach a path to an [`std::io::Error`], for use with `map_err`.
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { path: path.into(), source }
    }
}

/// The result type every fallible `nodal-core` function returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;
