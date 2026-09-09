//! What a machine has left behind: what `nodal doctor` answers with.
//!
//! Doctor reports and it does not act (`decisions/DL-015`). That decision reaches into
//! this file, because the words are the promise. Nothing here is written in a tense
//! that could be read as something Nodal did to the machine: a row says what is there
//! and how big it is, and the document ends with a line that says Nodal removed
//! nothing. A later command that removes things is a later command.
//!
//! Two sections, and the difference between them is not presentation. [`Doctor::here`]
//! is what belongs to the project the command was run in. [`Doctor::elsewhere`] is what
//! belongs to another project, and it carries a name and a size and nothing else: no
//! state, no intent, and no suggestion that a person do anything about it. A person
//! cleaning up one project must not be led into another project's work.
//!
//! A third section carries the local branches with no worktree ([`Branches`]), and its
//! rendering is the one place in this file where the two renderings differ in what they
//! show. A measured machine held 306 such branches and 22 of them held commits that
//! exist on no remote. Printing 306 rows at one weight puts the 22 inside the 284. So
//! the loud bucket prints a row each and the two safe buckets print one line each with
//! a count, and `--all` ([`Branches::expand`]) opens them. `--json` carries every row
//! either way: the flag chooses how much is shown, never what was found.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, Table};
use crate::workspace::sharing::{self, Sharing};

/// The columns of the section for this project.
const HERE: [&str; 5] = ["what", "kind", "size", "state", "intent"];

/// The columns of the section for other projects. Names and sizes, and no more.
const ELSEWHERE: [&str; 3] = ["what", "kind", "size"];

/// The columns of the loud bucket of the branch section.
const UNPUSHED: [&str; 4] = ["branch", "unpushed", "last commit", "upstream"];

/// The columns the safe buckets print under `--all`.
const SAFE: [&str; 3] = ["branch", "last commit", "where its commits are"];

/// Git's word for an upstream that is not there any more.
const GONE: &str = "gone";

/// How many characters of a recovered intent a row shows.
const INTENT_WIDTH: usize = 56;

/// The line that closes every report, in both renderings.
pub const CLOSING: &str = "nodal read this machine. it removed nothing and moved nothing.";

/// What kind of thing a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Another checkout of this repository, which another tool made. Under the
    /// checkout, beside it, or anywhere else the tool put it.
    Worktree,
    /// Generated build state that nothing has written to for a long time.
    StaleCache,
    /// A container that has stopped and is still there.
    ExitedContainer,
    /// A volume no container refers to.
    DanglingVolume,
    /// A directory named as a Nodal database, with no row in the registry.
    OrphanDatabase,
    /// A project holding more open units than the threshold.
    UnitCount,
}

impl Kind {
    /// What this kind is called in a row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::StaleCache => "stale cache",
            Self::ExitedContainer => "exited container",
            Self::DanglingVolume => "dangling volume",
            Self::OrphanDatabase => "orphan database",
            Self::UnitCount => "unit count",
        }
    }
}

/// One thing doctor found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// What kind of thing it is.
    pub kind: Kind,
    /// What it is called: a path, a container name, a volume name.
    pub what: String,
    /// What it holds, or `None` when nothing could measure it. A locked worktree has
    /// no size here, because doctor does not read inside one.
    pub bytes: Option<u64>,
    /// Whether the figure is the whole of it. `false` means an entry could not be read,
    /// so the size is a floor.
    #[serde(default)]
    pub partial: bool,
    /// What is true of it, in the words a report uses: `unpushed 3`, `dirty 12`,
    /// `behind 4`, `locked`.
    #[serde(default)]
    pub state: Vec<String>,
    /// Why the thing was made, where a record of that survives.
    #[serde(default)]
    pub intent: Option<String>,
}

impl Finding {
    /// A finding of this kind, by this name, with nothing else known yet.
    #[must_use]
    pub fn new(kind: Kind, what: impl Into<String>) -> Self {
        Self {
            kind,
            what: what.into(),
            bytes: None,
            partial: false,
            state: Vec::new(),
            intent: None,
        }
    }

    /// The same finding with a size.
    #[must_use]
    pub const fn sized(mut self, bytes: u64, complete: bool) -> Self {
        self.bytes = Some(bytes);
        self.partial = !complete;
        self
    }

    /// The same finding with one more word of state.
    #[must_use]
    pub fn says(mut self, state: impl Into<String>) -> Self {
        self.state.push(state.into());
        self
    }

    /// The size as a person reads it, with a mark when it is a floor.
    #[must_use]
    pub fn size(&self) -> String {
        match self.bytes {
            None => String::from(human::NONE),
            Some(bytes) if self.partial => format!("{}+", human::bytes(bytes)),
            Some(bytes) => human::bytes(bytes),
        }
    }
}

