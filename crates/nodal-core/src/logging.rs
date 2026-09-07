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

/// The filter directive to use: the environment overrides the flags, but an empty or
/// blank `NODAL_LOG` is the same as not setting it.
///
/// `NODAL_LOG=` reaches a process as an empty value, which `EnvFilter` reads as "no
/// directives" and which therefore silences errors as well as progress. Nothing a user
/// can type by accident should hide an error, so blank means unset.
fn requested_filter(from_env: Option<String>, verbosity: Verbosity) -> String {
    match from_env {
        Some(filter) if !filter.trim().is_empty() => filter,
        _ => verbosity.directive().to_owned(),
    }
}

/// Install the process-wide subscriber. Call once, early, from the binary.
///
/// # Errors
///
/// Returns [`Error::LogFilter`] if `NODAL_LOG` is set to something unparseable, and
/// [`Error::LoggingAlreadyInitialised`] if a subscriber is already in place.
pub fn init(verbosity: Verbosity) -> Result<()> {
    let requested = requested_filter(std::env::var(FILTER_ENV).ok(), verbosity);
    let filter = EnvFilter::try_new(&requested)
        .map_err(|source| Error::LogFilter { filter: requested, source })?;
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
    use super::{Verbosity, requested_filter};

    #[test]
    fn occurrences_saturate_at_trace() {
        assert_eq!(Verbosity::from_occurrences(0), Verbosity::Normal);
        assert_eq!(Verbosity::from_occurrences(1), Verbosity::Verbose);
        assert_eq!(Verbosity::from_occurrences(2), Verbosity::Trace);
        assert_eq!(Verbosity::from_occurrences(9), Verbosity::Trace);
    }

    #[test]
    fn blank_env_filter_is_treated_as_unset() {
        for blank in [String::new(), String::from("  ")] {
            assert_eq!(
                requested_filter(Some(blank), Verbosity::Normal),
                Verbosity::Normal.directive(),
                "a blank NODAL_LOG must not silence errors"
            );
        }
    }

    #[test]
    fn set_env_filter_overrides_the_flags() {
        assert_eq!(requested_filter(Some(String::from("debug")), Verbosity::Normal), "debug");
        assert_eq!(requested_filter(None, Verbosity::Verbose), Verbosity::Verbose.directive());
    }

    #[test]
    fn default_is_quiet() {
        assert_eq!(Verbosity::default(), Verbosity::Normal);
        assert_eq!(Verbosity::default().directive(), "warn");
    }
}
