//! What a verdict rests on: the positive record of what was read, beside the verdict
//! itself.
//!
//! A `nodal reclaim --check` that says safe used to print `commits: []`, `reasons: []`,
//! `paths: []`. That is a statement that nothing objected. It is not a statement of what
//! was looked at, and the two are different claims about a machine: a home with nothing in
//! it and a home whose work was silently outside what the predicate walks produce the same
//! bytes. So a contract of "cannot see, therefore not safe" could not be falsified from
//! the output, because the output never said what it saw.
//!
//! This is what it saw. Every verdict carries one, safe or not, and **nothing in it
//! changes the verdict** — [`crate::lifecycle::kernel::judge`] is the only maker of a safe
//! answer, and a reader that gated on a field of this record would be taking a second
//! opinion from something that never decided anything. What it does is make the first one
//! checkable: which object stores were asked and which answered, which refs of the home
//! were walked, and what was not checked at all and why.
//!
//! ## What is not here, because the kernel already names it
//!
//! The instant of the reading is [`crate::lifecycle::kernel::Evidence::read_at`], and the
//! reading of the process table is [`crate::lifecycle::kernel::Evidence::runtime`] — the
//! [`crate::lifecycle::assess::Runtime`] itself, which carries how many processes were
//! read, how many the host refused, and which readings of occupancy this host answers. A
//! second copy of either here would be a second place for one fact to drift in. The stores
//! a *safe* verdict rests on are [`crate::lifecycle::kernel::Proof::rests_on`]; what this
//! record adds is the store that was **asked and held nothing**, which no proof mentions
//! because no proof rests on it.
//!
//! ## Why "not checked" is a field and not a silence
//!
//! Three readings are switched off for a reclaim that is only deciding whether to refuse
//! ([`crate::lifecycle::assess::Input`]), because each costs processes and none of them
//! changes the answer. That is a good trade and it is invisible in the output, which is
//! the problem: a person reading a verdict cannot tell a group that is empty from a group
//! nobody asked for. [`Unchecked`] says which, and why.
//!
//! ## Where it is kept
//!
//! `nodal reclaim --check` prints it and writes nothing at all — the command is a reading
//! and `tests/safety/verdict_writes_nothing.rs` holds it to that. An **executed** reclaim
//! writes it into the unit's event log as an [`crate::model::EventKind::Verdict`] event,
//! so that "what did the last reclaim decide, and on what" has an answer afterwards. The
//! event carries the summary; the whole record is what `--check --json` prints, because an
//! event reference is one line of text and this is a document.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Everything one verdict was taken from, beside the readings the kernel names itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reading {
    /// The object stores a copy of a commit was looked for in, and what each said.
    pub stores: Vec<Store>,
    /// What was walked inside the home itself.
    pub refs: Refs,
    /// What was not read, and why. Never a failure; the reasons are as often "nothing
    /// asked for it" as "it would not answer".
    pub not_checked: Vec<Unchecked>,
}

impl Reading {
    /// The one line a report leads with: how much of what was asked for came back.
    #[must_use]
    pub fn summary(&self) -> String {
        let answered = self.stores.iter().filter(|store| store.answered == Answered::Yes).count();
        format!(
            "{answered}/{} stores answered, {} refs walked",
            self.stores.len(),
            self.refs.walked.len()
        )
    }
}

/// One repository a copy of a commit was looked for in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Store {
    /// Where it is.
    pub path: PathBuf,
    /// What it is to this home.
    pub role: Role,
    /// What it said.
    pub answered: Answered,
    /// Why, for an answer that needs one.
    pub why: Option<String>,
}

/// What a store is to the home being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The project's own checkout.
    Checkout,
    /// Another repository found on this disk beside it.
    Sibling,
}

/// Whether a store answered the question it was asked.
///
/// Three answers and not two, because "it was not asked" is not "it said nothing". A
/// store is skipped once every commit is accounted for, which is a reading that finished
/// early rather than a reading that failed, and a record that folded the two together
/// would report a healthy machine as a broken one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Answered {
    /// It was asked and it answered.
    Yes,
    /// It was asked and it did not answer: not a repository, unreadable, or its own
    /// reading failed. A store that does not answer holds nothing, which is the strict
    /// direction.
    No,
    /// It was not asked.
    NotAsked,
}

/// What was walked inside the home.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refs {
    /// The refs the assessed set of commits was taken from.
    pub walked: Vec<String>,
    /// How many commits that came to.
    pub commits: usize,
    /// The refs that were not walked, named rather than left out, because a commit on one
    /// of them is work this verdict says nothing about.
    pub not_walked: Vec<String>,
}

/// One reading that was not made, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Unchecked {
    /// What was not read.
    pub what: String,
    /// Why not, in one line a person can act on.
    pub why: String,
}

impl Unchecked {
    /// One reading that was not made.
    pub fn new(what: impl Into<String>, why: impl Into<String>) -> Self {
        Self { what: what.into(), why: why.into() }
    }
}
