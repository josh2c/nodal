//! The verdict on a checkout's other worktrees: what `nodal` answers with in a
//! repository it has never seen.
//!
//! This is the first minute of a person's life with the tool. They have twelve
//! worktrees another tool made, no memory of what nine of them were for, and no
//! reason yet to trust a command that writes. So the answer to a bare `nodal` in a
//! repository with no recipe and no registry row is one table and one closing line,
//! and producing it initialises nothing (`crate::runtime::verdict`).
//!
//! Every row is a **worktree**: a folder Nodal did not make. The word is the column
//! heading and the word the closing line counts in, and nothing here calls one of
//! these rows a unit. A unit is a home Nodal cloned, and the two are not the same
//! thing however alike their rows look.
//!
//! The columns answer, in order, the questions a person cleaning up asks:
//!
//! | column | the question |
//! |---|---|
//! | worktree | which folder is this, and where do I find it |
//! | for | what was it made for ([`crate::doctor::intent`]) |
//! | done | would merging it change any file ([`crate::git::integration`]) |
//! | only here | what does it hold that exists nowhere else |
//! | behind | how far has the base moved under it |
//! | size | what does it cost |
//! | age | how long has it been here |
//!
//! **ONLY HERE is the column the rest of the table is arranged around.** Done, behind
//! and size are all reasons to remove a worktree, and every one of them is wrong if
//! the directory holds the only copy of something. So the count of commits no remote
//! has and paths no commit holds is stated on its own, the sort puts a row that has
//! either at the top, and the closing line counts only the rows where both are zero.
//!
//! The closing line says what was found and then says that nothing was done about it,
//! in the same tense as [`super::doctor::CLOSING`] and for the same reason.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::integration::Integration;
use crate::model::{ProjectName, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, NONE, Table};

/// The columns of the verdict table, in the order it prints them.
///
/// The first heading is the word `worktree`, and that is a promise and not a label: a
/// row of this table is a folder Nodal did not make.
const COLUMNS: [&str; 7] = ["worktree", "for", "done", "only here", "behind", "size", "age"];

/// How many characters of an intent a row shows. The same width the doctor's report
/// uses, so one recovered prompt reads the same in both.
pub const INTENT_WIDTH: usize = 48;

/// What is said when nothing could say how far a worktree is behind.
///
/// A worktree with no upstream and a repository with no default branch are both this.
/// "0" would be a claim, and the claim would be the dangerous direction.
pub const UNKNOWN: &str = "unknown";

/// The sentence every verdict ends with, after the count and the size.
pub const CLOSING: &str = "nodal removed nothing.";

/// What a row is: a folder Nodal did not make, or a home it did.
///
/// The distinction is the whole reason this type exists. It is what stops the word
/// "unit" being printed against a directory some other tool made, and it is what the
/// closing line counts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A home Nodal made and the registry holds a row for.
    Unit,
    /// A worktree of this repository that Nodal did not make.
    Worktree,
}

impl RowKind {
    /// The word this kind carries in a row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Worktree => "worktree",
        }
    }
}

/// How far a worktree is behind, and what it was measured against.
///
/// The reference is carried with the count because the two are one fact. "twelve
/// behind" is a different statement about a branch depending on whether the twelve
/// commits are on a remote-tracking ref that was fetched this morning or on a local
/// `main` nobody has pulled for a month, and a person deciding what to delete needs
/// the second half.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behind {
    /// Commits the reference has that this worktree's HEAD does not.
    pub commits: u32,
    /// The revision it was measured against, as it was named.
    pub reference: String,
    /// Whether that revision is the branch's own upstream rather than the checkout's
    /// default branch.
    pub upstream: bool,
}

