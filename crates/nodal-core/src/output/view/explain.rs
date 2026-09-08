//! Why a unit is the way it is: what `nodal explain` answers with.
//!
//! `nodal show` says what a unit *is*. This says how it came to be that, and it exists
//! because every one of those answers is otherwise buried in a decision nobody watched
//! being made: which tree the home was cloned from and why that one, what the clone left
//! behind, what was removed from the copy afterwards, and where the ports came from.
//!
//! Nothing here is computed. Every line is read back out of the registry — the
//! environment's base, the unit's log, the project's port block — so what a person is
//! told is what was actually recorded at the time rather than what the same code would
//! decide today.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{BaseId, CommitId, Platform, Slug, Timestamp, WorkspaceFp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, NONE, Table};

/// How many characters of a fingerprint are shown. Enough to tell two apart in a
/// listing, which is all a person does with one.
const FINGERPRINT_WIDTH: usize = 12;

/// Where a unit's home came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "origin")]
pub enum Origin {
    /// The checkout was already there and Nodal adopted it where it stood.
    Adopted {
        /// Whether the directory carries work Nodal never made: always true here, and
        /// stated because it is the reason the home is never moved.
        root: bool,
    },
    /// The home is a copy-on-write clone of a base.
    Cloned {
        /// Which base.
        base: BaseId,
        /// Where that base is.
        path: PathBuf,
        /// The workspace key it is warm for, which is why this base and not another.
        fingerprint: WorkspaceFp,
        /// The platform it was built on; a base is not warm on another.
        platform: Platform,
        /// The commit its tree was put at.
        commit: CommitId,
        /// When the build finished.
        built_at: Timestamp,
    },
    /// The environment names a base this registry no longer has, or names none at all.
    Unrecorded {
        /// What is known, in words.
        why: String,
    },
}

/// One path a clone left out, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusion {
    /// The path, relative to the tree.
    pub path: String,
    /// Why a home does not receive it.
    pub reason: String,
    /// Who decided: `nodal` for the built-in table, `project` for `base.exclude`.
    pub decided_by: String,
}

/// One cache a relocation removed after the clone was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invalidation {
    /// When it happened.
    pub at: Timestamp,
    /// How many caches went.
    pub removed: String,
    /// The path the content had been made at.
    pub from: String,
    /// What was done, in the words the relocation recorded.
    pub body: String,
}

/// One port the unit holds, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortLine {
    /// What the recipe calls it.
    pub name: String,
    /// The port itself.
    pub port: u16,
    /// Where it came from, in words.
    pub source: String,
}

/// Why one unit is as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explained {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// Which unit.
    pub slug: Slug,
    /// Where its home is, when it has one.
    pub home: Option<PathBuf>,
    /// Where that home came from.
    pub origin: Origin,
    /// What the clone left out. Empty for a checkout adopted in place, because nothing
    /// was copied and so nothing was left out.
    pub excluded: Vec<Exclusion>,
    /// What was removed from the copy after it was made.
    pub invalidated: Vec<Invalidation>,
    /// The ports the unit holds.
    pub ports: Vec<PortLine>,
}

impl Render for Explained {
    const KIND: &'static str = "explanation";

    fn doc(&self) -> Doc {
        let mut doc = Doc::from_iter([Block::fields(self.heading())]);
        doc.push(Block::blank());
        doc.push(Block::line("what the home did not receive").at(1));
        doc.push(section(exclusion_table(&self.excluded), self.nothing_excluded()));
        doc.push(Block::blank());
        doc.push(Block::line("what was removed from it after it was made").at(1));
        doc.push(section(invalidation_table(&self.invalidated, self.now), NOTHING_INVALIDATED));
        doc.push(Block::blank());
        doc.push(Block::line("where the ports came from").at(1));
        doc.push(section(port_table(&self.ports), NOTHING_GRANTED));
        doc
    }
}

