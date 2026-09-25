//! The predicate's answer, read once into types.
//!
//! `nodal reclaim --check --json` is the public answer, so it is what the grid asks. The
//! document is parsed here and nowhere else, into the fields the comparison reads, and the
//! states are enums: a comparison that carried `"only_here"` around as text could compare
//! it with `"onlyhere"` and pass.
//!
//! The enums are deliberately closed. A disposition Nodal starts printing that this crate
//! does not name stops the grid with a parse error, which is the answer a reader wants: a
//! new disposition is a new rule, and a grid that skipped over it would go on reporting
//! agreement about a question it no longer understands.
//!
//! Only what the comparison needs is here. The evidence record, the trash path and the
//! instant are asserted by `tests/safety/tests/evidence_record.rs`, which is where they
//! belong; repeating them here would be a second, weaker copy of that suite.

use std::process::Output;

use serde::Deserialize;

/// The whole of the answer the comparison reads.
#[derive(Debug, Clone, Deserialize)]
pub struct Check {
    /// Whether a reclaim run now would go ahead.
    pub safe_to_reclaim: bool,
    /// The home's commits, grouped by where else they live.
    pub commits: Vec<Commits>,
    /// What the home holds that no commit does, grouped by what a reclaim would do to it.
    pub paths: Vec<Paths>,
    /// Why a person is needed, ranked.
    pub reasons: Vec<Reason>,
    /// What is running against the home, when that was read.
    pub runtime: Option<Runtime>,
}

/// One group of commits and where else they live.
#[derive(Debug, Clone, Deserialize)]
pub struct Commits {
    /// Where else, as the product's own tag.
    pub copies: Copies,
    /// How many commits are in the group.
    pub count: u64,
    /// Up to a few of them, for a report.
    #[serde(default)]
    pub sample: Vec<String>,
}

/// Where else a commit of this home lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Copies {
    /// Nowhere, and the remote question was settled.
    OnlyHere,
    /// Another object store on this machine holds it, and holds the work behind it.
    SecondLocalCopy,
    /// A dated reading of the remote reaches it.
    RemoteProved,
    /// Nothing proved a copy, and a reading that could have is missing.
    NotChecked,
}

impl Copies {
    /// Whether this disposition is a proven copy of the work.
    ///
    /// The two that are, and no third. The contract's own words: a store that exists is
    /// eligible, and only a store that reaches, is complete, owns its objects and is not
    /// the home is proven.
    #[must_use]
    pub const fn proves(self) -> bool {
        matches!(self, Self::SecondLocalCopy | Self::RemoteProved)
    }
}

/// One group of paths and what a reclaim would do to it.
#[derive(Debug, Clone, Deserialize)]
pub struct Paths {
    /// What the paths are to Git.
    pub held: Held,
    /// How many there are.
    pub count: u64,
}

/// What a path in the home is to Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Held {
    /// A tracked path that differs from `HEAD` or the index.
    Uncommitted,
    /// A path Git does not track and no ignore rule covers.
    Untracked,
    /// Ignored state no tool writes again.
    LocalState,
    /// Ignored state the exclusion table calls regenerable.
    Generated,
}

impl Held {
    /// Whether a path in this group is a member of the contracted loss set.
    ///
    /// The ignored halves are not. The contract states that in §1: an ignored path is
    /// environment, the report names it, and the trash keeps the kind nothing writes again.
    #[must_use]
    pub const fn in_the_loss_set(self) -> bool {
        matches!(self, Self::Uncommitted | Self::Untracked)
    }
}

/// One reason a person is needed.
#[derive(Debug, Clone, Deserialize)]
pub struct Reason {
    /// What is needed.
    pub needs: Needs,
    /// The words the report prints.
    pub detail: String,
}

/// What a reason asks of a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Work that may exist only here.
    UniqueLoss,
    /// Something Nodal did not start is standing in the home.
    BlockingRuntime,
    /// Nothing read the remote, so what it has is not known.
    UnknownEvidence,
    /// Merging would conflict, or the base has moved.
    Diverged,
    /// The unit is a person's to end.
    Review,
    /// Nothing.
    Nothing,
}

/// What is running against the home.
#[derive(Debug, Clone, Deserialize)]
pub struct Runtime {
    /// What is standing in the home that Nodal did not start.
    #[serde(default)]
    pub bystanders: Vec<serde_json::Value>,
    /// How many processes the host let the reading see.
    pub read: u64,
    /// How many it refused.
    pub withheld: u64,
}

impl Check {
    /// Ask the binary about one unit, and read the one document it prints.
    ///
    /// The exit code is asserted against the verdict here rather than in each caller. A
    /// `--check` whose exit code and whose `safe_to_reclaim` disagreed would make every
    /// script that gates on the code disagree with every script that gates on the field.
    ///
    /// # Panics
    ///
    /// If the command printed something that is not one JSON document, or if the code and
    /// the verdict disagree.
    #[must_use]
    pub fn read(asked: &Output) -> Self {
        let printed = nodal_safety::answer(asked);
        let read: Self = serde_json::from_str(&printed).unwrap_or_else(|why| {
            panic!(
                "--check --json is one document ({why}): {printed}{}",
                nodal_safety::stderr(asked)
            )
        });
        assert_eq!(
            read.safe_to_reclaim,
            asked.status.success(),
            "the exit code and the verdict disagree: {printed}"
        );
        read
    }

    /// Whether it refused over something standing in the home rather than over the work.
    #[must_use]
    pub fn blocked_by_occupancy(&self) -> bool {
        self.reasons.iter().any(|reason| reason.needs == Needs::BlockingRuntime)
    }
}