/// One worktree, as the verdict states it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRow {
    /// Whether Nodal made this one.
    pub kind: RowKind,
    /// How the row is named: relative to the checkout when it sits inside it, and by
    /// its whole path when it does not. A worktree beside the checkout is not findable
    /// by any shorter name.
    pub name: String,
    /// Where it is, in full, for a caller reading `--json`.
    pub path: PathBuf,
    /// The branch it has checked out, `None` when its HEAD is detached.
    #[serde(default)]
    pub branch: Option<String>,
    /// What it was made for, recovered or stated, `None` when no record of that
    /// survives.
    #[serde(default)]
    pub intent: Option<String>,
    /// What merging it into the checkout's default branch would do.
    pub done: Integration,
    /// Commits of it that exist on no remote.
    pub unpushed: u32,
    /// Paths in it that carry work no commit holds.
    pub uncommitted: u32,
    /// How far the base has moved under it, `None` when nothing could say.
    #[serde(default)]
    pub behind: Option<Behind>,
    /// What it occupies, `None` when nothing measured it. A locked worktree has no
    /// size, because nothing walked it.
    #[serde(default)]
    pub bytes: Option<u64>,
    /// Whether the size is a floor because an entry could not be read.
    #[serde(default)]
    pub partial: bool,
    /// When it was made, as far as the machine can say, `None` when nothing could.
    #[serde(default)]
    pub made_at: Option<Timestamp>,
    /// What is true of the row instead of a reading: `locked`, `prunable`, `not a
    /// checkout`. A row with one of these was deliberately not read further.
    #[serde(default)]
    pub note: Option<String>,
}

impl WorktreeRow {
    /// Whether the row holds something that exists nowhere else.
    ///
    /// This is the question every other column is subordinate to. A worktree that is
    /// done, small and old is still one a person must not lose, if this is true of it.
    #[must_use]
    pub const fn holds_unique_work(&self) -> bool {
        self.unpushed > 0 || self.uncommitted > 0
    }

    /// Whether removing this worktree would cost nothing: its work is on the base and
    /// it holds nothing of its own.
    #[must_use]
    pub const fn done_and_empty(&self) -> bool {
        self.done.is_integrated() && !self.holds_unique_work()
    }

    /// The size as a person reads it, with a mark when the figure is a floor.
    #[must_use]
    pub fn size(&self) -> String {
        match self.bytes {
            None => String::from(NONE),
            Some(bytes) if self.partial => format!("{}+", human::bytes(bytes)),
            Some(bytes) => human::bytes(bytes),
        }
    }

    /// What the row holds that exists nowhere else: `^3` commits no remote has, `*2`
    /// paths no commit holds.
    #[must_use]
    pub fn only_here(&self) -> String {
        let mut marks = Vec::new();
        if self.unpushed > 0 {
            marks.push(format!("^{}", self.unpushed));
        }
        if self.uncommitted > 0 {
            marks.push(format!("*{}", self.uncommitted));
        }
        if marks.is_empty() { String::from(NONE) } else { marks.join(" ") }
    }

    /// How far behind the row is, and what that was measured against.
    #[must_use]
    pub fn behind_cell(&self) -> String {
        match &self.behind {
            None => String::from(UNKNOWN),
            Some(behind) => format!("-{} ({})", behind.commits, behind.reference),
        }
    }

    /// What merging the row would do, or the reason nothing was read about it.
    ///
    /// A note wins. A locked worktree was not opened, so printing `unknown` there
    /// would say Git could not answer when the truth is that Nodal did not ask.
    #[must_use]
    pub fn done_cell(&self) -> String {
        match &self.note {
            Some(note) => note.clone(),
            None => self.done.label(),
        }
    }

    /// What the row was made for, cut to the width of the column.
    #[must_use]
    pub fn for_cell(&self) -> String {
        match &self.intent {
            None => String::from(NONE),
            Some(intent) => truncate(intent, INTENT_WIDTH),
        }
    }

    /// The age of the row, or the placeholder when nothing could date it.
    #[must_use]
    pub fn age_cell(&self, now: Timestamp) -> String {
        self.made_at.map_or_else(|| String::from(NONE), |made| human::span(now, made))
    }
}

