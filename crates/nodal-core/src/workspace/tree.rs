//! The one tree copier every backend shares.
//!
//! A backend decides one thing: how the bytes get to the other end, for one file or
//! for one whole directory. Everything else about a clone — which entries it holds,
//! the order they are made in, links that must stay one file, symbolic links,
//! permissions, extended attributes and timestamps — is here, so no two backends can
//! disagree about what a clone is.
//!
//! Three rules are worth stating, because they are what makes a copy behave like the
//! tree it came from.
//!
//! * Every entry is made here, one at a time. A backend that can copy a whole
//!   directory in one call does not get to, because a call that makes the tree itself
//!   makes a different tree: it has no way to know that two names in it are one file.
//! * Files that share an inode in the source share one in the copy, found by device
//!   and inode number, so a dependency tree of linked files stays one file instead of
//!   becoming many.
//! * Directory permissions and times are applied after everything inside them, because
//!   writing a child changes both.
//! * A file's mode is the last thing put on it, because most of a base is content its
//!   own owner may not write and every other piece of metadata is written through the
//!   mode. A copy given the mode of its source first is a copy the copier cannot
//!   finish.

use std::collections::HashMap;
use std::collections::hash_map::Entry as Slot;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::exclude::Excludes;
use super::walk::{Entry, Kind, walk};
use super::{Report, meta, xattr};
use crate::error::{Error, Result};

/// The permission a mode grants its owner to write the file. A mode without it is
/// what makes a copy something its own maker cannot finish.
const OWNER_WRITE: u32 = 0o200;

/// What one call of a backend put across.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Put {
    /// How many bytes the entry holds, as the source counts them.
    pub bytes: u64,
    /// Whether the copy shares its blocks with the source. `false` means the bytes
    /// were copied, and the report says so.
    pub shared: bool,
}

/// How a backend puts one regular file at `destination`. The copier adds the metadata
/// afterwards.
pub type PutFile = fn(source: &Path, destination: &Path, metadata: &Metadata) -> Result<Put>;

/// What a backend can do: put one regular file across. Everything else about a clone
/// is the copier's, so that no two backends can make a different tree.
#[derive(Debug, Clone, Copy)]
pub struct Ops {
    /// Puts one regular file across.
    pub file: PutFile,
}

/// Copy the tree at `source` into `destination`, leaving out what `exclude` names.
///
/// `destination` must not exist, and must not be inside `source` or hold it: a copy
/// into its own source has no end, and a project that lets it happen fills a disk with
/// checkouts inside checkouts.
///
/// # Errors
/// [`Error::MaterializeDestination`] when the destination cannot be used, [`Error::Io`]
/// when an entry could not be read or written, and whatever `ops` returns.
pub fn materialize(
    source: &Path,
    destination: &Path,
    exclude: &Excludes,
    ops: Ops,
) -> Result<Report> {
    check(source, destination)?;
    let (entries, skipped) = walk(source, exclude)?;
    std::fs::create_dir_all(destination).map_err(Error::io(destination))?;
    let mut copier = Copier::new(source, destination, ops, skipped.excluded);
    for entry in &entries {
        copier.put(entry)?;
    }
    copier.finish(&entries)?;
    Ok(copier.report)
}

/// One clone as it is made: where it comes from, where it goes, and what it holds so
/// far.
struct Copier<'a> {
    /// The tree being copied.
    source: &'a Path,
    /// The tree being made.
    destination: &'a Path,
    /// How this backend puts an entry across.
    ops: Ops,
    /// What the clone holds, as it is built.
    report: Report,
    /// The first copy of each source inode that more than one name points at.
    links: HashMap<(u64, u64), PathBuf>,
}

impl<'a> Copier<'a> {
    /// A copier that has done nothing yet, in a walk that left `excluded` entries out.
    fn new(source: &'a Path, destination: &'a Path, ops: Ops, excluded: usize) -> Self {
        let report = Report { excluded, ..Report::default() };
        Self { source, destination, ops, report, links: HashMap::new() }
    }

    /// Put one entry at its place in the clone.
    fn put(&mut self, entry: &Entry) -> Result<()> {
        let (from, to) = (entry.under(self.source), entry.under(self.destination));
        match entry.kind {
            Kind::Directory => self.put_directory(&to)?,
            Kind::Symlink => self.put_symlink(entry, &from, &to)?,
            Kind::File => self.put_file(entry, &from, &to)?,
            Kind::Other => {}
        }
        Ok(())
    }

