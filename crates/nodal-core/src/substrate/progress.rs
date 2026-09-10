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

use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::sync::{Arc, Mutex};

/// Words a person types to mean yes.
const AGREED: [&str; 2] = ["y", "yes"];

/// Where the lines a build writes about itself go, and the one question it may ask.
pub trait Reporter: Send + Sync {
    /// One line about what is happening now, without a trailing newline.
    fn line(&self, message: &str);

    /// Ask the person a yes-or-no question, and say what they answered.
    ///
    /// A build has exactly one of these to ask: whether to carry on with what a
    /// failed attempt left, or to start again. It goes through this trait because
    /// every caller already hands a build one of these and none of them would
    /// otherwise have a way to answer.
    ///
    /// Yes by default, and that is the deliberate answer for a sink with nobody
    /// behind it. Carrying on with a clone that is already on disk changes nothing a
    /// person would want changed: the workspace fingerprint is the same, so the clone
    /// is the same clone the fresh build would make. Refusing here instead would stop
    /// every build after a failure until somebody typed at it, which on a build server
    /// is a build that never runs again.
    fn agrees(&self, _question: &str) -> bool {
        true
    }
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

    /// Ask, when there is a terminal to ask. A question is printed to standard error
    /// beside the progress it belongs with, and standard output stays the answer.
    ///
    /// Nothing watching means yes, as the trait says, and no question is printed: a
    /// prompt on a build server's log that nobody could have answered reads as though
    /// the build waited for something.
    fn agrees(&self, question: &str) -> bool {
        let input = std::io::stdin();
        if !input.is_terminal() {
            return true;
        }
        eprint!("{question} [y/N] ");
        if std::io::stderr().flush().is_err() {
            return false;
        }
        let mut answer = String::new();
        if input.lock().read_line(&mut answer).is_err() {
            return false;
        }
        AGREED.contains(&answer.trim().to_lowercase().as_str())
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

    #[test]
    fn a_sink_with_nobody_behind_it_agrees() {
        assert!(Silent.agrees("retry from the install step?"));
        assert!(Collector::default().agrees("retry from the install step?"));
    }
}
