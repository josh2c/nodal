//! What `nodal uninstall` and `nodal upgrade` answer with.
//!
//! Both are one value with two renderings, for the reason every command here is: a
//! script reads the fields a person reads.
//!
//! [`Uninstall`] is the same value twice. The survey answers with it before anything is
//! removed, and the apply answers with it again with [`Uninstall::applied`] set. So the
//! summary a person agrees to is the list that is then acted on, and neither half can
//! describe something the other did not do.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::lifecycle::uniqueness::{Finding, Uniqueness};
use crate::model::Timestamp;
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, Table};
use crate::setup::channel::{Channel, Install};

/// What kind of thing an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The marked block in a shell's start-up file.
    RcBlock,
    /// A shell script in the state directory.
    Shim,
    /// The state directory itself.
    State,
    /// The hooks in one project's `.claude/settings.json`.
    ClaudeHooks,
}

impl Kind {
    /// The word the report prints for this kind.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RcBlock => "rc block",
            Self::Shim => "shim",
            Self::State => "state",
            Self::ClaudeHooks => "claude hooks",
        }
    }
}

/// One thing an uninstall would take away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// What kind of thing it is.
    pub kind: Kind,
    /// Where it is.
    pub path: PathBuf,
    /// What a person needs to know about it before they agree: which shell a block
    /// belongs to, how many homes a state directory holds.
    pub detail: String,
}

/// What an uninstall would do, or has done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Uninstall {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// Everything that would go, in the order it goes.
    pub items: Vec<Item>,
    /// The homes that hold work nothing else has. Empty is the ordinary case. Anything
    /// here stops the uninstall unless `--force` was given, and is printed either way.
    pub findings: Vec<Uniqueness>,
    /// Whether `--force` accepted the findings above.
    pub forced: bool,
    /// Whether the plan has been applied.
    pub applied: bool,
    /// What could not be read, and why. A note is not a failure; it is the difference
    /// between "no home holds work only it has" and "I could not look".
    pub notes: Vec<String>,
}

impl Render for Uninstall {
    const KIND: &'static str = "uninstall";

    fn doc(&self) -> Doc {
        let mut blocks = vec![Block::fields(vec![Field::new(
            if self.applied { "removed" } else { "removes" },
            self.summary(),
        )])];
        if !self.items.is_empty() {
            blocks.push(Block::table(self.items_table()));
        }
        if !self.findings.is_empty() {
            blocks.push(Block::table(self.findings_table()));
            blocks.push(Block::line(self.work_line()));
        }
        for note in &self.notes {
            blocks.push(Block::line(note.clone()));
        }
        Doc::from_iter(blocks)
    }
}

impl Uninstall {
    /// Whether the plan can run: nothing to lose, or a `--force` that accepted losing
    /// it.
    #[must_use]
    pub fn is_permitted(&self) -> bool {
        self.forced || self.findings.is_empty()
    }

    /// Whether there is anything to do at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many things of each kind, in one line.
    fn summary(&self) -> String {
        if self.items.is_empty() {
            return String::from("nothing; nodal has installed nothing on this machine");
        }
        let counts = [Kind::RcBlock, Kind::Shim, Kind::ClaudeHooks, Kind::State].map(|kind| {
            let count = self.items.iter().filter(|item| item.kind == kind).count();
            (kind, count)
        });
        let named: Vec<String> = counts
            .iter()
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| format!("{count} {}", kind.label()))
            .collect();
        named.join(crate::output::human::JOIN)
    }

    /// One row per thing that goes: what it is, where it is, and what a person needs to
    /// know about it before they agree.
    fn items_table(&self) -> Table {
        let mut table = Table::new(&["item", "path", "detail"]);
        for item in &self.items {
            table.push(vec![
                item.kind.label().to_owned(),
                item.path.display().to_string(),
                item.detail.clone(),
            ]);
        }
        table
    }

    /// One row per home that holds work nothing else has.
    fn findings_table(&self) -> Table {
        let mut table = Table::new(&["home", "only here"]);
        for answer in &self.findings {
            table.push(vec![
                answer.home.display().to_string(),
                Finding::summarise(&answer.findings),
            ]);
        }
        table
    }

    /// The line under that table: what will happen about it.
    fn work_line(&self) -> String {
        if self.forced {
            return String::from("--force was given, so this work goes with the state directory");
        }
        String::from("nothing was done; commit or push this work, or pass --force")
    }
}

/// What `nodal shell-init --install` answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    /// The shell the integration was written for.
    pub shell: String,
    /// The script that was written, under the state directory.
    pub shim: PathBuf,
    /// The start-up file the block is in.
    pub rc: PathBuf,
    /// Whether this run added the block. `false` means the file already held one, which
    /// is what a second install on one machine reports.
    pub added: bool,
}

impl Render for Installed {
    const KIND: &'static str = "shell integration";

