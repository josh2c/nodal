//! One clone: branch, what it is the only copy of, dirty paths, size, ignored, age.
//!
//! This reads one clone and states what it found. It does not conclude that a clone is
//! safe to delete; that conclusion needs the other clones on the machine, so it is drawn
//! in [`super::unique`] once the group is known. What is read here is one clone's
//! reading ([`CloneReading`]):
//! every ref tip, the remote-tracking refs, when the clone last heard from a remote, and
//! whether it fetches every branch.
//!
//! The walk that sizes the clone also sizes ignored directories, so a tree is read once.
//! Files with more than one link are recorded as well as counted, because Cargo hardlinks
//! one file into many directories and a group total that added them up would be over.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use crate::doctor::unique::{CloneReading, RemoteTip};
use crate::git::{Git, Oid};
use crate::model::Timestamp;
use crate::output::view::machine::{CloneRow, IgnoredDir};
use crate::{Error, Result};

/// How many ignored directories a row keeps.
const TOP: usize = 3;

/// Where a repository keeps its reading of any remote at all.
///
/// The wider prefix [`REMOTES`] sits inside, and the two answer different questions. That
/// one asks what this clone last saw of `origin`. This one asks which refs are a reading
/// of somewhere else, whoever the somewhere is, so that what is left over is what the
/// repository holds of its own accord.
const TRACKING: &str = "refs/remotes/";

/// The ref under `refs/remotes/<remote>/` that names a default branch rather than one.
const HEAD: &str = "HEAD";

/// Files seen already in this survey, by device and inode, so a hardlinked file is
/// counted once towards a group even though every clone holding it has it.
#[derive(Debug, Default)]
pub struct Links(BTreeSet<(u64, u64)>);

/// What inspecting one clone produced.
#[derive(Debug)]
pub struct Inspected {
    /// The row.
    pub row: CloneRow,
    /// What the clone can say about what else holds its commits.
    pub reading: CloneReading,
    /// Directory entries the size walk looked at.
    pub entries: u64,
}

/// Read one clone. A failure is the clone, not the rest of the survey.
///
/// # Errors
/// [`Error::NotARepository`] when `path` is not a checkout, and whatever Git reported
/// that is not a missing HEAD.
pub fn one(path: &Path, links: &mut Links) -> Result<Inspected> {
    let git = Git::open(path)?;
    let branch = git.current_branch()?;
    let origin = git.remote_url("origin").ok().flatten();
    let reading = reading_of(&git, path, branch.as_deref());
    let dirty = dirty_of(&git)?;
    let ignored = git.ignored_directories().unwrap_or_default();
    let measured = measure(path, &ignored, links);
    let committed = committed_of(&git)?;
    Ok(Inspected {
        row: CloneRow {
            path: path.to_path_buf(),
            branch: branch.unwrap_or_else(|| String::from("detached")),
            origin,
            unpushed: None,
            witnesses: Vec::new(),
            only_copy: None,
            unchecked: None,
            dirty,
            bytes: measured.bytes,
            repeated: measured.repeated,
            partial: !measured.complete,
            ignored: top_ignored(&ignored, &measured.buckets, &measured.shared),
            committed,
        },
        reading,
        entries: measured.entries,
    })
}

/// Everything one repository at `path` can say about where its commits also live.
///
/// The same reading [`one`] takes, for a caller that wants the clone's reading and none
/// of the
/// rest of a row. A destructive check reads a home and the checkout it belongs to this
/// way, so that the survey and the check read a repository with one pair of eyes.
///
/// A path Git will not read is no evidence and is not clean either: `unreadable` says
/// so, and every proof reports it as not checked.
#[must_use]
pub fn reading(path: &Path) -> CloneReading {
    let git = match Git::open(path) {
        Ok(git) => git,
        Err(error) => {
            return CloneReading { unreadable: Some(error.to_string()), ..CloneReading::default() };
        }
    };
    let branch = git.current_branch().unwrap_or_default();
    reading_of(&git, path, branch.as_deref())
}

