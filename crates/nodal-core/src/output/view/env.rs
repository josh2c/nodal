//! What a home is activated with: the names, where each came from, and what is missing.
//!
//! Names and origins only. A value never reaches this type, so neither `--json` nor the
//! human form can print one, and `nodal env --export` — the one rendering that does
//! carry values — is not a read type at all ([`crate::env::files::export`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::manifest::{Manifest, Missing, Origin, Want};
use crate::model::{EnvName, Slug, Timestamp};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, Table};

/// One variable of an activated home, as a report may speak of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarLine {
    /// The name.
    pub name: EnvName,
    /// Who supplied the value.
    pub origin: Origin,
}

/// The answer to `nodal env`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvReport {
    /// When the report was taken.
    pub now: Timestamp,
    /// The home it is about.
    pub home: PathBuf,
    /// The unit's handle.
    pub unit: Slug,
    /// Every name the home carries, in the order `.nodal/env` lists them.
    pub vars: Vec<VarLine>,
    /// Every declared name no source answered.
    pub missing: Vec<Missing>,
}

impl EnvReport {
    /// The report a home's manifest describes.
    #[must_use]
    pub fn from_manifest(manifest: &Manifest, now: Timestamp) -> Self {
        Self {
            now,
            home: manifest.home.clone(),
            unit: manifest.slug.clone(),
            vars: manifest
                .env
                .iter()
                .map(|(name, origin)| VarLine { name: name.clone(), origin: *origin })
                .collect(),
            missing: manifest.missing.clone(),
        }
    }
}

impl Render for EnvReport {
    const KIND: &'static str = "environment report";

    fn doc(&self) -> Doc {
        let mut doc = Doc::from_iter([Block::fields(vec![
            Field::new("unit", self.unit.to_string()),
            Field::new("home", self.home.display().to_string()),
        ])]);
        doc.push(Block::blank());
        doc.push(Block::table(vars_table(&self.vars)));
        if self.missing.is_empty() {
            return doc;
        }
        doc.push(Block::blank());
        doc.push(Block::line("missing"));
        doc.push(Block::table(missing_table(&self.missing)).at(2));
        doc
    }
}

/// One row per variable: the name and where its value came from.
fn vars_table(vars: &[VarLine]) -> Table {
    let mut table = Table::new(&["name", "from"]);
    for var in vars {
        table.push(vec![var.name.to_string(), String::from(origin(var.origin))]);
    }
    table
}

/// One row per name nothing answered, and what would answer it.
fn missing_table(missing: &[Missing]) -> Table {
    let mut table = Table::new(&["name", "wanted from"]);
    for line in missing {
        table.push(vec![line.name.to_string(), String::from(want(line.want))]);
    }
    table
}

/// What an origin is called in a report.
const fn origin(origin: Origin) -> &'static str {
    match origin {
        Origin::Identity => "nodal",
        Origin::Generated => "this unit",
        Origin::Machine => "this machine",
    }
}

/// What would answer a missing name, in the words a person can act on.
const fn want(want: Want) -> &'static str {
    match want {
        Want::Generated => "a service of this unit",
        Want::Secret | Want::RequiredLocal => "~/.nodal/secrets.env",
    }
}
