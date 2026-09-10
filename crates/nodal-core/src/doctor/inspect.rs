//! One clone: branch, unpushed commits, dirty paths, size, ignored directories, age.
//!
//! Unpushed is the `remote_containment` predicate doctor already uses. The walk that
//! sizes the clone also sizes ignored directories, so a tree is read once.

use std::fs;
use std::path::{Path, PathBuf};

use crate::git::Git;
use crate::model::Timestamp;
use crate::output::view::machine::{CloneRow, IgnoredDir};
use crate::{Error, Result};

/// How many ignored directories a row keeps.
const TOP: usize = 3;

/// What inspecting one clone produced.
#[derive(Debug)]
pub struct Inspected {
    /// The row.
    pub row: CloneRow,
    /// Directory entries the size walk looked at.
    pub entries: u64,
}

/// Read one clone. A failure is the clone, not the rest of the survey.
///
/// # Errors
/// [`Error::NotARepository`] when `path` is not a checkout, and whatever Git reported
/// that is not a missing HEAD.
pub fn one(path: &Path) -> Result<Inspected> {
    let git = Git::open(path)?;
    let branch = git.current_branch()?.unwrap_or_else(|| String::from("detached"));
    let origin = git.remote_url("origin").ok().flatten();
    let unpushed = unpushed_of(&git)?;
    let dirty = dirty_of(&git)?;
    let ignored = git.ignored_directories().unwrap_or_default();
    let measured = measure(path, &ignored);
    let committed = committed_of(&git)?;
    Ok(Inspected {
        row: CloneRow {
            path: path.to_path_buf(),
            branch,
            origin,
            unpushed,
            dirty,
            bytes: measured.bytes,
            partial: !measured.complete,
            ignored: top_ignored(&ignored, &measured.buckets),
            committed,
        },
        entries: measured.entries,
    })
}

/// Commits of HEAD no remote-tracking ref has. No HEAD is no unpushed commit.
fn unpushed_of(git: &Git) -> Result<usize> {
    match git.remote_containment("HEAD") {
        Ok(containment) => Ok(containment.unpushed.len()),
        Err(Error::Git { .. }) => Ok(0),
        Err(error) => Err(error),
    }
}

/// Paths a commit would capture. A status Git cannot read is none.
fn dirty_of(git: &Git) -> Result<usize> {
    match git.status() {
        Ok(status) => Ok(status.uncommitted().count()),
        Err(Error::Git { .. }) => Ok(0),
        Err(error) => Err(error),
    }
}

/// When HEAD was committed.
fn committed_of(git: &Git) -> Result<Option<Timestamp>> {
    match git.head_committed()? {
        Some(seconds) => Timestamp::from_unix_seconds(seconds).map(Some),
        None => Ok(None),
    }
}

/// One size walk: the clone as a whole, and each ignored directory.
struct Measured {
    bytes: u64,
    entries: u64,
    complete: bool,
    buckets: Vec<u64>,
}

/// Walk `root` once. File bytes go to the total and to the ignored prefix they sit in.
fn measure(root: &Path, prefixes: &[PathBuf]) -> Measured {
    let mut measured =
        Measured { bytes: 0, entries: 0, complete: true, buckets: vec![0; prefixes.len()] };
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            measured.complete = false;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                measured.complete = false;
                continue;
            };
            measured.entries += 1;
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                measured.complete = false;
                continue;
            };
            if !metadata.is_dir() {
                measured.bytes += metadata.len();
                add(&path, root, prefixes, &mut measured.buckets, metadata.len());
            }
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                queue.push(path);
            }
        }
    }
    measured
}

/// Add `bytes` to the ignored prefix `path` sits in, when it sits in one.
fn add(path: &Path, root: &Path, prefixes: &[PathBuf], buckets: &mut [u64], bytes: u64) {
    let Ok(relative) = path.strip_prefix(root) else {
        return;
    };
    for (index, prefix) in prefixes.iter().enumerate() {
        if relative == prefix || relative.starts_with(prefix) {
            buckets[index] += bytes;
            return;
        }
    }
}

/// The three largest ignored directories, largest first, zeros dropped.
fn top_ignored(prefixes: &[PathBuf], buckets: &[u64]) -> Vec<IgnoredDir> {
    let mut dirs: Vec<IgnoredDir> = prefixes
        .iter()
        .zip(buckets)
        .filter(|(_, bytes)| **bytes > 0)
        .map(|(path, bytes)| IgnoredDir::new(path.display().to_string(), *bytes))
        .collect();
    dirs.sort_by(|left, right| {
        right.bytes.cmp(&left.bytes).then_with(|| left.path.cmp(&right.path))
    });
    dirs.truncate(TOP);
    dirs
}
