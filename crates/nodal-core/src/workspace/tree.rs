//! The one tree copier every backend shares.
//!
//! A backend decides one thing: how the bytes get to the other end, for one file or
//! for one whole directory. Everything else about a clone — which entries it holds,
//! the order they are made in, links that must stay one file, symbolic links,
//! permissions, extended attributes and timestamps — is here, so no two backends can
//! disagree about what a clone is.
//!
//! Four rules are worth stating, because they are what makes a copy behave like the
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
//!
//! ## Why the copy runs on more than one thread
//!
//! A clone costs metadata and nothing else, so one thread spends the whole clone
//! waiting on the filesystem rather than working. `ci/measure-materialize.sh` is where
//! that is read: over two hundred thousand files, one thread took 3.3 s and eight took
//! 1.4 s, against 3.1 s for `cp -a --reflink=always`, which makes the same calls. So the
//! copier hands directories to a small pool of workers ([`workers`] says how many) and
//! each worker makes the entries of one directory.
//!
//! Three things keep the result the same tree at every worker count.
//!
//! * Every directory is made first, on this thread, parents before children. A worker
//!   never creates the directory another worker is writing into.
//! * Which name of a hard-linked file holds the copy, and which names are links to it,
//!   is decided before any worker starts, from the order of the walk. The workers then
//!   run in two passes: the first makes every copy, the second makes every link to one.
//!   So a worker never waits for a file another worker owes it, and two workers can
//!   never each hold what the other is waiting for.
//! * Directory permissions and times go on afterwards, deepest first, on this thread,
//!   once every worker has finished.
//!
//! A failure is reported at the same place twice, as it was on one thread: a worker
//! that fails stops the others, and the error kept is the one from the directory
//! earliest in the walk.

use std::collections::HashMap;
use std::collections::hash_map::Entry as Slot;
use std::fs::Metadata;
use std::ops::Range;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::exclude::Excludes;
use super::walk::{Entry, Kind, walk};
use super::{Report, meta, xattr};
use crate::error::{Error, Result};

/// The permission a mode grants its owner to write the file. A mode without it is
/// what makes a copy something its own maker cannot finish.
const OWNER_WRITE: u32 = 0o200;

/// The variable that says how many workers a clone runs on.
///
/// It is here for measurement and for a machine whose filesystem answers better at
/// another number. A person who sets nothing gets [`workers`].
pub const WORKERS_VAR: &str = "NODAL_MATERIALIZE_WORKERS";

/// The most workers a clone starts without being told to.
///
/// Measured on a twenty-eight core machine, the curve is flat from eight workers on:
/// the filesystem, not the processor, is what a clone waits for. A ceiling also keeps a
/// create off every core of a machine that is running the work the unit is for.
const WORKER_CEILING: usize = 8;

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

/// How many workers a clone runs on when nobody says otherwise.
///
/// [`WORKERS_VAR`] answers when it holds a whole number above zero. Otherwise the
/// answer is the core count of this machine, at most [`WORKER_CEILING`]. It is never
/// zero, so a caller can use it as it comes.
#[must_use]
pub fn workers() -> usize {
    if let Some(asked) = requested() {
        return asked;
    }
    std::thread::available_parallelism().map_or(1, |cores| cores.get().min(WORKER_CEILING))
}

/// The worker count [`WORKERS_VAR`] asks for, and `None` where it says nothing a count
/// can be read out of.
fn requested() -> Option<usize> {
    let asked = std::env::var_os(WORKERS_VAR)?;
    asked.to_str()?.trim().parse::<usize>().ok().filter(|count| *count > 0)
}

/// Copy the tree at `source` into `destination`, leaving out what `exclude` names, on
/// `workers` workers. A count below one is read as one; [`workers`] is what a caller
/// with no reason to choose passes.
///
/// `destination` must not exist, and must not be inside `source` or hold it: a copy
/// into its own source has no end, and a project that lets it happen fills a disk with
/// checkouts inside checkouts.
///
/// The result does not depend on the worker count: the same tree, byte for byte, with
/// the same report, at every number of workers. That is what
/// `tests/safety/tests/clone_identity.rs` asserts, and it is why the count is an
/// argument a test can pass rather than a variable it must set.
///
/// # Errors
/// [`Error::MaterializeDestination`] when the destination cannot be used, [`Error::Io`]
/// when an entry could not be read or written, and whatever `ops` returns.
pub fn materialize(
    source: &Path,
    destination: &Path,
    exclude: &Excludes,
    ops: Ops,
    workers: usize,
) -> Result<Report> {
    check(source, destination)?;
    let (entries, skipped) = walk(source, exclude)?;
    std::fs::create_dir_all(destination).map_err(Error::io(destination))?;
    let copier = Copier::new(source, destination, ops, &entries);
    let mut report = Report { excluded: skipped.excluded, ..Report::default() };
    merge(&mut report, &copier.directories()?);
    merge(&mut report, &copier.contents(workers.max(1))?);
    copier.finish()?;
    Ok(report)
}

