//! The line in `CLAUDE.md` and `AGENTS.md` that sends an agent to the memory.
//!
//! An agent reads the file its own vendor tells it to read. Nodal writes one line into
//! each of those two files, naming `WORKUNIT.md`, and nothing else: the memory is one
//! file, and a second copy of it in a vendor's file would be a second thing to keep
//! true.
//!
//! Three rules, and the third is the one that matters.
//!
//! The line is idempotent. It carries a marker, so a second write finds the line it
//! wrote and replaces it rather than adding another.
//!
//! A file Nodal creates is hidden from `git status`, through `.git/info/exclude`, for
//! the reason the activation files are: a home whose untracked files show up is a home
//! the uniqueness check calls dirty, and a dirty home is one nobody can reclaim.
//!
//! **A file the project tracks is left exactly as it is.** Many projects commit their
//! own `CLAUDE.md`. Writing a line into one would change a file under version control,
//! put the unit's home permanently in `git status`, and put the line in the diff of
//! every pull request the unit ever opens. Nodal states that it did not write there
//! instead. The memory is still written, and `WORKUNIT.md` is still the file to read.

use std::path::Path;

use crate::env::files;
use crate::git::Git;
use crate::{Error, Result};

/// The vendor files a pointer goes into, in the order they are written.
pub const FILES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];

/// What marks the line as Nodal's, so the next write replaces it.
pub const MARK: &str = "<!-- nodal -->";

/// The line itself.
pub const LINE: &str = "<!-- nodal --> Read `WORKUNIT.md` first: nodal writes this unit's \
                        objective, branch, changed files and every other open unit into it, and \
                        writes it again on every command.";

/// What one pass over a home's pointer files did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pointed {
    /// The files the line is now in.
    pub written: Vec<String>,
    /// The files that were left alone, and why.
    pub notes: Vec<String>,
}

/// Put the pointer in each vendor file of `home`, and hide what this created.
///
/// # Errors
/// [`Error::Io`] when a file cannot be read or written. A file the project tracks is a
/// note rather than an error.
pub fn write(home: &Path, git: &Git) -> Result<Pointed> {
    let mut pointed = Pointed::default();
    let tracked = match git.tracked(&FILES) {
        Ok(tracked) => tracked,
        Err(error) => {
            // Which files Git tracks is what decides whether Nodal may write in one. A
            // compile that could not ask leaves both alone: writing in a tracked file
            // is the harm the question exists to prevent, and a note is the cost of
            // not knowing.
            pointed.notes.push(format!("pointer: git could not be asked what it tracks: {error}"));
            hide(git, &[], &mut pointed);
            return Ok(pointed);
        }
    };
    let mut created: Vec<&str> = Vec::new();
    for name in FILES {
        if tracked.iter().any(|path| path == Path::new(name)) {
            pointed
                .notes
                .push(format!("{name}: the project tracks it, so nodal did not write in it"));
            continue;
        }
        let path = home.join(name);
        let existing = read(&path)?;
        if existing.is_none() {
            created.push(name);
        }
        let text = amended(existing.as_deref());
        if existing.as_deref() != Some(text.as_str()) {
            std::fs::write(&path, &text).map_err(Error::io(&path))?;
        }
        pointed.written.push(String::from(name));
    }
    hide(git, &created, &mut pointed);
    Ok(pointed)
}

/// Tell Git to leave alone the memory, and the vendor files Nodal itself created.
///
/// A vendor file that was already there is the person's own, whether or not Git tracks
/// it, so its place in `git status` stays theirs to decide.
fn hide(git: &Git, created: &[&str], pointed: &mut Pointed) {
    let mut lines = vec![format!("/{}", super::FILE)];
    lines.extend(created.iter().map(|name| format!("/{name}")));
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    let result = git.git_dir().and_then(|dir| files::exclude(&dir, &borrowed));
    if let Err(error) = result {
        pointed.notes.push(format!("info/exclude: {error}"));
    }
}

/// A file's text, or `None` when there is no such file.
fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// The file's text with the pointer in it, exactly once.
///
/// A line Nodal wrote before is replaced where it stands, so a person who put the
/// pointer where they wanted it keeps it there.
#[must_use]
pub fn amended(existing: Option<&str>) -> String {
    let Some(text) = existing else {
        return format!("{LINE}\n");
    };
    if text.lines().any(|line| line.trim_start().starts_with(MARK)) {
        let replaced: Vec<String> = text
            .lines()
            .map(|line| {
                if line.trim_start().starts_with(MARK) {
                    String::from(LINE)
                } else {
                    line.to_owned()
                }
            })
            .collect();
        return with_final_newline(replaced.join("\n"));
    }
    let mut amended = text.to_owned();
    if !amended.ends_with('\n') {
        amended.push('\n');
    }
    if !amended.trim().is_empty() {
        amended.push('\n');
    }
    amended.push_str(LINE);
    amended.push('\n');
    amended
}

/// Every file Nodal writes ends with a newline.
fn with_final_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{LINE, MARK, amended};

    #[test]
    fn a_file_that_is_not_there_becomes_one_line() {
        assert_eq!(amended(None), format!("{LINE}\n"));
    }

    #[test]
    fn writing_twice_leaves_one_line() {
        let once = amended(None);
        assert_eq!(amended(Some(&once)), once);
    }

    #[test]
    fn a_person_own_file_keeps_its_text_and_gains_one_line() {
        let amended = amended(Some("# House rules\n\nRun the tests.\n"));
        assert!(amended.starts_with("# House rules"), "{amended}");
        assert!(amended.ends_with(&format!("{LINE}\n")), "{amended}");
        assert_eq!(amended.matches(MARK).count(), 1);
    }

    #[test]
    fn an_older_pointer_is_replaced_where_it_stands() {
        let older = format!("# Rules\n\n{MARK} an older sentence.\n\nMore rules.\n");
        let amended = amended(Some(&older));
        assert_eq!(amended.matches(MARK).count(), 1, "{amended}");
        assert!(amended.contains(LINE), "{amended}");
        assert!(amended.ends_with("More rules.\n"), "the line keeps its place: {amended}");
    }
}