/// One line of text, cut at `width` with a single character standing for the rest.
///
/// Cut on a character boundary and never inside one: a prompt is somebody's own words
/// and may hold any of them.
#[must_use]
pub fn truncate(text: &str, width: usize) -> String {
    let line = text.lines().next().unwrap_or(text).trim();
    if line.chars().count() <= width {
        return line.to_owned();
    }
    let kept: String = line.chars().take(width.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The verdict on one checkout: every worktree its repository names, and what removing
/// each would cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// The top of the checkout the command was run in.
    pub checkout: PathBuf,
    /// What the registry calls the project here, `None` when it holds no row for it.
    /// A verdict with `None` is the one printed without anything being initialised.
    #[serde(default)]
    pub project: Option<ProjectName>,
    /// The instant the reading was taken, which every age is measured from.
    pub now: Timestamp,
    /// The revision every row's `done` was measured against, `None` when this
    /// repository has no default branch to measure with.
    #[serde(default)]
    pub base: Option<String>,
    /// The rows, in the order [`crate::runtime::verdict::order`] put them.
    pub rows: Vec<WorktreeRow>,
    /// What a reading could not answer. A note is not a failure: one checkout that
    /// could not be opened must not cost the answer about the other eleven.
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Verdict {
    /// The rows that are worktrees: folders Nodal did not make.
    pub fn worktrees(&self) -> impl Iterator<Item = &WorktreeRow> {
        self.rows.iter().filter(|row| row.kind == RowKind::Worktree)
    }

    /// The closing line: how many worktrees could go, what they hold, and the
    /// statement that nothing was done about them.
    ///
    /// Only worktrees are counted. A unit is Nodal's own and has its own command for
    /// ending it, so putting one in this count would offer a person a saving they
    /// would take with the wrong tool.
    #[must_use]
    pub fn closing(&self) -> String {
        let removable: Vec<&WorktreeRow> =
            self.worktrees().filter(|row| row.done_and_empty()).collect();
        let bytes: u64 = removable.iter().filter_map(|row| row.bytes).sum();
        match removable.len() {
            0 => format!("no worktree here is done and empty. {CLOSING}"),
            1 => format!(
                "1 worktree is done and holds nothing unique: {}. {CLOSING}",
                human::bytes(bytes)
            ),
            count => format!(
                "{count} worktrees are done and hold nothing unique: {}. {CLOSING}",
                human::bytes(bytes)
            ),
        }
    }

    /// The line above the table: which checkout this is about, and whether Nodal holds
    /// a row for it.
    #[must_use]
    pub fn heading(&self) -> String {
        match &self.project {
            Some(project) => format!("{}  ({})", self.checkout.display(), project),
            None => {
                format!("{}  (no project of nodal's; nothing was written)", self.checkout.display())
            }
        }
    }
}

impl Render for Verdict {
    const KIND: &'static str = "verdict";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        doc.push(Block::line(self.heading()));
        doc.push(Block::blank());
        if self.rows.is_empty() {
            doc.push(Block::line("this checkout has no other worktrees."));
        } else {
            doc.push(Block::table(table(&self.rows, self.now)));
        }
        doc.push(Block::blank());
        doc.push(Block::line(self.closing()));
        for note in &self.notes {
            doc.push(Block::line(note.clone()));
        }
        doc
    }
}

/// The table the verdict prints.
pub(crate) fn table(rows: &[WorktreeRow], now: Timestamp) -> Table {
    let mut table = Table::new(&COLUMNS);
    for row in rows {
        table.push(vec![
            row.name.clone(),
            row.for_cell(),
            row.done_cell(),
            row.only_here(),
            row.behind_cell(),
            row.size(),
            row.age_cell(now),
        ]);
    }
    table
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::{Behind, RowKind, Verdict, WorktreeRow, truncate};
    use crate::git::integration::{Integration, Reason};
    use crate::model::Timestamp;

    fn row(name: &str, done: Integration, unpushed: u32, uncommitted: u32) -> WorktreeRow {
        WorktreeRow {
            kind: RowKind::Worktree,
            name: String::from(name),
            path: PathBuf::from("/tmp").join(name),
            branch: Some(String::from(name)),
            intent: None,
            done,
            unpushed,
            uncommitted,
            behind: None,
            bytes: Some(1_000_000_000),
            partial: false,
            made_at: None,
            note: None,
        }
    }

    fn verdict(rows: Vec<WorktreeRow>) -> Verdict {
        Verdict {
            checkout: PathBuf::from("/home/dev/project"),
            project: None,
            now: Timestamp::now(),
            base: Some(String::from("main")),
            rows,
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_closing_line_counts_only_worktrees_that_hold_nothing_of_their_own() {
        let done = Integration::Integrated(Reason::Absorbed);
        let rows = vec![
            row("clean-one", done, 0, 0),
            row("clean-two", done, 0, 0),
            row("done-but-dirty", done, 0, 4),
            row("done-but-unpushed", done, 2, 0),
            row("still-open", Integration::Open, 0, 0),
        ];
        let line = verdict(rows).closing();
        assert!(line.starts_with("2 worktrees are done and hold nothing unique: 2.0 GB"), "{line}");
        assert!(line.ends_with("nodal removed nothing."), "{line}");
    }

    #[test]
    fn one_removable_worktree_is_said_in_the_singular() {
        let line =
            verdict(vec![row("only", Integration::Integrated(Reason::Ancestor), 0, 0)]).closing();
        assert!(line.starts_with("1 worktree is done and holds nothing unique: 1.0 GB"), "{line}");
    }

    #[test]
    fn nothing_removable_is_said_and_the_promise_is_still_made() {
        let line = verdict(vec![row("busy", Integration::Open, 3, 0)]).closing();
        assert_eq!(line, "no worktree here is done and empty. nodal removed nothing.");
    }

    #[test]
    fn a_unit_row_is_never_counted_as_a_worktree() {
        let mut unit = row("verdict-1", Integration::Integrated(Reason::Ancestor), 0, 0);
        unit.kind = RowKind::Unit;
        let line = verdict(vec![unit]).closing();
        assert_eq!(line, "no worktree here is done and empty. nodal removed nothing.");
    }

    #[test]
    fn a_worktree_with_no_upstream_and_no_base_says_unknown_rather_than_zero() {
        assert_eq!(row("x", Integration::Open, 0, 0).behind_cell(), "unknown");
    }

    #[test]
    fn a_behind_count_carries_the_revision_it_was_measured_against() {
        let mut measured = row("x", Integration::Open, 0, 0);
        measured.behind =
            Some(Behind { commits: 12, reference: String::from("origin/main"), upstream: true });
        assert_eq!(measured.behind_cell(), "-12 (origin/main)");
    }

    #[test]
    fn a_note_is_printed_instead_of_a_verdict_that_was_never_asked_for() {
        let mut held = row("x", Integration::Unknown, 0, 0);
        held.note = Some(String::from("locked"));
        assert_eq!(held.done_cell(), "locked");
    }

    #[test]
    fn only_here_names_both_kinds_of_work_and_says_nothing_when_there_is_none() {
        assert_eq!(row("x", Integration::Open, 3, 2).only_here(), "^3 *2");
        assert_eq!(row("x", Integration::Open, 0, 2).only_here(), "*2");
        assert_eq!(row("x", Integration::Open, 0, 0).only_here(), "—");
    }

    #[test]
    fn an_intent_is_cut_at_a_character_and_never_inside_one() {
        let wide = "рефакторинг очень длинной подсистемы импорта данных из внешних систем";
        let cut = truncate(wide, 20);
        assert_eq!(cut.chars().count(), 20, "{cut}");
        assert!(cut.ends_with('…'), "{cut}");
    }

    #[test]
    fn an_intent_is_the_first_line_and_nothing_after_it() {
        assert_eq!(truncate("fix the importer\nthen the exporter", 48), "fix the importer");
    }
}