/// One clone as it is made: where it comes from, where it goes, and what the walk
/// found.
///
/// Nothing here changes once it is built, so every worker reads one copier.
struct Copier<'a> {
    /// The tree being copied.
    source: &'a Path,
    /// The tree being made.
    destination: &'a Path,
    /// How this backend puts an entry across.
    ops: Ops,
    /// Every entry of the source the exclusion list kept, parents first.
    entries: &'a [Entry],
    /// For each entry that is a second name for a file, the entry that holds the copy
    /// it must be a hard link to. Decided from the walk, before any worker starts.
    followers: HashMap<usize, usize>,
}

impl<'a> Copier<'a> {
    /// A copier that has done nothing yet.
    fn new(source: &'a Path, destination: &'a Path, ops: Ops, entries: &'a [Entry]) -> Self {
        Self { source, destination, ops, entries, followers: followers(entries) }
    }

    /// Make every directory, parents before children, and count them.
    ///
    /// This is the whole reason a worker never has to make one: the tree of directories
    /// is there before any file is put in it.
    fn directories(&self) -> Result<Report> {
        let mut report = Report::default();
        for entry in self.entries.iter().filter(|entry| entry.kind == Kind::Directory) {
            let to = entry.under(self.destination);
            std::fs::create_dir_all(&to).map_err(Error::io(&to))?;
            report.directories += 1;
        }
        Ok(report)
    }

    /// Put every file and every symbolic link across, on `workers` workers.
    ///
    /// Two passes over the same directories. The first makes every copy; the second
    /// makes every name that is a hard link to one, and runs only where the source has
    /// such a name.
    fn contents(&self, workers: usize) -> Result<Report> {
        let groups = self.groups();
        let mut report = run(workers, &groups, |group| self.copies(group.clone()))?;
        if self.followers.is_empty() {
            return Ok(report);
        }
        let linking: Vec<Range<usize>> =
            groups.into_iter().filter(|group| self.holds_a_link(group)).collect();
        merge(&mut report, &run(workers, &linking, |group| self.links(group.clone()))?);
        Ok(report)
    }

    /// The walk cut into one range per directory, which is the unit of work a worker
    /// claims.
    ///
    /// A walk reads one directory at a time and appends what it holds, so the entries
    /// of one directory are next to each other. A range therefore names the contents of
    /// one directory, and two workers never write into one directory at once.
    fn groups(&self) -> Vec<Range<usize>> {
        let mut groups: Vec<Range<usize>> = Vec::new();
        let mut parent: Option<&Path> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            let holder = entry.relative.parent();
            match groups.last_mut() {
                Some(group) if parent == holder => group.end = index + 1,
                _ => {
                    groups.push(index..index + 1);
                    parent = holder;
                }
            }
        }
        groups
    }

    /// Whether any entry in `group` is a second name for a file.
    fn holds_a_link(&self, group: &Range<usize>) -> bool {
        group.clone().any(|index| self.followers.contains_key(&index))
    }

    /// Copy every file and recreate every symbolic link in one directory.
    ///
    /// A name that is a hard link to another copy is left for [`Copier::links`], which
    /// runs once every copy is there.
    fn copies(&self, group: Range<usize>) -> Result<Report> {
        let mut report = Report::default();
        for index in group {
            let Some(entry) = self.entries.get(index) else { continue };
            if self.followers.contains_key(&index) {
                continue;
            }
            let (from, to) = (entry.under(self.source), entry.under(self.destination));
            match entry.kind {
                Kind::Symlink => put_symlink(entry, &from, &to, &mut report)?,
                Kind::File => self.put_file(entry, &from, &to, &mut report)?,
                Kind::Directory | Kind::Other => {}
            }
        }
        Ok(report)
    }

    /// Make every name in one directory that is a second name for a file already
    /// copied.
    fn links(&self, group: Range<usize>) -> Result<Report> {
        let mut report = Report::default();
        for index in group {
            let (Some(first), Some(entry)) = (self.followers.get(&index), self.entries.get(index))
            else {
                continue;
            };
            let Some(made) = self.entries.get(*first) else { continue };
            let (from, to) = (made.under(self.destination), entry.under(self.destination));
            std::fs::hard_link(&from, &to).map_err(Error::io(&to))?;
            report.hardlinks += 1;
        }
        Ok(report)
    }

    /// Put one regular file across, with the metadata of the source on it.
    fn put_file(&self, entry: &Entry, from: &Path, to: &Path, report: &mut Report) -> Result<()> {
        let put = (self.ops.file)(from, to, &entry.metadata)?;
        report.bytes += put.bytes;
        if !put.shared {
            report.copied += 1;
        }
        report.files += 1;
        report.attributes += attributes(entry, from, to)?;
        meta::permissions(to, &entry.metadata)?;
        meta::times(to, &entry.metadata)
    }

    /// Give the root and every directory the copier made the permissions and times of
    /// its source, deepest first, once everything inside it is there.
    fn finish(&self) -> Result<()> {
        let made = self.entries.iter().rev().filter(|entry| entry.kind == Kind::Directory);
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

/// Recreate one symbolic link. The link is copied, never what it points at.
///
/// # Errors
/// [`Error::Io`] when the link could not be read or made.
fn put_symlink(entry: &Entry, from: &Path, to: &Path, report: &mut Report) -> Result<()> {
    let target = std::fs::read_link(from).map_err(Error::io(from))?;
    std::os::unix::fs::symlink(&target, to).map_err(Error::io(to))?;
    meta::times(to, &entry.metadata)?;
    report.symlinks += 1;
    Ok(())
}

/// For each entry that is a second name for a file, the entry that holds the copy.
///
/// The first name the walk reports for one inode holds the copy, and every later name
/// is a hard link to it. The walk has one order, so this answer is the same at every
/// worker count, and it is what makes a parallel copy hold the same links as a copy on
/// one thread.
fn followers(entries: &[Entry]) -> HashMap<usize, usize> {
    let mut first: HashMap<(u64, u64), usize> = HashMap::new();
    let mut followers = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.kind != Kind::File || entry.metadata.nlink() < 2 {
            continue;
        }
        match first.entry((entry.metadata.dev(), entry.metadata.ino())) {
            Slot::Occupied(made) => {
                followers.insert(index, *made.get());
            }
            Slot::Vacant(empty) => {
                empty.insert(index);
            }
        }
    }
    followers
}