/// Where a local branch's commits already are, which is what says whether losing the
/// branch would lose work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Commits of this branch exist on no remote-tracking ref. This is the only bucket
    /// that holds work a machine could lose.
    Unpushed,
    /// The default branch does not hold it, and every commit of it is on a remote.
    OnRemote,
    /// The default branch already holds it.
    Merged,
}

impl Standing {
    /// What this bucket is called in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unpushed => "unpushed",
            Self::OnRemote => "on a remote",
            Self::Merged => "merged",
        }
    }
}

/// One local branch that no worktree has checked out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRow {
    /// The short name, without `refs/heads/`.
    pub name: String,
    /// Which bucket it is in.
    pub standing: Standing,
    /// How many of its commits exist on no remote.
    pub unpushed: usize,
    /// The remote-tracking branch it is set to follow, `None` when it follows none.
    #[serde(default)]
    pub upstream: Option<String>,
    /// When the commit at its tip was made.
    pub committed: Timestamp,
    /// Whether the upstream it names is not there any more. A `fetch --prune` after
    /// somebody deleted the remote branch leaves this shape, and it is the case where a
    /// person's own tool has stopped telling them where the work is.
    #[serde(default)]
    pub upstream_gone: bool,
}

/// The local branches of the surveyed checkout that no worktree has checked out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Branches {
    /// The branch the merged bucket is measured against, `None` when this repository
    /// has none.
    #[serde(default)]
    pub base: Option<String>,
    /// Every branch, loudest first.
    #[serde(default)]
    pub rows: Vec<BranchRow>,
    /// Whether the safe buckets print row by row (`--all`).
    ///
    /// A rendering choice and not an answer. Both renderings carry every row; this
    /// decides how many of them the human one prints.
    #[serde(default, skip_serializing)]
    pub expand: bool,
}

impl Branches {
    /// The rows of one bucket, in the order they were found.
    #[must_use]
    pub fn bucket(&self, standing: Standing) -> Vec<&BranchRow> {
        self.rows.iter().filter(|row| row.standing == standing).collect()
    }
}

/// The checkout the command was run in, and the project it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    /// The top of the working tree.
    pub root: PathBuf,
    /// The project the registry names for that tree, `None` when it knows none. Doctor
    /// works before any unit exists, which is the state it is most needed in.
    pub project: Option<String>,
}

/// What a source could not read. A note is not a failure: it is the difference between
/// "nothing is there" and "I could not see".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Which source could not answer.
    pub source: String,
    /// What it said, in one line.
    pub why: String,
}

/// What a machine has left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Doctor {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The checkout the command was run in, `None` when it was run outside one.
    pub checkout: Option<Checkout>,
    /// The state root this machine keeps its homes under.
    pub state_root: PathBuf,
    /// What Nodal recorded about sharing file blocks there, and `None` where nothing
    /// has recorded it yet. A state root that cannot share blocks between files makes
    /// every unit home a full copy, and a person who is on one has no other way to
    /// learn it before the first home is made.
    ///
    /// Doctor reads the record and never takes one. A record is taken when the state
    /// root is made, and again when `nodal init --reprobe` asks.
    pub sharing: Option<Sharing>,
    /// What belongs to this project, largest first.
    pub here: Vec<Finding>,
    /// What belongs to another project, largest first. Names and sizes only.
    pub elsewhere: Vec<Finding>,
    /// The local branches of this checkout that no worktree has checked out.
    #[serde(default)]
    pub branches: Branches,
    /// What a source could not read.
    pub notes: Vec<Note>,
}

impl Doctor {
    /// The one line about the state root: the recorded fact, or that nothing recorded
    /// one. Doctor prints it in every case, so that a person who fixed a state root can
    /// see that they did.
    fn state_root_line(&self) -> String {
        self.sharing.as_ref().map_or_else(|| sharing::unrecorded(&self.state_root), Sharing::fact)
    }
}

impl Render for Doctor {
    const KIND: &'static str = "doctor";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        doc.push(Block::fields(vec![Field::new("state root", self.state_root_line())]).at(0));
        doc.push(Block::blank());
        doc.push(Block::fields(vec![Field::new("this project", self.subject())]).at(0));
        doc.push(section(&self.here, &HERE, "nothing of this project is left behind"));
        doc.push(Block::blank());
        doc.push(
            Block::fields(vec![Field::new("not this project", "other projects' leftovers")]).at(0),
        );
        doc.push(section(&self.elsewhere, &ELSEWHERE, "nothing of another project is here"));
        doc.push(Block::blank());
        doc.push(
            Block::fields(vec![Field::new("branches", "local branches with no worktree")]).at(0),
        );
        for block in self.branch_blocks() {
            doc.push(block);
        }
        if !self.notes.is_empty() {
            doc.push(Block::blank());
        }
        for note in &self.notes {
            doc.push(Block::line(format!("{}: {}", note.source, note.why)).at(0));
        }
        doc.push(Block::blank());
        doc.push(Block::line(CLOSING).at(0));
        doc
    }
}

