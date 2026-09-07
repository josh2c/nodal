//! The one error type for `nodal-core`.
//!
//! Variants are added per module as modules arrive; a variant carries the values a
//! caller needs to act on, never a pre-formatted string.

use std::path::PathBuf;

use thiserror::Error as ThisError;

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
}

impl Error {
    /// Attach a path to an [`std::io::Error`], for use with `map_err`.
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { path: path.into(), source }
    }
}

/// The result type every fallible `nodal-core` function returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;
