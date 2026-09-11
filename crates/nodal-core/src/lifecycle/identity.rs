//! Which project a directory belongs to: one answer, for the readers and the writer.
//!
//! A project used to be its checkout path. That is the right key for one person on one
//! machine and the wrong one for a host two people log in to: each of them clones the
//! repository into a directory of their own, and Nodal saw two projects, two bases, two
//! blocks of ports and two lists.
//!
//! So a project is the repository, and the checkout path is what identifies one that has
//! no repository to name — a tree that has never been pushed. [`remote_of`] reduces the
//! checkout's `origin` to the spelling every clone of it shares
//! ([`crate::git::remote::identity`]), and that is the key.
//!
//! # The order, and why it is the cheap one first
//!
//! The path is asked first and the remote second. A path match is a lookup in the
//! registry; a remote match costs one `git` call, and a command that resolves a project
//! runs on every entry into a home. Asking the remote only when the path is not recorded
//! keeps that call off the path a person takes fifty times a day.
//!
//! The two orders differ in one case: a checkout recorded under a path, later repointed
//! at another repository. That row keeps the identity it has. Re-keying every unit of a
//! project because somebody changed a remote is not a thing a command does while nobody
//! is looking, and [`crate::store::projects::set_remote_url`] refuses it as well.
//!
//! Nothing here reaches a network. `git remote get-url` reads the repository's own
//! configuration file.

use std::path::Path;

use rusqlite::Connection;

use crate::Result;
use crate::git::{Git, remote};
use crate::model::{Project, RemoteUrl};
use crate::store::projects;

/// The remote a project's identity is read from.
pub const ORIGIN: &str = "origin";

/// Which repository the checkout at `root` is a clone of.
///
/// A directory that is not a repository, a repository with no `origin`, and a URL that
/// reduces to nothing all answer `None`. A project with no answer is keyed by its path,
/// exactly as every project was before.
#[must_use]
pub fn remote_of(root: &Path) -> Option<RemoteUrl> {
    let url = Git::at(root).remote_url(ORIGIN).ok().flatten()?;
    remote::identity(&url).and_then(|text| RemoteUrl::parse(text).ok())
}

/// The recorded project for the checkout at `root`, and `None` where there is none.
///
/// This writes nothing, so every read command can use it: `nodal ls` in one engineer's
/// clone answers with the project the other engineer's clone recorded, and the list is
/// one list.
///
/// # Errors
/// Whatever the registry reports while the rows are read.
pub fn project_at(conn: &Connection, root: &Path) -> Result<Option<Project>> {
    if let Some(found) = projects::find_by_root(conn, root)? {
        return Ok(Some(found));
    }
    let Some(remote) = remote_of(root) else { return Ok(None) };
    projects::find_by_remote(conn, &remote)
}

/// The recorded project for the checkout `path` is inside, and `None` where none is.
///
/// The path walk comes first and covers every directory above `path`, because that is a
/// registry lookup and costs nothing. The remote is asked once afterwards, from `path`
/// itself: `git` searches upward for the repository, so one call answers for the whole
/// walk rather than one call per directory.
///
/// The project comes back standing in the checkout it was found through
/// ([`here`]), which is this person's clone and not the one the row records.
///
/// # Errors
/// Whatever the registry reports while the rows are read.
pub fn project_containing(conn: &Connection, path: &Path) -> Result<Option<Project>> {
    for directory in path.ancestors() {
        if let Some(project) = projects::find_by_root(conn, directory)? {
            return Ok(Some(here(project, directory)));
        }
    }
    let Some(remote) = remote_of(path) else { return Ok(None) };
    let Some(project) = projects::find_by_remote(conn, &remote)? else { return Ok(None) };
    let root = Git::at(path).top_level().unwrap_or_else(|_| path.to_path_buf());
    Ok(Some(here(project, &root)))
}

/// The same project, standing in the checkout the caller is in.
///
/// The row records where the project was first seen, which on a shared host is one
/// engineer's clone. Every operation acts on the tree the person is standing in
/// instead: handing back the stored path would let a merge fast-forward a branch in
/// somebody else's checkout, which is not a thing one account may do to another's.
#[must_use]
pub fn here(project: Project, root: &Path) -> Project {
    Project { root: root.to_path_buf(), ..project }
}