/// What one worker got through, and the first thing that stopped it.
#[derive(Default)]
struct Done {
    /// What its own share of the work put across.
    report: Report,
    /// The work item it failed on, and why.
    failure: Option<(usize, Error)>,
}

/// Run `each` over every item of `work`, on `workers` workers, and add up what they
/// did.
///
/// One worker means no thread is started at all, so a small copy pays nothing for the
/// pool and a test at one worker measures one thread.
///
/// # Errors
/// The error of the earliest work item that failed, which is the error a copy on one
/// thread would have reported.
fn run<T: Sync>(
    workers: usize,
    work: &[T],
    each: impl Fn(&T) -> Result<Report> + Sync,
) -> Result<Report> {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let count = workers.clamp(1, work.len().max(1));
    if count == 1 {
        return gather(vec![claim(work, &next, &stop, &each)]);
    }
    let mut all = Vec::with_capacity(count);
    std::thread::scope(|scope| {
        let threads: Vec<_> =
            (0..count).map(|_| scope.spawn(|| claim(work, &next, &stop, &each))).collect();
        for thread in threads {
            match thread.join() {
                Ok(done) => all.push(done),
                Err(panic) => std::panic::resume_unwind(panic),
            }
        }
    });
    gather(all)
}

/// Take work items until there are none left or another worker has failed.
fn claim<T>(
    work: &[T],
    next: &AtomicUsize,
    stop: &AtomicBool,
    each: &(impl Fn(&T) -> Result<Report> + Sync),
) -> Done {
    let mut done = Done::default();
    while !stop.load(Ordering::Relaxed) {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(item) = work.get(index) else { break };
        match each(item) {
            Ok(report) => merge(&mut done.report, &report),
            Err(error) => {
                stop.store(true, Ordering::Relaxed);
                done.failure = Some((index, error));
            }
        }
    }
    done
}

/// One report out of every worker's, and the failure from the earliest work item.
///
/// The earliest one is what a copy on one thread would have reported, so a tree that
/// cannot be copied names the same entry however many workers read it.
fn gather(all: Vec<Done>) -> Result<Report> {
    let mut report = Report::default();
    let mut failure: Option<(usize, Error)> = None;
    for done in all {
        merge(&mut report, &done.report);
        if let Some((index, error)) = done.failure
            && failure.as_ref().is_none_or(|(first, _)| index < *first)
        {
            failure = Some((index, error));
        }
    }
    failure.map_or(Ok(report), |(_, error)| Err(error))
}

/// Add what one worker put across to the whole clone's report.
fn merge(into: &mut Report, from: &Report) {
    into.files += from.files;
    into.directories += from.directories;
    into.symlinks += from.symlinks;
    into.hardlinks += from.hardlinks;
    into.attributes += from.attributes;
    into.excluded += from.excluded;
    into.copied += from.copied;
    into.bytes += from.bytes;
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
