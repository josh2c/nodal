//! Directories an ignore rule covers, as `git ls-files` lists them.
//!
//! Doctor's machine scan names the three largest of these so a person can see where a
//! clone's weight sits. The listing is one process and it writes nothing.

use std::path::{Path, PathBuf};

use super::cmd;
use crate::error::Result;

/// Directories Git's ignore rules cover, relative to `repo`.
///
/// Files an ignore rule covers are left out: the question is which directories a person
/// could delete, not which logs they could.
///
/// # Errors
/// [`crate::Error::Git`] when `git ls-files` failed, [`crate::Error::GitEncoding`] when
/// a path is not UTF-8.
pub(super) fn directories(repo: &Path) -> Result<Vec<PathBuf>> {
    let output = cmd::run_ok(
        repo,
        &["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"],
    )?;
    let mut directories = Vec::new();
    for record in output.records()? {
        let relative = PathBuf::from(record.trim_end_matches('/'));
        if relative.as_os_str().is_empty() {
            continue;
        }
        let path = repo.join(&relative);
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            directories.push(relative);
        }
    }
    Ok(directories)
}
