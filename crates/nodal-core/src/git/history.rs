//! What a branch did: the commits it added, and the files those commits changed.
//!
//! Two readings, both of them plumbing over a revision range, and both of them used to
//! compile a unit's memory ([`crate::context`]). A ledger says what a sibling unit has
//! done, and the honest answer to that is the commits on its branch and the files they
//! touched — never a summary somebody wrote down.
//!
//! Both parsers read NUL-separated records, because a path is bytes and a subject line
//! is free text: a reader that split on whitespace would break on the first file with a
//! space in its name, which is the kind of failure that appears once a project is real.

use std::path::PathBuf;

use super::cmd::Output;
use super::oid::Oid;
use super::status::Change;
use crate::error::{Error, Result};

/// One commit of a range, as a ledger names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The commit itself.
    pub oid: Oid,
    /// Its subject line, which is what a person reads to know what it was.
    pub subject: String,
}

impl Commit {
    /// The short form of the identifier, as Git prints it.
    #[must_use]
    pub fn short(&self) -> &str {
        let text = self.oid.as_str();
        text.get(..SHORT).unwrap_or(text)
    }
}

/// How many characters of an object identifier a ledger line carries. Git's own default
/// abbreviation, so a line can be pasted into a `git show`.
const SHORT: usize = 7;

/// One file a range of commits changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// What happened to it.
    pub change: Change,
    /// Its path, relative to the repository root. For a rename or a copy, where it
    /// ended up.
    pub path: PathBuf,
    /// Where it came from, for a rename or a copy.
    pub origin: Option<PathBuf>,
}

impl FileChange {
    /// The single letter Git uses for this change, which is what a ledger line carries.
    #[must_use]
    pub const fn letter(&self) -> char {
        match self.change {
            Change::Added => 'A',
            Change::Modified | Change::Unmodified => 'M',
            Change::Deleted => 'D',
            Change::Renamed => 'R',
            Change::Copied => 'C',
            Change::TypeChanged => 'T',
            Change::Other(code) => code,
        }
    }
}

/// Read `git log -z --format=%H %s` output.
///
/// # Errors
/// [`Error::GitEncoding`] when the output is not UTF-8, [`Error::GitParse`] when a
/// record is not an identifier followed by a subject.
pub fn commits(output: &Output) -> Result<Vec<Commit>> {
    let mut commits = Vec::new();
    for record in output.records()? {
        let (id, subject) = record.trim_start().split_once(' ').unwrap_or((record.trim(), ""));
        let oid = Oid::parse(id).map_err(|_| Error::GitParse {
            args: output.args.clone(),
            record: record.to_owned(),
        })?;
        commits.push(Commit { oid, subject: subject.trim().to_owned() });
    }
    Ok(commits)
}

/// Read `git diff --name-status -z` output.
///
/// A rename and a copy carry two paths, so the records are read as a stream rather than
/// as pairs: the status decides how many paths follow it.
///
/// # Errors
/// [`Error::GitEncoding`] when the output is not UTF-8, [`Error::GitParse`] when a
/// status has no path after it.
pub fn changes(output: &Output) -> Result<Vec<FileChange>> {
    let records = output.records()?;
    let mut reader = records.into_iter();
    let mut changes = Vec::new();
    while let Some(status) = reader.next() {
        let change = Change::parse(first_character(status));
        let missing = || Error::GitParse { args: output.args.clone(), record: status.to_owned() };
        let first = reader.next().ok_or_else(missing)?;
        let (origin, path) = match change {
            Change::Renamed | Change::Copied => {
                (Some(PathBuf::from(first)), PathBuf::from(reader.next().ok_or_else(missing)?))
            }
            _ => (None, PathBuf::from(first)),
        };
        changes.push(FileChange { change, path, origin });
    }
    Ok(changes)
}

/// The status letter of a `--name-status` record, which may carry a similarity score
/// after it (`R100`). A record with no letter at all reads as a change this version
/// does not model, rather than as a failure.
fn first_character(record: &str) -> char {
    record.chars().next().unwrap_or('?')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::Path;

    use super::{Change, changes, commits};
    use crate::git::cmd::Output;

    fn output(text: &str) -> Output {
        Output {
            args: vec![String::from("log")],
            stdout: text.as_bytes().to_vec(),
            stderr: String::new(),
            code: Some(0),
        }
    }

    #[test]
    fn a_subject_with_spaces_in_it_stays_one_subject() {
        let text = format!("{0} fix the two-digit year parser\0{0} second\0", "a".repeat(40));
        let read = commits(&output(&text)).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].subject, "fix the two-digit year parser");
        assert_eq!(read[0].short(), "aaaaaaa");
    }

    #[test]
    fn a_commit_with_an_empty_subject_is_still_a_commit() {
        let read = commits(&output(&format!("{}\0", "b".repeat(40)))).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].subject, "");
    }

    #[test]
    fn a_path_with_a_space_in_it_is_one_path() {
        let read = changes(&output("M\0src/two words.ts\0")).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].path, Path::new("src/two words.ts"));
        assert_eq!(read[0].change, Change::Modified);
    }

    #[test]
    fn a_rename_reads_both_of_its_paths() {
        let read = changes(&output("R100\0old.ts\0new.ts\0A\0added.ts\0")).unwrap();
        assert_eq!(read.len(), 2, "{read:?}");
        assert_eq!(read[0].origin.as_deref(), Some(Path::new("old.ts")));
        assert_eq!(read[0].path, Path::new("new.ts"));
        assert_eq!(read[0].letter(), 'R');
        assert_eq!(read[1].path, Path::new("added.ts"));
        assert_eq!(read[1].letter(), 'A');
    }

    #[test]
    fn a_status_with_no_path_after_it_is_a_parse_failure() {
        assert!(changes(&output("M\0")).is_err());
    }
}
