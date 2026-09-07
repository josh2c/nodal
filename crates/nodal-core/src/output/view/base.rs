//! The warm bases a project keeps, as `nodal base ls` prints them.

use serde::{Deserialize, Serialize};

use crate::model::{Base, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, NONE, Table};

/// One base, with what is true of it now rather than what the registry stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseRow {
    /// The record itself.
    pub base: Base,
    /// How many units hold this base against eviction.
    pub pins: u32,
    /// What it occupies, when it has been measured.
    pub disk_bytes: Option<u64>,
}

/// Every base on this machine for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseList {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The bases, in the order the producer chose.
    pub bases: Vec<BaseRow>,
}

impl Render for BaseList {
    const KIND: &'static str = "base list";

    fn doc(&self) -> Doc {
        if self.bases.is_empty() {
            return Doc::from_iter([Block::line("no base built yet")]);
        }
        let mut table =
            Table::new(&["base", "workspace", "platform", "commit", "pins", "disk", "used"]);
        for row in &self.bases {
            table.push(vec![
                short(&row.base.id.to_string()),
                short(&row.base.ws_fingerprint.0.to_string()),
                row.base.platform.to_string(),
                short(&row.base.commit.to_string()),
                row.pins.to_string(),
                row.disk_bytes.map_or_else(|| String::from(NONE), human::bytes),
                human::since(self.now, row.base.last_used),
            ]);
        }
        Doc::from_iter([Block::table(table)])
    }
}

/// The leading characters of an identifier, which is what a person compares by. Full
/// values stay in the JSON rendering, so nothing is lost to a tool.
fn short(value: &str) -> String {
    value.chars().take(8).collect()
}

/// What `nodal base build` did: the base a workspace now has, and how it got it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseBuild {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The base, with what is true of it now.
    pub base: BaseRow,
    /// Whether this invocation built it, rather than finding it warm.
    pub built: bool,
    /// Where its content came from, when this invocation built it.
    pub origin: Option<String>,
}

impl Render for BaseBuild {
    const KIND: &'static str = "base build";

    fn doc(&self) -> Doc {
        let verb = if self.built { "built" } else { "already warm" };
        let mut fields = vec![
            Field::new("base", self.base.base.id.to_string()),
            Field::new("workspace", self.base.base.ws_fingerprint.0.to_string()),
            Field::new("platform", self.base.base.platform.to_string()),
            Field::new("commit", self.base.base.commit.to_string()),
            Field::new("path", self.base.base.path.display().to_string()),
            Field::new("state", verb),
        ];
        if let Some(origin) = &self.origin {
            fields.push(Field::new("from", origin.clone()));
        }
        Doc::from_iter([Block::fields(fields)])
    }
}

/// What `nodal base gc` removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseSweep {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// How many idle bases the project was told to keep.
    pub keep: usize,
    /// The bases removed, in the order they were removed.
    pub removed: Vec<Base>,
}

impl Render for BaseSweep {
    const KIND: &'static str = "base sweep";

    fn doc(&self) -> Doc {
        if self.removed.is_empty() {
            return Doc::from_iter([Block::line("no base to remove")]);
        }
        let mut table = Table::new(&["removed", "workspace", "path"]);
        for base in &self.removed {
            table.push(vec![
                short(&base.id.to_string()),
                short(&base.ws_fingerprint.0.to_string()),
                base.path.display().to_string(),
            ]);
        }
        Doc::from_iter([Block::table(table)])
    }
}
