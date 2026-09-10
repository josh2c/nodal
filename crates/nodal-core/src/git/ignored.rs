//! What an ignore rule covers, as `git ls-files` lists it.
//!
//! Two callers, one listing. Doctor's machine scan names the three largest ignored
//! directories of a clone, so a person can see where its weight sits. The trash prune
//! ([`crate::workspace::prune`]) asks the same question for a different reason: it
//! removes generated state from a reclaimed home, and an ignore rule is the only
//! evidence it accepts that a directory holds no work.
//!
//! That is why the prune reads Git rather than the filesystem. `--others` lists what no
//! commit holds, so a directory the project tracks is not in the answer at all, and a
//! removal built on this listing cannot reach a tracked path. The listing is one
//! process and it writes nothing.

use std::path::{Path, PathBuf};

use super::cmd;
use crate::error::Result;

/// One path an ignore rule covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Where it is, relative to the checkout.
    pub relative: PathBuf,
    /// Whether it is a directory. A symbolic link is not one: the metadata is read
    /// without following links, so a link to a directory elsewhere reads as a file and
    /// nothing removes what it points at.
    pub directory: bool,
}

/// Everything Git's ignore rules cover, relative to `repo`, files as well as
/// directories.
///
/// `--directory` collapses an ignored directory into one record and does not descend
/// into it, so a `node_modules` of forty thousand files is one entry and a cache inside
/// a cache is named once.
///
/// A record that names nothing on disk is left out. Git answers about the rules and the
/// index; this answers about what is there, which is what a size and a removal need.
///
/// # Errors
/// [`crate::Error::Git`] when `git ls-files` failed, [`crate::Error::GitEncoding`] when
/// a path is not UTF-8.
pub(super) fn entries(repo: &Path) -> Result<Vec<Entry>> {
    let output = cmd::run_ok(
        repo,
        &["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"],
    )?;
    let mut entries = Vec::new();
    for record in output.records()? {
        let relative = PathBuf::from(record.trim_end_matches('/'));
        if relative.as_os_str().is_empty() {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(repo.join(&relative)) else {
            continue;
        };
        entries.push(Entry { relative, directory: metadata.is_dir() });
    }
    Ok(entries)
}

/// Directories Git's ignore rules cover, relative to `repo`.
///
/// Files an ignore rule covers are left out: the question is which directories a person
/// could delete, not which logs they could.
///
/// # Errors
/// As [`entries`].
pub(super) fn directories(repo: &Path) -> Result<Vec<PathBuf>> {
    Ok(entries(repo)?
        .into_iter()
        .filter(|entry| entry.directory)
        .map(|entry| entry.relative)
        .collect())
}
