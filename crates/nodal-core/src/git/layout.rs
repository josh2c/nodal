//! Where a repository keeps its own files, read without starting a process.
//!
//! Two readings need this and neither of them may cost a `git` invocation. The refs
//! stamp ([`crate::context::refresh`]) is taken on every home of a project on every
//! command, and the age of a ref ([`super::refs::last_moved`]) is taken on a checkout a
//! person has not agreed to anything in yet. So the layout is read from the files Git
//! itself writes, and a repository this cannot make sense of answers `None` rather than
//! guessing.
//!
//! Git writes the layout in two places. `.git` is a directory in an ordinary clone and a
//! file holding a `gitdir:` line in a linked worktree, and a linked worktree's git
//! directory holds a `commondir` file naming the directory it shares with the checkout
//! it was made from. Refs and their logs live in the shared one, which is why both
//! readings are here and not one.

use std::path::{Path, PathBuf};

/// The file a linked worktree's git directory names the shared directory in.
const COMMON: &str = "commondir";

/// The line a linked worktree's `.git` file names its git directory on.
const GITDIR: &str = "gitdir:";

/// Where a repository keeps the files of the checkout at `path`.
///
/// `None` when the path is not one this can find a git directory for. That is not an
/// error: a checkout a person moved or deleted is an ordinary thing, and every caller
/// here has an answer for a reading it could not take.
#[must_use]
pub fn dir(path: &Path) -> Option<PathBuf> {
    let dot = path.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    if dot.is_file() {
        let text = std::fs::read_to_string(&dot).ok()?;
        let pointed = text.strip_prefix(GITDIR)?.trim();
        return Some(path.join(pointed));
    }
    path.join("HEAD").is_file().then(|| path.to_path_buf())
}

/// The directory a checkout shares with every worktree of its repository.
///
/// The git directory itself for an ordinary clone, and the directory its `commondir`
/// names for a linked worktree. This is where refs and their logs are, so a reading of
/// either taken in a worktree and a reading taken in the checkout are one reading.
#[must_use]
pub fn common_dir(path: &Path) -> Option<PathBuf> {
    let git_dir = dir(path)?;
    let Ok(named) = std::fs::read_to_string(git_dir.join(COMMON)) else {
        return Some(git_dir);
    };
    let pointed = Path::new(named.trim());
    if pointed.as_os_str().is_empty() {
        return Some(git_dir);
    }
    Some(if pointed.is_absolute() { pointed.to_path_buf() } else { git_dir.join(pointed) })
}