    /// Make one directory. The walk then fills it, and [`Copier::finish`] gives it the
    /// permissions and times of its source once everything inside it is there.
    fn put_directory(&mut self, to: &Path) -> Result<()> {
        std::fs::create_dir_all(to).map_err(Error::io(to))?;
        self.report.directories += 1;
        Ok(())
    }

    /// Recreate one symbolic link. The link is copied, never what it points at.
    fn put_symlink(&mut self, entry: &Entry, from: &Path, to: &Path) -> Result<()> {
        let target = std::fs::read_link(from).map_err(Error::io(from))?;
        std::os::unix::fs::symlink(&target, to).map_err(Error::io(to))?;
        meta::times(to, &entry.metadata)?;
        self.report.symlinks += 1;
        Ok(())
    }

    /// Put one regular file: a hard link to a copy already made, or a new copy with
    /// the metadata of the source on it.
    fn put_file(&mut self, entry: &Entry, from: &Path, to: &Path) -> Result<()> {
        if let Some(first) = self.linked(entry, to) {
            std::fs::hard_link(&first, to).map_err(Error::io(to))?;
            self.report.hardlinks += 1;
            return Ok(());
        }
        let put = (self.ops.file)(from, to, &entry.metadata)?;
        self.count(put);
        self.report.files += 1;
        self.report.attributes += attributes(entry, from, to)?;
        meta::permissions(to, &entry.metadata)?;
        meta::times(to, &entry.metadata)
    }

    /// Add what one call put across to the report.
    fn count(&mut self, put: Put) {
        self.report.bytes += put.bytes;
        if !put.shared {
            self.report.copied += 1;
        }
    }

    /// The copy this entry must be a hard link to, when the source has one name for
    /// this inode already. Records `to` as that copy the first time.
    fn linked(&mut self, entry: &Entry, to: &Path) -> Option<PathBuf> {
        if entry.metadata.nlink() < 2 {
            return None;
        }
        match self.links.entry((entry.metadata.dev(), entry.metadata.ino())) {
            Slot::Occupied(first) => Some(first.get().clone()),
            Slot::Vacant(empty) => {
                empty.insert(to.to_path_buf());
                None
            }
        }
    }

    /// Give the root and every directory the copier made the permissions and times of
    /// its source, deepest first, once everything inside it is there.
    fn finish(&self, entries: &[Entry]) -> Result<()> {
        let made = entries.iter().rev().filter(|entry| entry.kind == Kind::Directory);
        for entry in made {
            let to = entry.under(self.destination);
            meta::permissions(&to, &entry.metadata)?;
            meta::times(&to, &entry.metadata)?;
        }
        let root = std::fs::symlink_metadata(self.source).map_err(Error::io(self.source))?;
        xattr::copy(self.source, self.destination)?;
        meta::permissions(self.destination, &root)
    }
}

/// Carry the extended attributes of one file onto its copy, and report how many.
///
/// Writing an attribute needs the write permission the mode grants, and the owner is
/// not excused from the rule. A base is full of files that grant it to nobody: git
/// writes every loose object and every pack file `0444`, and macOS puts an attribute of
/// its own on each of them. The copy of such a file is read-only as soon as it exists,
/// because a backend gives it the mode of its source, so an attribute written
/// afterwards is refused and the whole clone stops.
///
/// So the copy is opened for as long as its attributes are written, and
/// [`Copier::put_file`] gives it the mode of its source afterwards, which is the last
/// thing done to it. A source that already grants its owner a write needs none of this
/// and pays nothing for it.
///
/// # Errors
/// [`Error::Io`] when the copy could not be opened, or an attribute not read or written.
fn attributes(entry: &Entry, from: &Path, to: &Path) -> Result<usize> {
    if entry.metadata.mode() & OWNER_WRITE == 0 {
        meta::writable(to, &entry.metadata)?;
    }
    xattr::copy(from, to)
}

/// Refuse a destination a clone cannot be made at.
fn check(source: &Path, destination: &Path) -> Result<()> {
    let refuse = |why: &'static str| {
        Err(Error::MaterializeDestination { destination: destination.to_path_buf(), why })
    };
    if destination.symlink_metadata().is_ok() {
        return refuse("it is already there");
    }
    if destination.starts_with(source) {
        return refuse("it is inside the tree being cloned");
    }
    if source.starts_with(destination) {
        return refuse("it holds the tree being cloned");
    }
    Ok(())
}