/// Everything this clone can say about where its commits also live.
///
/// A clone Git will not read is no evidence and is not clean either: `unreadable` says
/// so, and the proof reports it as not checked.
fn reading_of(git: &Git, path: &Path, branch: Option<&str>) -> CloneReading {
    let tips = match git.all_refs() {
        Ok(refs) => refs,
        Err(error) => {
            return CloneReading { unreadable: Some(error.to_string()), ..CloneReading::default() };
        }
    };
    let head = match head_of(git, branch, &tips) {
        Ok(head) => head,
        Err(error) => {
            return CloneReading { unreadable: Some(error.to_string()), ..CloneReading::default() };
        }
    };
    let refspecs = git.fetch_refspecs("origin").unwrap_or_default();
    CloneReading {
        head,
        remotes: remote_tips(&tips),
        own: own_tips(&tips),
        tips: tips.into_iter().map(|reference| reference.oid).collect(),
        heard: super::unique::heard(path),
        complete: super::unique::complete(&refspecs),
        shallow: path.join(".git/shallow").exists(),
        unreadable: None,
    }
}

/// The tips of the refs this repository holds of its own accord: its branches, its tags,
/// its stash.
///
/// Everything under [`TRACKING`] is left out. Those are a reading of somewhere else, and
/// what this answers is what the repository has to say for itself.
///
/// It is taken here rather than asked for later because here is where the names are. The
/// same `git for-each-ref` that fills `tips` knows which of them are remote-tracking, and
/// a caller that wanted the split afterwards had a list of commits with the names already
/// dropped — so it ran `for-each-ref` a second time to read them back.
fn own_tips(tips: &[crate::git::refs::Ref]) -> Vec<Oid> {
    tips.iter().filter(|tip| !tip.name.starts_with(TRACKING)).map(|tip| tip.oid.clone()).collect()
}

