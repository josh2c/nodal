//! The command line of the measurement harness.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

/// What to measure, how many times, and the number it must stay under.
pub struct Options {
    /// The binary to spawn.
    pub binary: PathBuf,
    /// The arguments to spawn it with. `--version` is the fast path this gate measures.
    pub arguments: Vec<OsString>,
    /// How many runs to discard before recording.
    pub warmup: usize,
    /// How many runs to record.
    pub runs: usize,
    /// The median must stay at or under this.
    pub threshold: Duration,
    /// Whether to judge the median. A reference measurement only reports.
    pub gate: bool,
}

/// How to call the harness.
pub const USAGE: &str = "usage: nodal-startup-bench [--runs N] [--warmup N] \
                         [--threshold-ms MS] [--report-only] <binary> [argument...]";

impl Options {
    /// Read the options from the process arguments, skipping the program name.
    pub fn from_environment() -> Result<Self, String> {
        Self::parse(std::env::args_os().skip(1))
    }

    /// Read the options from an argument list.
    ///
    /// Flags come first and the binary last, so that arguments meant for the binary are
    /// never read as flags. `--` also ends the flags, for a binary whose own first
    /// argument starts with a dash.
    fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut arguments = arguments.peekable();
        while let Some(argument) = arguments.next_if(is_flag) {
            let name = argument.to_string_lossy().into_owned();
            if name == "--" {
                break;
            }
            if name == "--report-only" {
                options.gate = false;
                continue;
            }
            let value = arguments.next().ok_or_else(|| format!("{name} needs a value"))?;
            options.set(&name, &value)?;
        }
        options.binary = PathBuf::from(arguments.next().ok_or_else(|| USAGE.to_owned())?);
        options.arguments = arguments.collect();
        options.check()
    }

    /// Apply one flag.
    fn set(&mut self, name: &str, value: &OsString) -> Result<(), String> {
        let text = value.to_str().ok_or_else(|| format!("the value of {name} is not text"))?;
        match name {
            "--runs" => self.runs = number(name, text)?,
            "--warmup" => self.warmup = number(name, text)?,
            "--threshold-ms" => self.threshold = milliseconds(name, text)?,
            _ => return Err(format!("unknown option {name}\n{USAGE}")),
        }
        Ok(())
    }

    /// Refuse a set of options that cannot produce a median.
    fn check(self) -> Result<Self, String> {
        if self.runs == 0 {
            return Err("--runs must be at least 1".to_owned());
        }
        Ok(self)
    }
}

impl Default for Options {
    /// The defaults the gate runs with. `ci/startup-budget.sh` states the threshold, so
    /// the number CI enforces is read in one place; this default only serves a local run.
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            arguments: Vec::new(),
            warmup: 20,
            runs: 200,
            threshold: Duration::from_millis(5),
            gate: true,
        }
    }
}

/// Whether an argument is a flag rather than the binary to measure.
fn is_flag(argument: &OsString) -> bool {
    argument.as_encoded_bytes().first() == Some(&b'-')
}

/// Read a count.
fn number(name: &str, text: &str) -> Result<usize, String> {
    text.parse().map_err(|_| format!("{name} needs a whole number, not {text}"))
}

/// Read a duration stated in milliseconds.
fn milliseconds(name: &str, text: &str) -> Result<Duration, String> {
    let value: f64 =
        text.parse().map_err(|_| format!("{name} needs a number of milliseconds, not {text}"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{name} needs a positive number, not {text}"));
    }
    Ok(Duration::from_secs_f64(value / 1_000.0))
}

#[cfg(test)]
mod tests {
    use super::Options;
    use std::ffi::OsString;
    use std::path::Path;
    use std::time::Duration;

    fn parse(arguments: &[&str]) -> Result<Options, String> {
        Options::parse(arguments.iter().map(OsString::from))
    }

    #[test]
    fn the_binary_and_its_arguments_are_kept_apart_from_the_flags() {
        let options = parse(&["--runs", "3", "target/release/nodal", "--version"])
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(options.runs, 3);
        assert_eq!(options.binary, Path::new("target/release/nodal"));
        assert_eq!(options.arguments, vec![OsString::from("--version")]);
    }

    #[test]
    fn a_threshold_is_stated_in_milliseconds() {
        let options =
            parse(&["--threshold-ms", "12.5", "nodal"]).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(options.threshold, Duration::from_micros(12_500));
    }

    #[test]
    fn two_dashes_end_the_flags() {
        let options = parse(&["--", "nodal", "-x"]).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(options.binary, Path::new("nodal"));
        assert_eq!(options.arguments, vec![OsString::from("-x")]);
    }

    #[test]
    fn defaults_hold_when_no_flag_is_given() {
        let options = parse(&["nodal"]).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(options.runs, 200);
        assert_eq!(options.warmup, 20);
        assert_eq!(options.threshold, Duration::from_millis(5));
    }

    #[test]
    fn a_reference_measurement_only_reports() {
        let options = parse(&["--report-only", "true"]).unwrap_or_else(|error| panic!("{error}"));
        assert!(!options.gate);
        assert!(parse(&["true"]).unwrap_or_else(|error| panic!("{error}")).gate);
    }

    #[test]
    fn bad_options_are_refused() {
        assert!(parse(&[]).is_err());
        assert!(parse(&["--runs"]).is_err());
        assert!(parse(&["--runs", "0", "nodal"]).is_err());
        assert!(parse(&["--runs", "many", "nodal"]).is_err());
        assert!(parse(&["--threshold-ms", "0", "nodal"]).is_err());
        assert!(parse(&["--unknown", "1", "nodal"]).is_err());
    }
}
