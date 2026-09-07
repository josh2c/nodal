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

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Timestamp;
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, Table};

/// The columns of the section for this project.
const HERE: [&str; 5] = ["what", "kind", "size", "state", "intent"];

/// The columns of the section for other projects. Names and sizes, and no more.
const ELSEWHERE: [&str; 3] = ["what", "kind", "size"];

/// How many characters of a recovered intent a row shows.
const INTENT_WIDTH: usize = 56;

/// The line that closes every report, in both renderings.
pub const CLOSING: &str = "nodal read this machine. it removed nothing and moved nothing.";

/// What kind of thing a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A checkout inside another checkout, which another tool made.
    NestedWorktree,
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
            Self::NestedWorktree => "nested worktree",
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
    /// What is true of it, in the words a report uses: `unmerged 3`, `dirty 12`,
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
    /// What belongs to this project, largest first.
    pub here: Vec<Finding>,
    /// What belongs to another project, largest first. Names and sizes only.
    pub elsewhere: Vec<Finding>,
    /// What a source could not read.
    pub notes: Vec<Note>,
}

impl Render for Doctor {
    const KIND: &'static str = "doctor";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        doc.push(Block::fields(vec![Field::new("this project", self.subject())]).at(0));
        doc.push(section(&self.here, &HERE, "nothing of this project is left behind"));
        doc.push(Block::blank());
        doc.push(
            Block::fields(vec![Field::new("not this project", "other projects' leftovers")]).at(0),
        );
        doc.push(section(&self.elsewhere, &ELSEWHERE, "nothing of another project is here"));
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
    use super::{CLOSING, Checkout, Doctor, Finding, Kind, Note, shorten};
    use crate::model::Timestamp;
    use crate::output::Render;

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
                Finding::new(Kind::NestedWorktree, ".claude/worktrees/auth")
                    .sized(7_850_000_000, true)
                    .says("unmerged 3")
                    .says("dirty 12"),
                Finding::new(Kind::NestedWorktree, ".claude/worktrees/held").says("locked"),
            ],
            elsewhere: vec![
                Finding::new(Kind::StaleCache, "/home/j/other/.next").sized(3_330_000_000, true),
            ],
            notes: vec![Note {
                source: String::from("docker"),
                why: String::from("docker is not installed"),
            }],
        }
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
