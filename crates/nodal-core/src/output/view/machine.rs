//! What `nodal doctor --machine` answers with: every clone under the roots, grouped.
//!
//! The report writes nothing. A row is a clone Nodal did not make. A unit home is
//! skipped, not listed. The last line is the same promise [`super::doctor::CLOSING`]
//! makes for the checkout survey.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, Table};
use crate::output::view::doctor::CLOSING;

/// The columns of the group table, largest first.
const COLUMNS: [&str; 8] =
    ["group", "clones", "unpushed", "dirty", "size", "ignored", "last commit", "unique"];

/// How many ignored directories a row names.
const IGNORED: usize = 3;

/// A directory an ignore rule covers, with its logical size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoredDir {
    /// The path relative to the clone, or the shared name in a group.
    pub path: String,
    /// Apparent bytes under it.
    pub bytes: u64,
}

impl IgnoredDir {
    /// One ignored directory.
    #[must_use]
    pub fn new(path: impl Into<String>, bytes: u64) -> Self {
        Self { path: path.into(), bytes }
    }
}

/// One clone the walk found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneRow {
    /// The working tree, resolved.
    pub path: PathBuf,
    /// The checked-out branch, or `detached`.
    pub branch: String,
    /// The URL of `origin`, `None` when this clone names no such remote.
    #[serde(default)]
    pub origin: Option<String>,
    /// Commits of HEAD that exist on no remote-tracking ref.
    pub unpushed: usize,
    /// Paths a commit would capture.
    pub dirty: usize,
    /// Logical size of the working tree and its `.git`.
    pub bytes: u64,
    /// Whether the size is a floor because an entry could not be read.
    #[serde(default)]
    pub partial: bool,
    /// The three largest ignored directories.
    #[serde(default)]
    pub ignored: Vec<IgnoredDir>,
    /// When HEAD was committed, `None` when the clone has no commit.
    #[serde(default)]
    pub committed: Option<Timestamp>,
}

/// Clones that share one `origin` URL, or one clone that has no remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// The normalised origin URL, or the path of a clone with no remote.
    pub name: String,
    /// How many clones the group holds.
    pub clones: usize,
    /// Unpushed commits across the clones.
    pub unpushed: usize,
    /// How many clones have uncommitted paths.
    pub dirty: usize,
    /// Logical size of every clone.
    pub bytes: u64,
    /// The three largest ignored directories, summed by name.
    #[serde(default)]
    pub ignored: Vec<IgnoredDir>,
    /// The most recent HEAD commit in the group.
    #[serde(default)]
    pub committed: Option<Timestamp>,
    /// Every clone is clean and every commit exists on a remote.
    pub nothing_unique: bool,
    /// Each clone, largest first.
    pub repositories: Vec<CloneRow>,
}

/// How many entries the walk looked at, and how long the survey took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Walked {
    /// Directory entries inspected, including the size walks.
    pub entries: u64,
    /// Wall time of the survey, in whole milliseconds.
    pub millis: u64,
}

/// A path the walk did not enter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skip {
    /// The directory.
    pub path: PathBuf,
    /// Why it was skipped.
    pub why: String,
}

impl Skip {
    /// One skipped path.
    #[must_use]
    pub fn new(path: impl AsRef<Path>, why: impl Into<String>) -> Self {
        Self { path: path.as_ref().to_path_buf(), why: why.into() }
    }
}

/// Every clone under the roots, grouped by remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineReport {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The roots that were walked.
    pub roots: Vec<PathBuf>,
    /// Directory levels walked from each root.
    pub depth: usize,
    /// What the walk cost.
    pub walked: Walked,
    /// Paths the walk did not enter.
    #[serde(default)]
    pub skipped: Vec<Skip>,
    /// Groups, largest first.
    pub groups: Vec<Group>,
}

impl Render for MachineReport {
    const KIND: &'static str = "doctor.machine";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        doc.push(self.header());
        doc.push(self.body());
        for skip in &self.skipped {
            doc.push(Block::line(format!("{}: {}", skip.path.display(), skip.why)));
        }
        doc.push(Block::blank());
        doc.push(Block::line(CLOSING).at(0));
        doc
    }
}

