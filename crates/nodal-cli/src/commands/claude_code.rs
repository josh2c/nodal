//! `nodal claude-code <event>`: the commands Claude Code's hooks run.
//!
//! Each subcommand is one event. It reads the payload Claude Code sends on standard
//! input and answers on standard output in the form that event requires, which is not
//! the same form for all four ([`nodal_core::adapters::claude_code`]).
//!
//! Only one of them can fail: `worktree-create` is a provider, and Claude Code ends the
//! session when it does not answer with a directory. It therefore prints a refusal
//! Claude will reject and says why on standard error, rather than printing nothing. The
//! other three are observers and always succeed, because a session should not end
//! because a memory could not be read.

use std::io::Write;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use nodal_core::adapters::claude_code::{self, Payload};
use nodal_core::store::Store;

/// Arguments of `nodal claude-code`.
#[derive(Debug, Args)]
pub struct ClaudeCode {
    /// Which event is being answered.
    #[command(subcommand)]
    pub event: Event,
}

/// The events Nodal answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum Event {
    /// Make the unit this session will work in and print its home.
    WorktreeCreate,
    /// Print the memory of the unit this session is starting in, if it is in one.
    SessionStart,
    /// Record this session's last message as a stated handoff.
    Stop,
    /// Note that a session let go of a home. It removes nothing.
    WorktreeRemove,
}

impl ClaudeCode {
    /// Whether the event reads or writes the registry.
    ///
    /// `session-start` does neither: it reads a file in the directory it was given. It
    /// therefore does not open the registry, so a session started in a directory Nodal
    /// has never heard of does not create one.
    #[must_use]
    pub const fn needs_registry(&self) -> bool {
        !matches!(self.event, Event::SessionStart)
    }

    /// Answer the event.
    ///
    /// # Errors
    ///
    /// [`nodal_core::Error::Io`] when standard input or standard output cannot be read
    /// or written. Everything else is answered rather than propagated: see the module
    /// note.
    pub fn run(&self, store: Option<&mut Store>) -> nodal_core::Result<ExitCode> {
        let payload = Payload::read(std::io::stdin())?;
        match (self.event, store) {
            (Event::WorktreeCreate, Some(store)) => create(store, &payload),
            (Event::SessionStart, _) => Ok(start(&payload)),
            (Event::Stop, Some(store)) => Ok(observe(claude_code::stop(store, &payload))),
            (Event::WorktreeRemove, Some(store)) => {
                Ok(observe(claude_code::worktree_remove(store, &payload)))
            }
            (_, None) => Ok(ExitCode::SUCCESS),
        }
    }
}

/// The provider event: a directory on standard output, or a refusal and a reason.
fn create(store: &mut Store, payload: &Payload) -> nodal_core::Result<ExitCode> {
    match claude_code::worktree_create(store, payload) {
        Ok(home) => {
            say(&home.display().to_string())?;
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("nodal: {error}");
            say(claude_code::REFUSED)?;
            Ok(ExitCode::FAILURE)
        }
    }
}

/// The memory of the unit the session is starting in, and nothing at all otherwise.
///
/// A memory that cannot be read is a line on standard error and an empty answer. The
/// session still starts; it starts without the context it would have had.
fn start(payload: &Payload) -> ExitCode {
    match claude_code::session_start(payload) {
        Ok(Some(memory)) => print!("{memory}"),
        Ok(None) => {}
        Err(error) => eprintln!("nodal: {error}"),
    }
    ExitCode::SUCCESS
}

/// What an observer answers with: nothing, whatever happened.
fn observe(outcome: nodal_core::Result<bool>) -> ExitCode {
    if let Err(error) = outcome {
        eprintln!("nodal: {error}");
    }
    ExitCode::SUCCESS
}

/// One line on standard output, flushed, because a provider's answer is read by the
/// process that is waiting for it.
fn say(line: &str) -> nodal_core::Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "{line}").map_err(nodal_core::Error::io("<stdout>"))?;
    out.flush().map_err(nodal_core::Error::io("<stdout>"))
}
