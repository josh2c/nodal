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
    /// Apparent bytes under it, for one clone. A group's row carries the bytes that
    /// remain once a file shared between its clones is counted once.
    pub bytes: u64,
    /// Of `bytes`, how many belong to files another path in this survey holds too.
    #[serde(default)]
    pub repeated: u64,
}

impl IgnoredDir {
    /// One ignored directory, none of whose bytes are known to be anywhere else.
    #[must_use]
    pub fn new(path: impl Into<String>, bytes: u64) -> Self {
        Self { path: path.into(), bytes, repeated: 0 }
    }

    /// One ignored directory, with the bytes of it that were counted somewhere else.
    #[must_use]
    pub fn shared(path: impl Into<String>, bytes: u64, repeated: u64) -> Self {
        Self { path: path.into(), bytes, repeated }
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
    /// Commits of HEAD that no remote-tracking ref this run trusts already holds.
    ///
    /// A ref is trusted unless a clone of the same remote that heard from it later does
    /// not have the branch. That is how a branch deleted on the remote stops counting as
    /// a push; `super::super::super::doctor::unique` states the rule in full.
    pub unpushed: usize,
    /// Commits of HEAD no other copy on this machine holds, `None` when the proof could
    /// not be made.
    #[serde(default)]
    pub only_copy: Option<usize>,
    /// Why this clone was not checked, `None` when it was.
    #[serde(default)]
    pub unchecked: Option<String>,
    /// Paths a commit would capture.
    pub dirty: usize,
    /// Logical size of the working tree and its `.git`.
    pub bytes: u64,
    /// Of `bytes`, how many belong to files another path in this survey was already
    /// counted for. A group takes these off so a hardlinked file is counted once.
    #[serde(default)]
    pub repeated: u64,
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
    /// Every clone was checked, and none of them holds work that is only here.
    pub nothing_unique: bool,
    /// How many clones hold commits no other copy on this machine has.
    #[serde(default)]
    pub unique_clones: usize,
    /// How many commits those clones are the only copy of.
    #[serde(default)]
    pub unique_commits: usize,
    /// How many clones this run could not examine.
    #[serde(default)]
    pub unchecked: usize,
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
        for group in &self.groups {
            for block in details(group) {
                doc.push(block);
            }
        }
        if let Some(line) = self.not_checked() {
            doc.push(Block::blank());
            doc.push(line);
        }
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

    /// How many clones this run could not examine, when any could not be.
    fn not_checked(&self) -> Option<Block> {
        let clones: usize = self.groups.iter().map(|group| group.unchecked).sum();
        (clones > 0).then(|| {
            Block::line(format!(
                "{} not checked; a clone this run could not read is not known to be safe",
                plural(clones, "clone")
            ))
        })
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
        unique(group),
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
///
/// "nothing unique" is a conclusion this run reached by looking, so it is printed only
/// when every clone of the group was examined and none of them is the only copy of
/// anything. A group with a clone that could not be read says so instead.
fn unique(group: &Group) -> String {
    let mut said = Vec::new();
    if group.unique_clones > 0 {
        let holds = if group.unique_clones == 1 { "holds" } else { "hold" };
        said.push(format!("{} {holds} unique work", plural(group.unique_clones, "clone")));
    } else if group.nothing_unique {
        said.push(String::from("nothing unique"));
    }
    if group.unchecked > 0 {
        said.push(format!("{} not checked", plural(group.unchecked, "clone")));
    }
    let off = group.repositories.iter().filter(|row| row.unpushed > 0).count();
    if off > group.unique_clones {
        said.push(format!("{} on no remote", off - group.unique_clones));
    }
    if said.is_empty() { String::from(human::NONE) } else { human::join(&said) }
}

/// How many clones a list names before it says how many more there are.
const LISTED: usize = 10;

/// What the person needs to do about a clone that holds the only copy of its commits.
const RESCUE: &str = "to send one of these to its remote, run `git push origin HEAD` in the clone";

/// The clones of one group that hold work, named, with a count each.
///
/// Two lists, because they answer two questions. The first is the one the uniqueness
/// column answers: delete this folder and these commits are gone from the machine. The
/// second is a weaker warning about clones whose commits another clone here still holds,
/// so the folder is safe to delete, but no remote has the work.
fn details(group: &Group) -> Vec<Block> {
    let only: Vec<&CloneRow> =
        group.repositories.iter().filter(|row| row.only_copy.is_some_and(|n| n > 0)).collect();
    let off: Vec<&CloneRow> = group
        .repositories
        .iter()
        .filter(|row| row.unpushed > 0 && row.only_copy.is_none_or(|only| only == 0))
        .collect();
    let unread: Vec<&CloneRow> =
        group.repositories.iter().filter(|row| row.unchecked.is_some()).collect();
    if only.is_empty() && off.is_empty() && unread.is_empty() {
        return Vec::new();
    }
    let mut blocks = vec![Block::blank(), Block::line(group.name.clone())];
    blocks.extend(listing("unique work, the only copy on this machine", &only, |row| {
        row.only_copy.unwrap_or(0)
    }));
    blocks.extend(listing("on no remote, held by another clone here", &off, |row| row.unpushed));
    blocks.extend(unreadable(&unread));
    if !only.is_empty() {
        blocks.push(Block::line(RESCUE).at(2));
    }
    blocks
}

/// One titled list of clones and how many commits each one holds.
fn listing(title: &str, rows: &[&CloneRow], count: impl Fn(&CloneRow) -> usize) -> Vec<Block> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut blocks = vec![Block::line(title).at(2)];
    for row in rows.iter().take(LISTED) {
        blocks.push(
            Block::line(format!("{}  {}", row.path.display(), plural(count(row), "commit"))).at(3),
        );
    }
    if rows.len() > LISTED {
        blocks.push(Block::line(format!("and {} more", rows.len() - LISTED)).at(3));
    }
    blocks
}

/// The clones the run could not examine, with what stopped it.
fn unreadable(rows: &[&CloneRow]) -> Vec<Block> {
    if rows.is_empty() {
        return Vec::new();
    }
    let mut blocks = vec![Block::line("not checked").at(2)];
    for row in rows.iter().take(LISTED) {
        let why = row.unchecked.as_deref().unwrap_or("no reason given");
        blocks.push(Block::line(format!("{}  {why}", row.path.display())).at(3));
    }
    blocks
}

/// A count and its noun, with the `s` English wants on everything but one.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
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
                unique_clones: 1,
                unique_commits: 1,
                unchecked: 0,
                repositories: vec![CloneRow {
                    path: "/tmp/a".into(),
                    branch: String::from("main"),
                    origin: Some(String::from("git@github.com:josh2c/nodal.git")),
                    unpushed: 1,
                    only_copy: Some(1),
                    unchecked: None,
                    dirty: 0,
                    bytes: 20_000_000,
                    repeated: 0,
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
        let text = clean().doc().to_string();
        assert!(text.contains("nothing unique"), "{text}");
    }

    /// A group whose every clone was examined and found to be a second copy.
    fn clean() -> MachineReport {
        let mut clean = report();
        clean.groups[0].nothing_unique = true;
        clean.groups[0].unpushed = 0;
        clean.groups[0].dirty = 0;
        clean.groups[0].unique_clones = 0;
        clean.groups[0].unique_commits = 0;
        clean.groups[0].repositories[0].unpushed = 0;
        clean.groups[0].repositories[0].only_copy = Some(0);
        clean
    }

    /// The fault this column was rebuilt for: a clone the run could not read used to
    /// fall into "nothing unique" with every clone it had read.
    #[test]
    fn a_clone_that_could_not_be_read_is_not_called_clean() {
        let mut report = clean();
        report.groups[0].unchecked = 1;
        report.groups[0].nothing_unique = false;
        report.groups[0].repositories[0].unchecked = Some(String::from("permission denied"));
        let text = report.doc().to_string();
        assert!(!text.contains("nothing unique"), "{text}");
        assert!(text.contains("1 clone not checked"), "{text}");
        assert!(text.contains("permission denied"), "{text}");
    }

    /// A clone that is the only copy of its commits is named, counted, and followed by
    /// the one command that would send the work somewhere else.
    #[test]
    fn a_clone_holding_the_only_copy_is_named_with_its_count() {
        let text = report().doc().to_string();
        assert!(text.contains("1 clone holds unique work"), "{text}");
        assert!(text.contains("/tmp/a  1 commit"), "{text}");
        assert!(text.contains("git push origin HEAD"), "{text}");
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
