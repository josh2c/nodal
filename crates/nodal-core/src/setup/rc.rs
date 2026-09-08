//! The lines Nodal writes into a start-up file, and the rule that removes them again.
//!
//! # The block
//!
//! The lines go in one marked block. A marked block is what makes removal exact: the
//! uninstall does not search for a line that looks like Nodal's, it removes the region
//! between two markers Nodal wrote.
//!
//! # The round trip
//!
//! A start-up file that had the block appended and then removed must be byte-for-byte
//! the file it was before. That holds because of one rule, applied in both directions:
//! **the block always begins with exactly one newline, and removal always takes that
//! newline with it.** So a file that ended with a newline gets a blank line before the
//! block and gets it back; a file that ended without one is joined to the block by that
//! newline and ends without one again; an empty file becomes empty again.
//!
//! # What the block may contain
//!
//! The prompt hook runs in every shell a person opens. That makes this the most
//! security-sensitive text Nodal writes, and it is bounded to two rules:
//!
//! - it evaluates nothing. The block sources a file in Nodal's own state directory.
//!   Earlier drafts had the start-up file run `eval "$(nodal shell-init bash)"`, which
//!   evaluates the output of a process on every shell start. Sourcing a file Nodal
//!   wrote does the same work, starts no process, and gives a person a file they can
//!   read before they trust it.
//! - it reaches no network, and neither does the script it loads.
//!
//! `tests/safety/tests/no_network.rs` reads the emitted text and asserts both.

use std::path::Path;

use crate::runtime::shells::Shell;

/// The first line of the block.
pub const BEGIN: &str = "# >>> nodal >>>";

/// The last line of the block.
pub const END: &str = "# <<< nodal <<<";

/// The line between them that says what wrote this and what removes it.
const NOTE: &str = "# Written by `nodal shell-init --install`. `nodal uninstall` removes it.";

/// The same line, in a file the install made because there was none.
///
/// The two notes are how an uninstall knows whether to leave an empty file behind or
/// take it away: a file Nodal made and nobody has written in since is Nodal's to
/// remove, and a file that was already there is not. The difference is a sentence a
/// person reads rather than a record kept somewhere else about a file they own.
const NOTE_CREATED: &str = "# Written by `nodal shell-init --install`, which made this file. `nodal uninstall` \
     removes the file.";

/// The block for `shell`, loading the shim at `shim`.
///
/// `created` says whether the install made the start-up file itself. It changes one
/// comment line, and that line is what [`made_the_file`] later reads.
///
/// The block starts with one newline and ends with one, which is the whole of the
/// removal rule ([`remove`]).
#[must_use]
pub fn block(shell: Shell, shim: &Path, created: bool) -> String {
    let path = shim.to_string_lossy();
    let load = match shell {
        Shell::Bash | Shell::Zsh => format!("[ -f '{}' ] && . '{}'", posix(&path), posix(&path)),
        Shell::Fish => format!("test -f '{}'; and source '{}'", fish(&path), fish(&path)),
    };
    let note = if created { NOTE_CREATED } else { NOTE };
    format!("\n{BEGIN}\n{note}\n{load}\n{END}\n")
}

/// Whether the block in `text` says the install made this start-up file.
///
/// A file that holds no block is not one Nodal made.
#[must_use]
pub fn made_the_file(text: &str) -> bool {
    find(text).is_some_and(|range| text[range].contains("which made this file"))
}

/// Where the block is in `text`, as the byte range removal takes.
///
/// The range starts at the newline before [`BEGIN`] and ends after the newline that
/// closes [`END`]. `None` when the file holds no block.
#[must_use]
pub fn find(text: &str) -> Option<std::ops::Range<usize>> {
    let begin = line_start(text, text.find(BEGIN)?);
    let end = text[begin..].find(END).map(|offset| begin + offset)?;
    let after = text[end..].find('\n').map_or(text.len(), |offset| end + offset + 1);
    Some(begin.saturating_sub(1)..after)
}

