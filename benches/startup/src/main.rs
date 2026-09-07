//! `nodal-startup-bench <binary> [argument...]`: time a fast path and gate on its median.
//!
//! DL-017 sets a 5 ms cold-start budget for the hot paths. This harness is what turns
//! that budget into a number CI can fail on. It spawns the release binary many times,
//! reports the median, and exits non-zero when the median is over the threshold it was
//! given. `ci/startup-budget.sh` states the threshold CI uses and why.
//!
//! The harness measures a whole spawn from the parent, so the reported number includes
//! the parent's fork and exec cost. It is an upper bound on startup, not a floor.

mod measure;
mod options;
mod stats;

use std::process::ExitCode;

use options::Options;
use stats::{Sorted, milliseconds};

/// Measure, print, and report whether the median stayed inside the threshold.
fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("startup: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Run the measurement. The bool is whether the median stayed inside the threshold.
fn run() -> Result<bool, String> {
    let options = Options::from_environment()?;
    let timings = measure::set(&options.binary, &options.arguments, options.warmup, options.runs)
        .map_err(|error| format!("{}: {error}", options.binary.display()))?;
    report(&options, &timings);
    Ok(!options.gate || timings.median() <= options.threshold)
}

/// Print the measurement, and the verdict, on every run.
///
/// Every number is printed whether the gate passes or fails, so a run that passes still
/// leaves the record a later calibration needs.
fn report(options: &Options, timings: &Sorted) {
    let command = std::iter::once(options.binary.display().to_string())
        .chain(options.arguments.iter().map(|a| a.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(" ");
    println!("startup: {command}");
    println!("startup: {} runs after {} warm-up runs", timings.count(), options.warmup);
    println!(
        "startup: median {} ms · p95 {} ms · min {} ms · max {} ms",
        milliseconds(timings.median()),
        milliseconds(timings.percentile(95)),
        milliseconds(timings.min()),
        milliseconds(timings.max())
    );
    if !options.gate {
        return;
    }
    let threshold = milliseconds(options.threshold);
    if timings.median() <= options.threshold {
        println!("startup: median is inside the {threshold} ms threshold");
    } else {
        eprintln!(
            "startup: median {} ms is over the {threshold} ms threshold",
            milliseconds(timings.median())
        );
    }
}