impl Doctor {
    /// The checkout line: the project's name and where it is.
    fn subject(&self) -> String {
        let Some(checkout) = &self.checkout else {
            return String::from("no checkout here; every finding is under \"not this project\"");
        };
        let root = checkout.root.display().to_string();
        checkout
            .project
            .as_ref()
            .map_or_else(|| root.clone(), |name| format!("{name}{}{root}", human::JOIN))
    }
}

impl Doctor {
    /// The branch section: the loud bucket in full, and the safe buckets as counts.
    ///
    /// This is the one place a rendering shows less than it holds. A machine with 306
    /// branches and 22 of them unbacked-up needs the 22 read, and 284 rows above them
    /// is how a person stops reading. `--all` opens the safe buckets.
    fn branch_blocks(&self) -> Vec<Block> {
        if self.branches.rows.is_empty() {
            return vec![Block::line("no local branch is without a worktree")];
        }
        let mut blocks = vec![self.loud()];
        for standing in [Standing::Merged, Standing::OnRemote] {
            blocks.extend(self.safe(standing));
        }
        blocks
    }

    /// The unpushed branches, one row each, or a line saying there are none.
    fn loud(&self) -> Block {
        let unpushed = self.branches.bucket(Standing::Unpushed);
        if unpushed.is_empty() {
            return Block::line("no branch holds commits that exist on no remote");
        }
        let mut table = Table::new(&UNPUSHED);
        for row in unpushed {
            table.push(vec![
                row.name.clone(),
                row.unpushed.to_string(),
                human::span(self.now, row.committed),
                upstream(row),
            ]);
        }
        Block::table(table)
    }

    /// One safe bucket: a count, and the rows themselves under `--all`.
    fn safe(&self, standing: Standing) -> Vec<Block> {
        let rows = self.branches.bucket(standing);
        if rows.is_empty() {
            return Vec::new();
        }
        let count = Block::line(format!("{} {}", rows.len(), self.bucket_name(standing)));
        if !self.branches.expand {
            return vec![count];
        }
        let mut table = Table::new(&SAFE);
        for row in rows {
            table.push(vec![
                row.name.clone(),
                human::span(self.now, row.committed),
                self.bucket_name(standing),
            ]);
        }
        vec![count, Block::table(table)]
    }

    /// What a bucket is called in this report, with the branch a merge is measured
    /// against named rather than implied.
    fn bucket_name(&self, standing: Standing) -> String {
        match (standing, &self.branches.base) {
            (Standing::Merged, Some(base)) => format!("merged into {base}"),
            (Standing::Merged, None) => String::from("merged"),
            (Standing::OnRemote, _) => String::from("unmerged, every commit on a remote"),
            (Standing::Unpushed, _) => String::from("unpushed"),
        }
    }
}

/// What a branch's upstream column says: the upstream it follows, that the upstream is
/// not there any more, or that it follows none.
///
/// `gone` is git's own word for the second, printed in `%(upstream:track)`. Doctor
/// states git's verdict and does not rephrase it, and the word it would otherwise reach
/// for is one a read-only report may not use ([`super::doctor`] is checked against
/// them).
fn upstream(row: &BranchRow) -> String {
    if row.upstream_gone {
        return String::from(GONE);
    }
    row.upstream.clone().unwrap_or_else(|| String::from(human::NONE))
}

/// One section: a table of findings, or a line saying there are none.
fn section(findings: &[Finding], columns: &[&str], empty: &str) -> Block {
    if findings.is_empty() {
        return Block::line(empty);
    }
    let mut table = Table::new(columns);
    for finding in findings {
        table.push(cells(finding, columns.len()));
    }
    Block::table(table)
}

/// The cells of one row. A section with three columns shows the name, the kind and the
/// size, and stops there: that is what makes the second section names and sizes only.
fn cells(finding: &Finding, columns: usize) -> Vec<String> {
    let mut row = vec![finding.what.clone(), finding.kind.label().to_owned(), finding.size()];
    if columns > 3 {
        row.push(human::join(&finding.state));
        row.push(shorten(finding.intent.as_deref()));
    }
    row
}

