//! What a reclaimed home loses on its way into the trash.
//!
//! A live home keeps its build output and its installed dependencies, and that is the
//! whole point of it: the clone shares blocks with the base, so a warm `target` costs
//! nothing to carry and saves a cold build. A reclaimed home keeps them for a fortnight
//! and nothing ever reads them. One built Rust unit measured here added thirteen
//! gigabytes to the trash, of which under one was work.
//!
//! So the trash holds the home without its build output and its dependencies. The rest
//! of it stays, and the difference between the two is the point of this module.
//!
//! # Two gates, and a directory has to pass both
//!
//! **An ignore rule must cover it.** The candidates come from
//! [`crate::git::Git::ignored_entries`], which is `git ls-files --others --ignored`.
//! `--others` is what makes this safe: a path any commit holds is not in the answer, so
//! nothing here can reach a tracked file, whatever its name is. A project that commits
//! a `build` directory keeps it.
//!
//! **The exclusion table must call it regenerable.** [`super::exclude::regenerable`] is
//! the list, and it is the same table that decides what a clone carries, read for the
//! one case where the answer inverts. Everything else an ignore rule covers stays: a
//! `.env.local`, a local database file, a scratch directory somebody kept notes in.
//! Those are the reason a person goes back into the trash at all, and the report names
//! them so that going back does not require guessing.
//!
//! The files Nodal itself wrote into the home are left out of that answer. A home
//! carries `.nodal/`, `.envrc`, `WORKUNIT.md` and the rest, every one of them under an
//! ignore rule Nodal added for it, and a report that listed the six of them would bury
//! the one `.env.local` a person is looking for. [`crate::env::files::WRITTEN`] is the
//! table that says which files those are, and it is the same table the uniqueness check
//! reads to decide that they are not work.
//!
//! # What a failure here does
//!
//! Nothing. Every removal that could not be made is a note and the directory is
//! reported as kept, because by the time this runs the home has already moved and the
//! step's only alternative would be to fail a reclaim over a directory nobody needs.
//! A prune that removed nothing leaves a trash entry that is exactly what it used to
//! be, which is the state every earlier version of Nodal shipped.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{exclude, remove};
use crate::doctor::size;
use crate::env::files;
use crate::git::Git;

/// How many kept paths a report names one by one. Above it, the report says how many
/// there are and how much they hold.
///
/// A person reading a reclaim wants to know whether anything of theirs is in the trash.
/// A dozen names answers that; two hundred names is a wall that answers nothing, and
/// `nodal show` is where the whole list is read.
pub const NAMED: usize = 12;

/// One directory the prune removed from a trashed home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    /// Where it was, relative to the home.
    pub path: PathBuf,
    /// What it held, as the sum of the files under it.
    pub bytes: u64,
    /// Why it could be removed, in the words the exclusion table uses.
    pub reason: String,
}

/// One path an ignore rule covers that the trash keeps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// Where it is, relative to the home.
    pub path: PathBuf,
    /// What it holds.
    pub bytes: u64,
}

/// What one prune did to one trashed home.
///
/// Journalled, because the registry write that ends a reclaim records the bytes on the
/// trash row and does not always run in the process that produced them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// What was removed, in the order it was found.
    pub removed: Vec<Removal>,
    /// The ignored paths the trash keeps, in the order they were found.
    pub kept: Vec<Kept>,
    /// How many bytes the removals dropped.
    pub bytes: u64,
    /// How many bytes the kept paths hold.
    pub kept_bytes: u64,
    /// What could not be done, and why. A note is never a failure here.
    pub notes: Vec<String>,
}

impl Report {
    /// Record a path the trash kept, whether the table never offered it or a removal of
    /// it was refused.
    fn keep(&mut self, path: PathBuf, bytes: u64) {
        self.kept_bytes += bytes;
        self.kept.push(Kept { path, bytes });
    }

    /// Whether the prune left the home as it found it.
    #[must_use]
    pub const fn changed_nothing(&self) -> bool {
        self.removed.is_empty()
    }