impl Explained {
    /// The first block: which unit, where it is, and where it came from.
    fn heading(&self) -> Vec<Field> {
        let mut fields = vec![
            Field::new("unit", self.slug.to_string()),
            Field::new(
                "home",
                self.home
                    .as_ref()
                    .map_or_else(|| String::from(NONE), |home| home.display().to_string()),
            ),
        ];
        fields.extend(origin_fields(&self.origin, self.now));
        fields
    }

    /// Why there is nothing to list under the exclusions, which is different for an
    /// adopted checkout and for a home whose recipe excluded nothing.
    fn nothing_excluded(&self) -> &'static str {
        match self.origin {
            Origin::Adopted { .. } => {
                "nothing was copied, so nothing was left out: the checkout was already here"
            }
            Origin::Cloned { .. } | Origin::Unrecorded { .. } => {
                "the clone left nothing out; the whole tree was copied"
            }
        }
    }
}

/// What is said where a table would be empty.
const NOTHING_INVALIDATED: &str = "no cache was removed: nothing in the home recorded a \
                                   path other than its own";

/// The same, for a unit that holds no port.
const NOTHING_GRANTED: &str = "no port is granted to this unit";

/// A table, or one line saying why there is none.
fn section(table: Table, empty: &str) -> Block {
    if table.is_empty() { Block::line(empty).at(2) } else { Block::table(table).at(2) }
}

/// Where the home came from, as fields.
fn origin_fields(origin: &Origin, now: Timestamp) -> Vec<Field> {
    match origin {
        Origin::Adopted { .. } => vec![
            Field::new("origin", String::from("adopted where it stands")),
            Field::new(
                "why",
                String::from(
                    "the checkout was already on this machine, so Nodal registered it \
                     rather than copying it; a reclaim unregisters it and never moves it",
                ),
            ),
        ],
        Origin::Cloned { base, path, fingerprint, platform, commit, built_at } => vec![
            Field::new("origin", format!("a clone of base {base}")),
            Field::new(
                "why",
                format!(
                    "it is the base warm for this workspace ({key}) on {platform}, built \
                     at commit {commit}",
                    key = short(fingerprint.0.as_str()),
                    commit = short(&commit.to_string())
                ),
            ),
            Field::new(
                "base",
                format!("{} · built {}", path.display(), human::since(now, *built_at)),
            ),
        ],
        Origin::Unrecorded { why } => vec![Field::new("origin", why.clone())],
    }
}

/// The columns of the exclusion table.
const EXCLUSION_COLUMNS: [&str; 3] = ["path", "decided by", "why"];

/// What a clone left out.
fn exclusion_table(exclusions: &[Exclusion]) -> Table {
    let mut table = Table::new(&EXCLUSION_COLUMNS);
    for exclusion in exclusions {
        table.push(vec![
            exclusion.path.clone(),
            exclusion.decided_by.clone(),
            exclusion.reason.clone(),
        ]);
    }
    table
}

/// The columns of the invalidation table.
const INVALIDATION_COLUMNS: [&str; 3] = ["when", "removed", "made at"];

/// What a relocation removed.
fn invalidation_table(invalidations: &[Invalidation], now: Timestamp) -> Table {
    let mut table = Table::new(&INVALIDATION_COLUMNS);
    for invalidation in invalidations {
        table.push(vec![
            human::since(now, invalidation.at),
            invalidation.removed.clone(),
            invalidation.from.clone(),
        ]);
    }
    table
}

/// The columns of the port table.
const PORT_COLUMNS: [&str; 3] = ["name", "port", "from"];

/// The ports a unit holds.
fn port_table(ports: &[PortLine]) -> Table {
    let mut table = Table::new(&PORT_COLUMNS);
    for line in ports {
        table.push(vec![line.name.clone(), line.port.to_string(), line.source.clone()]);
    }
    table
}

/// The first few characters of a digest, which is what a person compares.
fn short(text: &str) -> String {
    text.chars().take(FINGERPRINT_WIDTH).collect()
}
