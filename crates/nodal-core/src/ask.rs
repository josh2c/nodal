//! The one yes-or-no question this program knows how to put, and what an unwatched run
//! means for it.
//!
//! Five places asked a person to agree to something: a reclaim that would remove a
//! worktree, a merge, an uninstall, a hook approval, and a base build deciding what to
//! do with what a failed attempt left. Each carried its own copy of the same six lines —
//! honour the flag that answers in advance, look for a terminal, print the question,
//! read a line, compare it against the words that mean yes — and each had slipped a
//! little from the others.
//!
//! The copies agreed about the mechanism and differed about one thing only: what an
//! unwatched run means. That is the real per-question decision, so it is the argument,
//! and [`Unwatched`] is the list of answers a caller may give. Everything else is here
//! once.
//!
//! This is in the library and not in the binary because one of the five is
//! [`crate::substrate::progress`], which is here. A question is not a command-line
//! concern: it is a thing this program does to a terminal, wherever the code that needs
//! it lives.

use std::io::{BufRead as _, IsTerminal as _, Write as _};

use crate::{Error, Result};

/// The words a person types to mean yes. Anything else, including nothing, means no.
const AGREED: [&str; 2] = ["y", "yes"];

/// What an unwatched run means for one question.
///
/// A run with no terminal cannot be asked. What that is taken to mean is not the same
/// for every question, and guessing is how a script either hangs or does something
/// nobody saw, so each caller states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwatched {
    /// Take it as no, and say nothing. For a question whose no is the ordinary answer
    /// and costs nothing: the work the question was about is left undone.
    No,
    /// Refuse, and say what to pass instead. For a question whose yes changes something
    /// a person would want to have seen. The text names the flag.
    Refuse(&'static str),
    /// Take it as yes. For a question whose yes changes nothing a person would want
    /// changed, where refusing would stop every unattended run until somebody typed at
    /// it.
    Yes,
}

/// Put `question` to the person, and answer it.
///
/// `yes` is the answer given in advance by a flag, and it is honoured before anything
/// looks for a terminal: a person who has already decided is not asked again, and a
/// script that passes it never depends on what is watching.
///
/// The question goes to standard error, beside whatever else the command is saying
/// there, so that standard output stays the command's one answer.
///
/// # Errors
///
/// [`Error::InvalidValue`] when there is no terminal and `unwatched` is
/// [`Unwatched::Refuse`], and [`Error::Io`] when the question could not be printed or
/// the answer could not be read.
pub fn agreed(yes: bool, question: &str, unwatched: Unwatched) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    let input = std::io::stdin();
    if !input.is_terminal() {
        return match unwatched {
            Unwatched::No => Ok(false),
            Unwatched::Yes => Ok(true),
            Unwatched::Refuse(instead) => {
                Err(Error::InvalidValue { kind: "agreement", value: String::from(instead) })
            }
        };
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush().map_err(Error::io("<stderr>"))?;
    let mut answer = String::new();
    input.lock().read_line(&mut answer).map_err(Error::io("<stdin>"))?;
    Ok(AGREED.contains(&answer.trim().to_lowercase().as_str()))
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use super::{Unwatched, agreed};

    /// The flag is honoured before anything reads the terminal, so this is the one case
    /// a test can state without one.
    #[test]
    fn the_flag_that_answers_in_advance_is_honoured_whatever_is_watching() {
        for unwatched in [Unwatched::No, Unwatched::Yes, Unwatched::Refuse("pass --yes")] {
            assert_eq!(agreed(true, "anything?", unwatched).ok(), Some(true), "{unwatched:?}");
        }
    }

    /// A test binary has no terminal, so these run the unwatched arm as written.
    #[test]
    fn an_unwatched_run_gets_the_answer_its_caller_named() {
        assert_eq!(agreed(false, "q?", Unwatched::No).ok(), Some(false));
        assert_eq!(agreed(false, "q?", Unwatched::Yes).ok(), Some(true));

        let refused =
            agreed(false, "q?", Unwatched::Refuse("pass --yes to do it")).expect_err("a refusal");
        assert!(refused.to_string().contains("pass --yes to do it"), "{refused}");
    }
}