/// The commit HEAD names. A branch's tip is already in hand; a detached HEAD is asked.
fn head_of(git: &Git, branch: Option<&str>, tips: &[crate::git::refs::Ref]) -> Result<Option<Oid>> {
    if let Some(branch) = branch {
        let full = format!("refs/heads/{branch}");
        return Ok(tips.iter().find(|tip| tip.name == full).map(|tip| tip.oid.clone()));
    }
    match git.rev_parse(HEAD) {
        Ok(oid) => Ok(Some(oid)),
        Err(Error::Git { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What this clone last saw of `origin`, branch by branch.
///
/// One maker for this reading, over in [`super::unique::tracked_in`], because three
/// callers ask it and a second implementation is a second answer. Refs of any other remote
/// are left out: a group is the clones that share the URL of `origin`, so `origin` is the
/// remote the group's question is about, and `backup/main` is not a reading of
/// `origin/main`. They stay in `tips`, because a ref of any name keeps an object alive in
/// the store it sits in, and that is a second copy whoever wrote the ref.
fn remote_tips(tips: &[crate::git::refs::Ref]) -> Vec<RemoteTip> {
    super::unique::tracked_in(tips)
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
///
/// A commit Git will not read has no date, and that is not a reason to drop the clone
/// from the survey. The row that stays is what carries "not checked" to the report; a
/// clone that failed its way out of the list would be a clone nobody is warned about.
fn committed_of(git: &Git) -> Result<Option<Timestamp>> {
    match git.head_committed() {
        Ok(Some(seconds)) => Timestamp::from_unix_seconds(seconds).map(Some),
        Ok(None) | Err(Error::Git { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// One size walk: the clone as a whole, and each ignored directory.
struct Measured {
    bytes: u64,
    repeated: u64,
    entries: u64,
    complete: bool,
    buckets: Vec<u64>,
    shared: Vec<u64>,
}

/// Walk `root` once. File bytes go to the total and to the ignored prefix they sit in.
fn measure(root: &Path, prefixes: &[PathBuf], links: &mut Links) -> Measured {
    let mut measured = Measured {
        bytes: 0,
        repeated: 0,
        entries: 0,
        complete: true,
        buckets: vec![0; prefixes.len()],
        shared: vec![0; prefixes.len()],
    };
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
                let repeat = links.repeat(&metadata);
                if repeat {
                    measured.repeated += metadata.len();
                }
                if let Some(index) = prefix_of(&path, root, prefixes) {
                    measured.buckets[index] += metadata.len();
                    if repeat {
                        measured.shared[index] += metadata.len();
                    }
                }
            }
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                queue.push(path);
            }
        }
    }
    measured
}

impl Links {
    /// Whether this file's bytes were already counted for another path in this survey.
    ///
    /// A file with one link cannot be anywhere else, so it is never recorded: the set
    /// holds only what Cargo and `git clone --local` share between directories.
    fn repeat(&mut self, metadata: &fs::Metadata) -> bool {
        metadata.nlink() > 1 && !self.0.insert((metadata.dev(), metadata.ino()))
    }
}

/// Which ignored prefix `path` sits in, when it sits in one.
fn prefix_of(path: &Path, root: &Path, prefixes: &[PathBuf]) -> Option<usize> {
    let relative = path.strip_prefix(root).ok()?;
    prefixes.iter().position(|prefix| relative == prefix || relative.starts_with(prefix))
}

/// The three largest ignored directories, largest first, zeros dropped.
fn top_ignored(prefixes: &[PathBuf], buckets: &[u64], shared: &[u64]) -> Vec<IgnoredDir> {
    let mut dirs: Vec<IgnoredDir> = prefixes
        .iter()
        .zip(buckets)
        .zip(shared)
        .filter(|((_, bytes), _)| **bytes > 0)
        .map(|((path, bytes), repeated)| {
            IgnoredDir::shared(path.display().to_string(), *bytes, *repeated)
        })
        .collect();
    dirs.sort_by(|left, right| {
        right.bytes.cmp(&left.bytes).then_with(|| left.path.cmp(&right.path))
    });
    dirs.truncate(TOP);
    dirs
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use super::remote_tips;
    use crate::git::Oid;
    use crate::git::refs::Ref;

    fn reference(name: &str, seed: u8) -> Ref {
        Ref {
            name: name.to_owned(),
            oid: Oid::parse(&format!("{seed:02x}").repeat(20)).expect("a well formed id"),
        }
    }

    /// A branch with a slash in it keeps every part after the remote.
    #[test]
    fn a_remote_ref_gives_up_its_remote_and_keeps_its_branch() {
        let tips = [reference("refs/remotes/origin/nodal/doctor", 1)];
        let remotes = remote_tips(&tips);
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].branch, "nodal/doctor");
    }

    /// A group is the clones of one `origin`, and a second remote is a different
    /// question. `backup/main` may not answer for `origin/main`.
    #[test]
    fn a_ref_of_another_remote_says_nothing_about_origin() {
        let tips = [
            reference("refs/remotes/backup/main", 1),
            reference("refs/remotes/upstream/main", 2),
            reference("refs/remotes/origin/main", 3),
        ];
        let remotes = remote_tips(&tips);
        assert_eq!(remotes.len(), 1, "only origin is read: {remotes:?}");
        assert_eq!(remotes[0].oid, reference("refs/remotes/origin/main", 3).oid);
    }

    /// The default-branch pointer is not a branch, and counting it would be counting
    /// one branch twice.
    #[test]
    fn the_default_branch_pointer_is_not_a_branch() {
        let tips = [reference("refs/remotes/origin/HEAD", 1)];
        assert!(remote_tips(&tips).is_empty());
    }

    /// A local branch is no evidence about a remote.
    #[test]
    fn a_local_branch_is_not_a_remote_tip() {
        let tips = [reference("refs/heads/main", 1)];
        assert!(remote_tips(&tips).is_empty());
    }
}