/// An intent as one line, cut to the width a column holds.
fn shorten(intent: Option<&str>) -> String {
    let Some(intent) = intent else {
        return String::from(human::NONE);
    };
    let one_line: String = intent.split_whitespace().collect::<Vec<&str>>().join(" ");
    if one_line.chars().count() <= INTENT_WIDTH {
        return one_line;
    }
    let cut: String = one_line.chars().take(INTENT_WIDTH - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::{
        BranchRow, Branches, CLOSING, Checkout, Doctor, Finding, Kind, Note, Standing, shorten,
    };
    use crate::model::Timestamp;
    use crate::output::Render;
    use crate::workspace::sharing::{Shares, Sharing};

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("a fixed instant")
    }

    fn report() -> Doctor {
        Doctor {
            now: at("2026-09-07T12:00:00Z"),
            checkout: Some(Checkout {
                root: "/home/j/code/storefront".into(),
                project: Some(String::from("storefront")),
            }),
            here: vec![
                Finding::new(Kind::Worktree, ".claude/worktrees/auth")
                    .sized(7_850_000_000, true)
                    .says("unpushed 3")
                    .says("dirty 12"),
                Finding::new(Kind::Worktree, ".claude/worktrees/held").says("locked"),
            ],
            elsewhere: vec![
                Finding::new(Kind::StaleCache, "/home/j/other/.next").sized(3_330_000_000, true),
            ],
            branches: branches(),
            state_root: PathBuf::from("/home/j/.nodal"),
            sharing: Some(sharing(Shares::Yes)),
            notes: vec![Note {
                source: String::from("docker"),
                why: String::from("docker is not installed"),
            }],
        }
    }

    /// The record a test forces about the state root, for each of the three answers.
    fn sharing(shares: Shares) -> Sharing {
        let filesystem = if shares == Shares::Yes { "btrfs" } else { "ext4" };
        Sharing {
            root: PathBuf::from("/home/j/.nodal"),
            filesystem: Some(String::from(filesystem)),
            shares,
            probed_at: at("2026-03-02T09:00:00Z"),
            device: Some(66),
        }
    }

    /// One branch of each bucket, and one whose upstream was deleted.
    fn branches() -> Branches {
        Branches {
            base: Some(String::from("origin/main")),
            rows: vec![
                branch("importer/retry", Standing::Unpushed, 33, true),
                branch("hotfix/logs", Standing::Unpushed, 2, false),
                branch("shipped", Standing::Merged, 0, false),
                branch("review/api", Standing::OnRemote, 0, false),
            ],
            expand: false,
        }
    }

    fn branch(name: &str, standing: Standing, unpushed: usize, gone: bool) -> BranchRow {
        BranchRow {
            name: String::from(name),
            standing,
            unpushed,
            upstream: gone.then(|| format!("origin/{name}")),
            committed: at("2026-09-01T12:00:00Z"),
            upstream_gone: gone,
        }
    }

    /// The header says which case this machine is in, and it says it either way. A
    /// report that spoke up only about the bad case would leave a person who fixed it
    /// with no way to see that they had.
    #[test]
    fn doctor_says_the_state_root_shares_blocks() {
        let text = report().doc().to_string();
        let line = header(&text);
        assert!(line.contains("/home/j/.nodal"), "{line}");
        assert!(line.contains("btrfs"), "{line}");
        assert!(line.contains("nodal shares blocks here"), "{line}");
        assert!(!line.contains("full copy"), "{line}");
    }

    #[test]
    fn doctor_says_when_nodal_does_not_share_blocks_at_the_state_root() {
        let line = header(&with(sharing(Shares::No)).doc().to_string());
        assert!(line.contains("ext4"), "{line}");
        assert!(line.contains("nodal does not share blocks here"), "{line}");
        assert!(line.contains("full copy"), "{line}");
    }

    /// A state root nobody could ask is reported as one. "Could not ask" and "cannot
    /// share" are different machines and different fixes, and only one of them is
    /// about a filesystem.
    #[test]
    fn doctor_says_when_the_question_could_not_be_put() {
        let refused = Shares::could_not_ask(&std::io::Error::from_raw_os_error(libc::EROFS));
        let line = header(&with(sharing(refused)).doc().to_string());
        assert!(line.contains("could not ask"), "{line}");
        assert!(line.contains(&format!("errno {}", libc::EROFS)), "{line}");
        assert!(!line.contains("does not share blocks"), "{line}");
    }

    /// A machine nothing has recorded an answer for is said to have no record. Doctor
    /// does not take one: a probe writes, and this command writes nothing.
    #[test]
    fn doctor_says_when_there_is_no_record() {
        let mut unrecorded = report();
        unrecorded.sharing = None;
        let line = header(&unrecorded.doc().to_string());
        assert!(line.contains("/home/j/.nodal"), "{line}");
        assert!(line.contains("has not recorded"), "{line}");
        assert!(line.contains("--reprobe"), "{line}");
        assert!(!line.contains("full copy"), "{line}");
    }

    /// Doctor states the fact and prints no fix. `nodal init` prints the fix, at the
    /// moment a person chooses the state root and can still choose another.
    #[test]
    fn doctor_prints_no_fix() {
        let line = header(&with(sharing(Shares::No)).doc().to_string());
        assert!(!line.contains("move the state root"), "{line}");
    }

    /// The report of a machine whose state root answered this way.
    fn with(record: Sharing) -> Doctor {
        Doctor { sharing: Some(record), ..report() }
    }

    /// The one line of the header that is about the state root.
    fn header(text: &str) -> String {
        text.lines().find(|line| line.contains("state root")).expect("the state root line").into()
    }

    /// The branch section, and the loud bucket printed in full.
    ///
    /// A machine with 306 branches and 22 of them holding commits no remote has needs
    /// the 22 read. Every one of them is a row, with the count, the age and the state of
    /// its upstream.
    #[test]
    fn every_branch_holding_commits_no_remote_has_is_a_row_of_its_own() {
        let text = report().doc().to_string();
        let line = text.lines().find(|line| line.contains("importer/retry")).expect("the row");
        assert!(line.contains("33"), "the count of commits nobody else has: {line}");
        assert!(line.contains('d'), "how old the last commit is: {line}");
        assert!(line.contains("gone"), "and that its upstream is not there: {line}");
        assert!(text.contains("hotfix/logs"), "{text}");
    }

    /// The safe buckets are counts, so that they cannot bury the loud one.
    #[test]
    fn a_branch_whose_commits_are_elsewhere_is_a_count_and_not_a_row() {
        let text = report().doc().to_string();
        assert!(text.contains("1 merged into origin/main"), "{text}");
        assert!(text.contains("1 unmerged, every commit on a remote"), "{text}");
        assert!(!text.contains("shipped"), "a safe branch is not a row by default: {text}");
        assert!(!text.contains("review/api"), "{text}");
    }

    #[test]
    fn all_opens_the_safe_buckets_without_changing_what_was_found() {
        let mut opened = report();
        opened.branches.expand = true;
        let text = opened.doc().to_string();
        assert!(text.contains("shipped"), "{text}");
        assert!(text.contains("review/api"), "{text}");
        assert!(text.contains("1 merged into origin/main"), "the counts stay: {text}");
        assert!(text.contains("importer/retry"), "and so does the loud bucket: {text}");
    }

    #[test]
    fn a_checkout_whose_branches_all_have_worktrees_says_so() {
        let mut quiet = report();
        quiet.branches = Branches::default();
        let text = quiet.doc().to_string();
        assert!(text.contains("no local branch is without a worktree"), "{text}");
    }

    #[test]
    fn a_checkout_with_no_unpushed_branch_says_that_and_still_counts_the_rest() {
        let mut safe = report();
        safe.branches.rows.retain(|row| row.standing != Standing::Unpushed);
        let text = safe.doc().to_string();
        assert!(text.contains("no branch holds commits that exist on no remote"), "{text}");
        assert!(text.contains("1 merged into origin/main"), "{text}");
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
    fn the_second_section_carries_names_and_sizes_and_no_more() {
        let text = report().doc().to_string();
        let (_, tail) = text.split_once("not this project").expect("the second section");
        assert!(tail.contains("3.3 GB"), "{tail}");
        assert!(!tail.to_uppercase().contains("INTENT"), "{tail}");
        assert!(!tail.to_uppercase().contains("STATE"), "{tail}");
    }

    #[test]
    fn a_locked_worktree_has_no_size_because_doctor_did_not_look_inside() {
        let text = report().doc().to_string();
        let line = text.lines().find(|line| line.contains("held")).expect("the locked row");
        assert!(line.contains("locked"), "{line}");
        assert!(line.contains(crate::output::human::NONE), "{line}");
    }

    #[test]
    fn a_long_intent_is_cut_to_one_line() {
        let long = "a ".repeat(80);
        let cut = shorten(Some(&long));
        assert!(cut.chars().count() <= 56, "{cut}");
        assert!(cut.ends_with('…'), "{cut}");
        assert_eq!(shorten(Some("two\nlines")), "two lines");
        assert_eq!(shorten(None), crate::output::human::NONE);
    }
}