impl MachineReport {
    /// The roots, the depth, and what the walk cost.
    fn header(&self) -> Block {
        let roots = self
            .roots
            .iter()
            .map(|root| root.display().to_string())
            .collect::<Vec<_>>()
            .join(human::JOIN);
        Block::fields(vec![
            Field::new("roots", roots),
            Field::new("depth", self.depth.to_string()),
            Field::new(
                "walked",
                format!("{} files · {} ms", self.walked.entries, self.walked.millis),
            ),
        ])
        .at(0)
    }

    /// The group table, or a line saying there is none.
    fn body(&self) -> Block {
        if self.groups.is_empty() {
            return Block::line("no repository under these roots");
        }
        let mut table = Table::new(&COLUMNS);
        for group in &self.groups {
            table.push(row(self.now, group));
        }
        Block::table(table)
    }
}

/// The cells of one group.
fn row(now: Timestamp, group: &Group) -> Vec<String> {
    vec![
        group.name.clone(),
        group.clones.to_string(),
        group.unpushed.to_string(),
        group.dirty.to_string(),
        human::bytes(group.bytes),
        ignored(&group.ignored),
        group.committed.map_or_else(|| String::from(human::NONE), |then| human::span(now, then)),
        unique(group.nothing_unique),
    ]
}

/// The ignored column: names and sizes, or the placeholder.
fn ignored(dirs: &[IgnoredDir]) -> String {
    let cells: Vec<String> = dirs
        .iter()
        .take(IGNORED)
        .map(|dir| format!("{} {}", dir.path, human::bytes(dir.bytes)))
        .collect();
    human::join(&cells)
}

/// What the unique column says.
fn unique(nothing: bool) -> String {
    if nothing { String::from("nothing unique") } else { String::from(human::NONE) }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use super::{CloneRow, Group, IgnoredDir, MachineReport, Skip, Walked};
    use crate::model::Timestamp;
    use crate::output::Render;
    use crate::output::view::doctor::CLOSING;

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("a fixed instant")
    }

    fn report() -> MachineReport {
        MachineReport {
            now: at("2026-09-07T12:00:00Z"),
            roots: vec!["/home/j".into()],
            depth: 6,
            walked: Walked { entries: 12_000, millis: 800 },
            skipped: vec![Skip::new("/home/j/.nodal", "nodal's state directory")],
            groups: vec![Group {
                name: String::from("github.com/josh2c/nodal"),
                clones: 3,
                unpushed: 1,
                dirty: 1,
                bytes: 50_000_000,
                ignored: vec![IgnoredDir::new("target", 40_000_000)],
                committed: Some(at("2026-09-06T12:00:00Z")),
                nothing_unique: false,
                repositories: vec![CloneRow {
                    path: "/tmp/a".into(),
                    branch: String::from("main"),
                    origin: Some(String::from("git@github.com:josh2c/nodal.git")),
                    unpushed: 1,
                    dirty: 0,
                    bytes: 20_000_000,
                    partial: false,
                    ignored: vec![IgnoredDir::new("target", 15_000_000)],
                    committed: Some(at("2026-09-06T12:00:00Z")),
                }],
            }],
        }
    }

    #[test]
    fn the_table_names_the_group_and_the_safe_to_delete_signal() {
        let text = report().doc().to_string();
        assert!(text.contains("github.com/josh2c/nodal"), "{text}");
        assert!(text.contains("50.0 MB"), "{text}");
        assert!(text.contains("target"), "{text}");
        assert!(!text.contains("nothing unique"), "{text}");
    }

    #[test]
    fn a_clean_pushed_group_says_nothing_unique() {
        let mut clean = report();
        clean.groups[0].nothing_unique = true;
        clean.groups[0].unpushed = 0;
        clean.groups[0].dirty = 0;
        let text = clean.doc().to_string();
        assert!(text.contains("nothing unique"), "{text}");
    }

    #[test]
    fn the_report_ends_by_saying_nothing_was_removed() {
        let text = report().doc().to_string();
        assert!(text.trim_end().ends_with(CLOSING), "{text}");
    }

    #[test]
    fn no_word_of_the_report_says_anything_was_taken_away() {
        let text = report().doc().to_string().to_lowercase();
        for word in ["deleted", "removed the", "cleaned", "freed", "pruned", "dropped"] {
            assert!(!text.contains(word), "{word:?} is in a report that only reads: {text}");
        }
    }

    #[test]
    fn a_skip_is_printed_with_its_reason() {
        let text = report().doc().to_string();
        assert!(text.contains("/home/j/.nodal: nodal's state directory"), "{text}");
    }
}
