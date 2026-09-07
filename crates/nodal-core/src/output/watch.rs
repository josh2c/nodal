//! The status stream: newline-delimited JSON, produced by polling.
//!
//! There is no daemon and no file watcher, so `nodal status --watch` asks the same
//! question on a timer and writes a line whenever the answer has changed. Three
//! consequences are deliberate:
//!
//! * A frame is the read type's own compact JSON, one document per line, so a consumer
//!   parses `status --watch` with exactly the reader it uses for `status --json`.
//! * Repeated identical answers are not repeated on the wire. A consumer therefore
//!   holds the last frame as current state, and quiet means unchanged rather than gone.
//! * The instant an answer was taken is written on the frame but does not make one, so
//!   a clock that moves on its own does not fill the stream (`Render::VOLATILE`).
//!
//! The polling itself is a [`Source`], so the loop can be driven by a registry in
//! production and by a fixed list of answers in a test, and [`Stream`] — the part that
//! decides whether a frame is worth writing — is pure and unit-testable on its own.

use std::io::Write;
use std::time::Duration;

use crate::output::{Render, json};

/// How often a stream asks, when its caller does not say.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(2);

/// Where a stream's answers come from.
pub trait Source {
    /// The read type this source answers with.
    type Item: Render;

    /// Answer the question once. Called on every tick.
    ///
    /// # Errors
    ///
    /// Whatever reading the answer returns; the stream stops and propagates it.
    fn poll(&mut self) -> crate::Result<Self::Item>;
}

/// How a stream runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The wait between polls.
    pub interval: Duration,
    /// Stop after this many polls. `None` runs until the process is stopped, which is
    /// what the CLI passes; a test passes a count so the loop is finite and its output
    /// is a fixed string.
    pub max_polls: Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self { interval: DEFAULT_INTERVAL, max_polls: None }
    }
}

/// What a run of a stream did, so a caller can report it without counting lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// How many times the source was asked.
    pub polls: u64,
    /// How many frames were written; never more than `polls`.
    pub frames: u64,
}

/// The change filter: it holds the last frame written and emits only what differs.
#[derive(Debug, Clone, Default)]
pub struct Stream {
    previous: Option<String>,
}

impl Stream {
    /// A stream that has written nothing, so its first frame is always emitted.
    #[must_use]
    pub const fn new() -> Self {
        Self { previous: None }
    }

    /// The line this value should put on the wire, or `None` when it says nothing the
    /// last line did not already say. `T::VOLATILE` fields are written but do not count
    /// as news, so a clock that moves on its own does not fill the stream.
    ///
    /// # Errors
    ///
    /// [`Error::Render`](crate::Error::Render) if the value cannot be encoded as JSON.
    pub fn frame<T: Render>(&mut self, value: &T) -> crate::Result<Option<String>> {
        let key = change_key(value)?;
        if self.previous.as_ref() == Some(&key) {
            return Ok(None);
        }
        self.previous = Some(key);
        json::compact(value).map(Some)
    }
}

/// What is compared between two answers: the document without its volatile fields.
fn change_key<T: Render>(value: &T) -> crate::Result<String> {
    let mut document = serde_json::to_value(value)
        .map_err(|source| crate::Error::Render { kind: T::KIND, source })?;
    if let Some(object) = document.as_object_mut() {
        for field in T::VOLATILE {
            object.remove(*field);
        }
    }
    serde_json::to_string(&document)
        .map_err(|source| crate::Error::Render { kind: T::KIND, source })
}

/// Poll `source` on `options.interval` and write every changed answer to `out`.
///
/// Sleeping is the only thing this function does that a test cannot observe, and it is
/// skipped when the interval is zero, which is how the suite drives a finite run.
///
/// # Errors
///
/// Whatever the source returns, [`Error::Render`](crate::Error::Render) if an answer
/// cannot be encoded, and [`Error::Io`](crate::Error::Io) if the destination refuses
/// the bytes.
pub fn run<S: Source, W: Write>(
    source: &mut S,
    out: &mut W,
    options: Options,
) -> crate::Result<Report> {
    let mut stream = Stream::new();
    let mut report = Report::default();
    while options.max_polls.is_none_or(|max| report.polls < max) {
        if report.polls > 0 && !options.interval.is_zero() {
            std::thread::sleep(options.interval);
        }
        let value = source.poll()?;
        report.polls += 1;
        if let Some(line) = stream.frame(&value)? {
            out.write_all(line.as_bytes()).map_err(crate::Error::io(S::Item::KIND))?;
            out.flush().map_err(crate::Error::io(S::Item::KIND))?;
            report.frames += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::time::Duration;

    use serde::Serialize;

    use super::{Options, Source, Stream, run};
    use crate::output::{Doc, Render};

    #[derive(Debug, Serialize)]
    struct Ping {
        now: u32,
        beat: u32,
    }

    impl Render for Ping {
        const KIND: &'static str = "ping";
        fn doc(&self) -> Doc {
            Doc::new()
        }
    }

    struct Fixed {
        answers: Vec<u32>,
        next: usize,
    }

    impl Source for Fixed {
        type Item = Ping;
        fn poll(&mut self) -> crate::Result<Ping> {
            let beat = self.answers.get(self.next).copied().unwrap_or(0);
            self.next += 1;
            Ok(Ping { now: u32::try_from(self.next).unwrap_or(0), beat })
        }
    }

    fn drive(answers: &[u32]) -> (String, super::Report) {
        let mut source = Fixed { answers: answers.to_vec(), next: 0 };
        let mut out = Vec::new();
        let options = Options {
            interval: Duration::ZERO,
            max_polls: Some(answers.len().try_into().unwrap_or(0)),
        };
        let report = run(&mut source, &mut out, options).expect("the fixed source answers");
        (String::from_utf8(out).expect("json is utf-8"), report)
    }

    #[test]
    fn one_document_per_line() {
        let (text, report) = drive(&[1, 2, 3]);
        assert_eq!(
            text,
            "{\"now\":1,\"beat\":1}\n{\"now\":2,\"beat\":2}\n{\"now\":3,\"beat\":3}\n"
        );
        assert_eq!((report.polls, report.frames), (3, 3));
    }

    /// The clock moves on every poll, so this also shows that a volatile field is
    /// written on the frame it does appear in without making a frame of its own.
    #[test]
    fn an_unchanged_answer_says_nothing() {
        let (text, report) = drive(&[1, 1, 1, 2]);
        assert_eq!(text, "{\"now\":1,\"beat\":1}\n{\"now\":4,\"beat\":2}\n");
        assert_eq!((report.polls, report.frames), (4, 2));
    }

    #[test]
    fn the_first_answer_is_always_a_frame() {
        let mut stream = Stream::new();
        assert!(stream.frame(&Ping { now: 0, beat: 0 }).expect("encodes").is_some());
        assert!(stream.frame(&Ping { now: 1, beat: 0 }).expect("encodes").is_none());
    }
}
