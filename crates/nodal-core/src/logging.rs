//! Process-wide tracing setup: one place installs the subscriber.
//!
//! Diagnostics go to stderr so that `--json` output on stdout stays machine-readable.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::util::SubscriberInitExt;

use crate::error::{Error, Result};

/// Environment variable that overrides the filter built from `-v` flags.
pub const FILTER_ENV: &str = "NODAL_LOG";

/// How much detail the CLI writes to stderr.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verbosity {
    /// Warnings and errors only. The default for a non-interactive tool.
    #[default]
    Normal,
    /// `-v`: Nodal's own info-level progress.
    Verbose,
    /// `-vv` or more: Nodal's debug traces, including spawned commands.
    Trace,
}

impl Verbosity {
    /// Map the number of `-v` flags onto a level.
    #[must_use]
    pub fn from_occurrences(count: u8) -> Self {
        match count {
            0 => Self::Normal,
            1 => Self::Verbose,
            _ => Self::Trace,
        }
    }

    /// The [`EnvFilter`] directive this level corresponds to.
    #[must_use]
    pub fn directive(self) -> &'static str {
        match self {
            Self::Normal => "warn",
            Self::Verbose => "warn,nodal=info,nodal_core=info",
            Self::Trace => "warn,nodal=debug,nodal_core=debug",
        }
    }
}

/// Install the process-wide subscriber. Call once, early, from the binary.
///
/// # Errors
///
/// Returns [`Error::LogFilter`] if `NODAL_LOG` is set but unparseable, and
/// [`Error::LoggingAlreadyInitialised`] if a subscriber is already in place.
pub fn init(verbosity: Verbosity) -> Result<()> {
    let filter = match std::env::var(FILTER_ENV) {
        Ok(filter) => {
            EnvFilter::try_new(&filter).map_err(|source| Error::LogFilter { filter, source })?
        }
        Err(_) => EnvFilter::new(verbosity.directive()),
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .finish()
        .try_init()
        .map_err(|_| Error::LoggingAlreadyInitialised)
}

#[cfg(test)]
mod tests {
    use super::Verbosity;

    #[test]
    fn occurrences_saturate_at_trace() {
        assert_eq!(Verbosity::from_occurrences(0), Verbosity::Normal);
        assert_eq!(Verbosity::from_occurrences(1), Verbosity::Verbose);
        assert_eq!(Verbosity::from_occurrences(2), Verbosity::Trace);
        assert_eq!(Verbosity::from_occurrences(9), Verbosity::Trace);
    }

    #[test]
    fn default_is_quiet() {
        assert_eq!(Verbosity::default(), Verbosity::Normal);
        assert_eq!(Verbosity::default().directive(), "warn");
    }
}
