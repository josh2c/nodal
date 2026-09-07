//! Where a build says what it is doing.
//!
//! A base is built by the first `nodal new` that needs one, and building it takes as
//! long as a clone and an install take. A user who asked for a unit and got silence
//! assumes the tool has hung, so every step of a build reports itself before it starts.
//!
//! The sink is a trait rather than a print, for three reasons: the same build runs
//! under `nodal base build`, under `nodal new` and under the resolver that finishes an
//! interrupted one; the tests read the lines back and assert on them; and a machine
//! reading `--json` must not be given a stream of prose on standard output. Nothing
//! here is written to standard output: progress is standard error, and the answer is
//! standard output.

use std::sync::{Arc, Mutex};

/// Where the lines a build writes about itself go.
pub trait Reporter: Send + Sync {
    /// One line about what is happening now, without a trailing newline.
    fn line(&self, message: &str);
}

/// Says nothing. What a caller that only wants the answer passes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Silent;

impl Reporter for Silent {
    fn line(&self, _message: &str) {}
}

/// Writes each line to standard error, so a build's progress never mixes with the
/// answer a `--json` reader is parsing on standard output.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stderr;

impl Reporter for Stderr {
    fn line(&self, message: &str) {
        eprintln!("{message}");
    }
}

/// The sink a command reports a build to.
///
/// One function rather than the same three lines in every command: standard error when
/// a person is reading, and nothing at all when a tool asked for JSON, so that the
/// answer on standard output is the whole of the output.
#[must_use]
pub fn sink(json: bool) -> Arc<dyn Reporter> {
    if json { Arc::new(Silent) } else { Arc::new(Stderr) }
}

/// Keeps every line, in order. What a test asserts on.
#[derive(Debug, Default)]
pub struct Collector {
    /// The lines so far. A build's steps run on one thread, but the trait is shared
    /// behind an `Arc`, so the field is behind a lock rather than a cell.
    lines: Mutex<Vec<String>>,
}

impl Collector {
    /// The lines reported so far, in order. A poisoned lock reports nothing rather
    /// than panicking: progress is never the reason a build fails.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().map_or_else(|_| Vec::new(), |lines| lines.clone())
    }
}

impl Reporter for Collector {
    fn line(&self, message: &str) {
        if let Ok(mut lines) = self.lines.lock() {
            lines.push(message.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Collector, Reporter, Silent};

    #[test]
    fn a_collector_keeps_the_order_lines_arrived_in() {
        let collector = Collector::default();
        collector.line("first");
        collector.line("second");
        assert_eq!(collector.lines(), vec!["first".to_owned(), "second".to_owned()]);
    }

    #[test]
    fn silence_accepts_a_line_and_keeps_nothing() {
        Silent.line("ignored");
    }
}