    /// The prune in words, for an event body and a log line.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.changed_nothing() {
            return String::from(
                "The trashed home held no build output and no installed dependencies that an \
                 ignore rule covers. Nothing was removed from it.",
            );
        }
        let list = self
            .removed
            .iter()
            .map(|removal| removal.path.display().to_string())
            .collect::<Vec<String>>()
            .join(", ");
        format!(
            "Dropped {count} {noun} from the trashed home: {list}. Each one is generated state a \
             tool writes again. The trash keeps everything else the home held.",
            count = self.removed.len(),
            noun = if self.removed.len() == 1 { "directory" } else { "directories" },
        )
    }
}

/// Remove the build output and the installed dependencies from the home at `path`, and
/// say what went and what stayed.
///
/// `path` is the home where it is now, which for a reclaim is the trash. Nothing here
/// looks at where it used to be: a prune is about what a directory holds, not about
/// where it sits.
///
/// Repeatable: a second sweep of a home whose generated state has gone finds nothing to
/// remove and removes nothing.
///
/// # Errors
/// None. A home that is not there, a directory that is not a repository, a listing that
/// failed and a removal that was refused are each one note on the report, for the reason
/// the module documentation gives.
#[must_use]
pub fn sweep(path: &Path) -> Report {
    let surveyed = survey(path);
    let mut report = Report { notes: surveyed.notes, ..Report::default() };
    for candidate in surveyed.candidates {
        let Some(reason) = candidate.reason else {
            report.keep(candidate.path, candidate.bytes);
            continue;
        };
        if let Err(why) = remove::tree(&path.join(&candidate.path)) {
            report.notes.push(why.to_string());
            report.keep(candidate.path, candidate.bytes);
            continue;
        }
        report.bytes += candidate.bytes;
        report.removed.push(Removal {
            path: candidate.path,
            bytes: candidate.bytes,
            reason: reason.to_owned(),
        });
    }
    report
}

/// One path an ignore rule covers, and what the same contract says may become of it.
///
/// The reason is the whole of the classification: `Some` is a directory the exclusion
/// table calls regenerable, so a tool writes it again from the tree that is still there;
/// `None` is state that exists nowhere else and is the reason a person opens the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Where it is, relative to the home.
    pub path: PathBuf,
    /// What it holds, as the sum of the file sizes under it.
    pub bytes: u64,
    /// Why the trash need not keep it, `None` when the table does not call it
    /// regenerable.
    pub reason: Option<&'static str>,
}

/// What the ignored state of one home is, before anything is done about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Survey {
    /// The paths, in the order Git listed them.
    pub candidates: Vec<Candidate>,
    /// What could not be read, and why. A note is never a failure here.
    pub notes: Vec<String>,
}

/// Classify the ignored state at `path` and change nothing.
///
/// This is the read-only half of [`sweep`], and it is the whole of the two gates: an
/// ignore rule has to cover a path for it to be here at all, and
/// [`super::exclude::regenerable`] is what says which of those a tool writes again.
/// `nodal reclaim --check` answers with this classification and `sweep` acts on it, so
/// the preflight and the prune cannot disagree about what a reclaim would drop.
///
/// A candidate under a directory already classed as regenerable is left out: the
/// directory answers for everything in it, and naming both would count the same bytes
/// twice.
///
/// That holds through a failed removal as well, and it is the one place where this
/// classification changed what a sweep does. A directory answers for what is under it
/// whether or not it goes, so a `node_modules` that would not be removed leaves the
/// trash the whole of it rather than pieces of it. The alternative — descending into a
/// directory whose own removal was refused — leaves a half-pruned tree that no report
/// describes, to save bytes in the one case where a removal failed. `git ls-files
/// --directory` collapses an ignored directory into one record and does not descend into
/// it, so the shape this decides is one Git does not produce for a regenerable path
/// anyway; it is stated because the behaviour has to be one thing and not an accident.
///
/// # Errors
/// None. A home that is not there, a directory that is not a repository and a listing
/// that failed are each a note, for the reason the module documentation gives.
#[must_use]
pub fn survey(path: &Path) -> Survey {
    let mut surveyed = Survey::default();
    if !path.is_dir() {
        // Said as a reading and not as an action, because two callers print it and only
        // one of them removes anything. `nodal reclaim --check` prints the notes of this
        // survey verbatim, and a read-only command must not report a prune it did not do.
        surveyed
            .notes
            .push(format!("{} is not there, so no ignored state was read", path.display()));
        return surveyed;
    }
    let entries = match Git::open(path).and_then(|git| git.ignored_entries()) {
        Ok(entries) => entries,
        Err(why) => {
            surveyed.notes.push(why.to_string());
            return surveyed;
        }
    };
    let mut regenerable: Vec<PathBuf> = Vec::new();
    for entry in &entries {
        if regenerable.iter().any(|whole| entry.relative.starts_with(whole)) {
            continue;
        }
        let reason = entry.directory.then(|| reason_for(&entry.relative)).flatten();
        if reason.is_some() {
            regenerable.push(entry.relative.clone());
        } else if holds_another(&entries, &entry.relative) || nodal_wrote(path, &entry.relative) {
            continue;
        }
        let bytes = size::measure(&path.join(&entry.relative)).bytes;
        surveyed.candidates.push(Candidate { path: entry.relative.clone(), bytes, reason });
    }
    surveyed
}

