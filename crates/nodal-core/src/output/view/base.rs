//! The warm bases a project keeps, as `nodal base ls` prints them.

use serde::{Deserialize, Serialize};

use crate::model::{Base, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, NONE, Table};

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
