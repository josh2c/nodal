//! `git ls-tree` records: the one view of a commit's tree the rest of Nodal reads, and
//! the tree object each of a list of commits names.

use std::path::{Path, PathBuf};

use super::{cmd, oid::Oid};
use crate::error::{Error, Result};

/// The tree object each of `commits` names, in the order they were given.
///
/// One process for the whole list. `git rev-parse` prints one line per revision it was
/// given, in order, so a caller comparing many commits against many tips pays twice
/// rather than once per commit. `--verify` is not given, because it accepts one revision
/// and this asks about a list; what it would have guarded is guarded instead by comparing
/// the number of answers to the number of questions.
///
/// `--end-of-options` is not given either: a plain `rev-parse` echoes an argument it does
/// not recognise rather than refusing it, and that separator came back as a line of the
/// answer. Nothing here needs it. Every argument is `<40 hex characters>^{tree}`, built
/// from an [`Oid`] this repository printed, which no option parser can read as a flag.
///
/// Every commit must be one this repository has. The caller read them out of this
/// repository, and a revision Git cannot resolve fails the whole reading rather than
/// shifting the answers under the ones after it.
///
/// # Errors
/// [`Error::Git`] when a revision could not be resolved, [`Error::GitOid`] on unreadable
/// output, and [`Error::GitParse`] when the answer has a different number of lines from
/// the question.
pub(super) fn of(repo: &Path, commits: &[Oid]) -> Result<Vec<Oid>> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }
    let asked: Vec<String> =
        commits.iter().map(|oid| format!("{}^{{tree}}", oid.as_str())).collect();
    let mut args = vec!["rev-parse"];
    args.extend(asked.iter().map(String::as_str));
    let output = cmd::run_ok(repo, &args)?;
    let lines = output.lines()?;
    if lines.len() != commits.len() {
        return Err(Error::GitParse { args: output.args.clone(), record: lines.join(" ") });
    }
    lines.iter().map(|line| Oid::parse(line)).collect()
}

/// What a tree entry points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file.
    Blob,
    /// A subdirectory.
    Tree,
    /// A submodule (gitlink).
    Commit,
    /// An annotated tag object.
    Tag,
}

impl Kind {
    /// Parse the type word `git ls-tree` prints.
    fn parse(word: &str) -> Option<Self> {
        match word {
            "blob" => Some(Self::Blob),
            "tree" => Some(Self::Tree),
            "commit" => Some(Self::Commit),
            "tag" => Some(Self::Tag),
            _ => None,
        }
    }
}

/// One entry of a tree at a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The six-digit octal mode, as printed.
    pub mode: String,
    /// What the entry points at.
    pub kind: Kind,
    /// The object id of the entry, which is what fingerprints are built from.
    pub oid: Oid,
    /// Path relative to the repository root.
    pub path: PathBuf,
}

/// Parse the NUL-separated records of `git ls-tree -z`.
///
/// Each record is `<mode> SP <type> SP <oid> TAB <path>`.
///
/// # Errors
/// [`Error::GitParse`] when a record does not have that shape, [`Error::GitOid`] when
/// the object id is malformed.
pub(super) fn parse(args: &[String], records: &[&str]) -> Result<Vec<Entry>> {
    records.iter().map(|record| parse_entry(args, record)).collect()
}

/// Parse one `ls-tree` record.
fn parse_entry(args: &[String], record: &str) -> Result<Entry> {
    let malformed = || Error::GitParse { args: args.to_vec(), record: record.to_owned() };
    let (head, path) = record.split_once('\t').ok_or_else(malformed)?;
    let mut words = head.split(' ');
    let (Some(mode), Some(kind), Some(oid), None) =
        (words.next(), words.next(), words.next(), words.next())
    else {
        return Err(malformed());
    };
    Ok(Entry {
        mode: mode.to_owned(),
        kind: Kind::parse(kind).ok_or_else(malformed)?,
        oid: Oid::parse(oid)?,
        path: PathBuf::from(path),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::{Kind, parse};

    const OID: &str = "1e2f3a4b5c6d7e8f90112233445566778899aabb";

    #[test]
    fn parses_a_blob_and_a_gitlink() {
        let blob = format!("100644 blob {OID}\tpkg/a b.json");
        let link = format!("160000 commit {OID}\tvendor/dep");
        let entries = parse(&[], &[blob.as_str(), link.as_str()]).unwrap();
        assert_eq!(entries[0].kind, Kind::Blob);
        assert_eq!(entries[0].mode, "100644");
        assert_eq!(entries[0].path, Path::new("pkg/a b.json"));
        assert_eq!(entries[1].kind, Kind::Commit);
    }

    #[test]
    fn rejects_records_without_a_tab_or_with_an_unknown_type() {
        assert!(parse(&[], &[&format!("100644 blob {OID} nope")]).is_err());
        assert!(parse(&[], &[&format!("100644 thing {OID}\tp")]).is_err());
    }
}