/// Whether everything at `relative` is a file Nodal wrote into the home.
///
/// One file answers by its name. A directory answers by what is under it, which is what
/// makes `.claude` Nodal's in a home holding nothing but the settings file Nodal wrote
/// there, and the person's again the moment they keep something of their own beside it.
///
/// An empty directory is nobody's state and is left out on the same reasoning: naming
/// it in a report offers a person a directory with nothing in it.
fn nodal_wrote(home: &Path, relative: &Path) -> bool {
    let path = home.join(relative);
    if path.is_file() {
        return files::WRITTEN.iter().any(|written| Path::new(written.path) == relative);
    }
    let Ok(entries) = std::fs::read_dir(&path) else { return false };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { return false };
        let inside = relative.join(entry.file_name());
        if kind.is_dir() {
            if !nodal_wrote(home, &inside) {
                return false;
            }
        } else if !files::WRITTEN.iter().any(|written| Path::new(written.path) == inside) {
            return false;
        }
    }
    true
}

/// Whether another listed path is inside `relative`.
///
/// Git lists the whole chain: a directory the project does not track, holding nothing
/// but ignored content, is reported alongside every ignored path under it. So `apps`,
/// `apps/web` and `apps/web/node_modules` arrive as three records naming one tree.
///
/// Only the deepest of them is read. Counting a parent as well would add its bytes to
/// the report twice, and naming it as state the trash kept would offer a person a
/// directory that holds nothing but the dependencies the prune had already taken.
fn holds_another(entries: &[crate::git::ignored::Entry], relative: &Path) -> bool {
    entries.iter().any(|other| other.relative != relative && other.relative.starts_with(relative))
}

/// Why the trash need not keep `relative`, or `None` when the table does not call it
/// regenerable.
///
/// A row matches a directory whose path ends with it, so the `node_modules` of the
/// third package of a repository is the same row as the one at the root.
/// [`super::relocate`] matches the same way, for the same reason.
fn reason_for(relative: &Path) -> Option<&'static str> {
    exclude::regenerable()
        .into_iter()
        .find(|row| relative.ends_with(row.path))
        .map(|row| row.reason)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::sweep;

    #[test]
    fn a_home_that_is_not_a_repository_is_a_note_and_never_a_failure() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("target")).unwrap();
        let report = sweep(root.path());
        assert!(report.changed_nothing());
        assert_eq!(report.notes.len(), 1, "the reason is stated once");
        assert!(root.path().join("target").is_dir(), "nothing was removed on a reason it gave");
    }

    #[test]
    fn a_home_that_is_not_there_is_a_note_naming_it() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("gone");
        let report = sweep(&missing);
        assert!(report.changed_nothing());
        assert!(report.notes[0].contains("gone"), "{:?}", report.notes);
    }

    /// The note is a reading, not a claim about an action, because `nodal reclaim
    /// --check` prints these words and removes nothing.
    #[test]
    fn a_note_says_what_was_read_and_never_what_was_pruned() {
        let root = tempfile::tempdir().unwrap();
        let report = sweep(&root.path().join("gone"));
        assert!(report.notes[0].contains("no ignored state was read"), "{:?}", report.notes);
        assert!(!report.notes[0].contains("pruned"), "{:?}", report.notes);
    }

    #[test]
    fn a_report_that_removed_nothing_says_so() {
        let report = super::Report::default();
        assert!(report.changed_nothing());
        assert!(report.describe().contains("no build output"), "{}", report.describe());
    }
}