/// `text` with the block for `shell` appended, or `None` when it already holds one.
///
/// Appending twice is the same as appending once, so an install a person runs again is
/// not a second block.
#[must_use]
pub fn add(text: &str, shell: Shell, shim: &Path, created: bool) -> Option<String> {
    if find(text).is_some() {
        return None;
    }
    Some(format!("{text}{}", block(shell, shim, created)))
}

/// `text` with the block removed, or `None` when it holds none.
///
/// The inverse of [`add`], byte for byte.
#[must_use]
pub fn remove(text: &str) -> Option<String> {
    let range = find(text)?;
    let mut kept = String::with_capacity(text.len());
    kept.push_str(&text[..range.start]);
    kept.push_str(&text[range.end..]);
    Some(kept)
}

/// The index of the start of the line `at` is on.
fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |newline| newline + 1)
}

/// A single-quoted POSIX shell word: the one character a quote does not cover is the
/// quote itself.
fn posix(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// The same for fish, which reads two escapes inside single quotes and no others.
fn fish(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use super::{BEGIN, END, add, block, made_the_file, remove};
    use crate::runtime::shells::Shell;

    /// Where a shim would be.
    fn shim() -> &'static Path {
        Path::new("/home/dev/.nodal/shims/nodal.bash")
    }

    /// Install and uninstall, as the two functions do it.
    fn round_trip(before: &str) -> String {
        let installed = add(before, Shell::Bash, shim(), false).unwrap();
        assert!(installed.contains(BEGIN) && installed.contains(END), "{installed}");
        remove(&installed).unwrap()
    }

    #[test]
    fn a_file_is_byte_identical_after_an_install_and_an_uninstall() {
        for before in [
            "",
            "\n",
            "PS1='$ '\n",
            "PS1='$ '",
            "# a comment\n\n\nexport PATH=/usr/bin\n",
            "alias ll='ls -l'\n\n",
        ] {
            assert_eq!(round_trip(before), before, "{before:?} did not survive the round trip");
        }
    }

    #[test]
    fn a_block_says_whether_the_install_made_the_file_it_is_in() {
        let made = add("", Shell::Bash, shim(), true).unwrap();
        assert!(made_the_file(&made), "{made}");
        let joined = add("PS1='$ '\n", Shell::Bash, shim(), false).unwrap();
        assert!(!made_the_file(&joined), "{joined}");
        assert!(!made_the_file("PS1='$ '\n"), "a file with no block was never made by nodal");
    }

    #[test]
    fn a_file_that_holds_the_block_is_not_given_a_second_one() {
        let once = add("PS1='$ '\n", Shell::Bash, shim(), false).unwrap();
        assert!(add(&once, Shell::Bash, shim(), false).is_none());
    }

    #[test]
    fn a_file_that_holds_no_block_is_left_alone() {
        assert!(remove("PS1='$ '\n").is_none());
    }

    #[test]
    fn lines_a_person_added_after_the_block_stay_where_they_are() {
        let installed = add("first\n", Shell::Bash, shim(), false).unwrap();
        let later = format!("{installed}last\n");
        assert_eq!(remove(&later).unwrap(), "first\nlast\n");
    }

    #[test]
    fn the_block_evaluates_nothing_and_names_the_file_it_loads() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let text = block(shell, shim(), false);
            assert!(!text.contains("eval"), "{text}");
            assert!(!text.contains("$("), "{text}");
            assert!(text.contains("/home/dev/.nodal/shims/nodal.bash"), "{text}");
        }
    }

    #[test]
    fn a_path_that_holds_a_quote_is_quoted_and_not_interpreted() {
        let odd = Path::new("/home/o'dell/.nodal/shims/nodal.bash");
        assert!(block(Shell::Bash, odd, false).contains(r"o'\''dell"));
        assert!(block(Shell::Fish, odd, false).contains(r"o\'dell"));
    }
}