    /// No volatile field: two installs of one machine are the same document.
    const VOLATILE: &'static [&'static str] = &[];

    fn doc(&self) -> Doc {
        let state = if self.added { "added" } else { "already there" };
        Doc::from_iter([
            Block::fields(vec![
                Field::new("shell", self.shell.clone()),
                Field::new("script", self.shim.display().to_string()),
                Field::new("start-up file", format!("{} ({state})", self.rc.display())),
            ]),
            Block::line(String::from(
                "open a new shell to load it; `nodal uninstall` removes both again",
            )),
        ])
    }
}

/// What `nodal upgrade` and `nodal update` answer with.
///
/// One value, no clock and no network: where this binary is, what put it there, and the
/// command that replaces it. Nodal fetches nothing and compares no version (DL-034).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upgrade {
    /// The version that is running.
    pub version: String,
    /// Where the binary is, what installed it, and what upgrades it.
    #[serde(flatten)]
    pub install: Install,
}

impl Render for Upgrade {
    const KIND: &'static str = "upgrade";

    /// No volatile field: two readings of one machine are the same document.
    const VOLATILE: &'static [&'static str] = &[];

    fn doc(&self) -> Doc {
        Doc::from_iter([
            Block::fields(vec![
                Field::new("version", self.version.clone()),
                Field::new("binary", self.install.path.display().to_string()),
                Field::new("installed by", channel_label(self.install.channel).to_owned()),
                Field::new("upgrade with", self.install.command.clone()),
            ]),
            Block::line(String::from(
                "nodal fetches nothing and updates nothing itself; the command above does it",
            )),
        ])
    }
}

/// What one channel is called in the report.
const fn channel_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Cargo => "cargo",
        Channel::Homebrew => "homebrew",
        Channel::System => "a system package",
        Channel::Binary => "a binary placed by hand",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Item, Kind, Uninstall, Upgrade};
    use crate::model::Timestamp;
    use crate::output::Render;
    use crate::setup::channel::{self, Channel};

    fn at() -> Timestamp {
        Timestamp::parse("2026-09-07T09:00:00Z").unwrap()
    }

    fn plan(items: Vec<Item>) -> Uninstall {
        Uninstall {
            now: at(),
            items,
            findings: Vec::new(),
            forced: false,
            applied: false,
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_machine_with_nothing_on_it_is_told_so_rather_than_shown_an_empty_table() {
        let lines = plan(Vec::new()).doc().lines().join("\n");
        assert!(lines.contains("nodal has installed nothing"), "{lines}");
    }

    #[test]
    fn every_item_is_named_before_anything_is_removed() {
        let items = vec![
            Item {
                kind: Kind::RcBlock,
                path: PathBuf::from("/home/dev/.bashrc"),
                detail: String::from("the bash block, and nothing else in the file"),
            },
            Item {
                kind: Kind::State,
                path: PathBuf::from("/home/dev/.nodal"),
                detail: String::from("the registry and 2 unit homes"),
            },
        ];
        let lines = plan(items).doc().lines().join("\n");
        assert!(lines.contains("removes"), "the summary is in the future tense: {lines}");
        assert!(lines.contains("/home/dev/.bashrc"), "{lines}");
        assert!(lines.contains("2 unit homes"), "{lines}");
    }

    #[test]
    fn a_home_that_holds_work_nothing_else_has_stops_the_uninstall() {
        let mut answer = plan(Vec::new());
        answer.findings.push(crate::lifecycle::uniqueness::Uniqueness {
            home: PathBuf::from("/home/dev/.nodal/acme/e/01J8Z6H0"),
            findings: vec![crate::lifecycle::uniqueness::Finding::Untracked {
                count: 1,
                sample: vec![PathBuf::from("notes.txt")],
            }],
        });
        assert!(!answer.is_permitted());
        let lines = answer.doc().lines().join("\n");
        assert!(lines.contains("nothing was done"), "{lines}");
        assert!(lines.contains("notes.txt"), "{lines}");
    }

    #[test]
    fn a_forced_uninstall_says_what_it_accepted_losing() {
        let mut answer = plan(Vec::new());
        answer.forced = true;
        answer.findings.push(crate::lifecycle::uniqueness::Uniqueness {
            home: PathBuf::from("/home/dev/.nodal/acme/e/01J8Z6H0"),
            findings: vec![crate::lifecycle::uniqueness::Finding::Untracked {
                count: 1,
                sample: vec![PathBuf::from("notes.txt")],
            }],
        });
        assert!(answer.is_permitted());
        assert!(answer.doc().lines().join("\n").contains("--force was given"));
    }

    #[test]
    fn an_upgrade_names_the_channel_and_the_one_command_that_upgrades_it() {
        let install =
            channel::describe(std::path::Path::new("/home/dev/.cargo/bin/nodal"), None, None);
        assert_eq!(install.channel, Channel::Cargo);
        let report = Upgrade { version: String::from("0.1.0"), install };
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("cargo"), "{lines}");
        assert!(lines.contains("cargo install nodal --force"), "{lines}");
        assert!(lines.contains("fetches nothing"), "{lines}");
    }
}
