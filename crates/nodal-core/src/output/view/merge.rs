//! What `nodal merge` answers with, before it runs and after.
//!
//! One type for both. The plan a person is shown and the report they are given are the
//! same document with the same fields, because they are two readings of one pipeline:
//! what the stages will do, and what they did. A person who agreed to a plan can compare
//! it with the answer line by line, and a script reads one shape either way.
//!
//! The field to read first is [`Merged::conflict`]. A rebase that stopped is not a
//! failure — the repository is in a state Git and a person both know — so it is a value
//! here, with the paths and the two commands that end it, rather than an error.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::lifecycle::hooks::Ran;
use crate::model::{Timestamp, Unit};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field, Table};
use crate::output::view::Reclaimed;

/// One stage of the pipeline as the report prints it: which stage, and one line about
/// what it will do or did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageLine {
    /// Which stage: `commit`, `squash`, `rebase`, `forward` or `remove`.
    pub name: String,
    /// What it will do, or what it did, in the words the report prints.
    pub detail: String,
}

impl StageLine {
    /// A line about the stage of a name.
    pub fn new(name: &str, detail: impl Into<String>) -> Self {
        Self { name: name.to_owned(), detail: detail.into() }
    }
}

/// A rebase that stopped, and the two ways out of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// The paths Git could not merge.
    pub paths: Vec<String>,
    /// The home the rebase is stopped in, which is where they are resolved.
    pub home: PathBuf,
    /// The command that carries on once they are resolved.
    pub resume: String,
    /// The command that puts the branch back where the merge found it.
    pub abort: String,
}

/// What one merge will do, or did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merged {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit's handle.
    pub slug: String,
    /// The branch the unit owns.
    pub branch: String,
    /// The branch the work merges into, when one was resolved.
    pub target: Option<String>,
    /// The ref that holds the branch as it was before the squash, while there is one.
    pub premerge: Option<String>,
    /// The stages, in the order they run.
    pub stages: Vec<StageLine>,
    /// The rebase that stopped, when one did. Nothing after it ran.
    pub conflict: Option<Conflict>,
    /// The hooks that ran, in the order they ran.
    pub hooks: Vec<Ran>,
    /// What the reclaim did, when the merge asked for one and it went through.
    pub reclaimed: Option<Box<Reclaimed>>,
    /// Why the reclaim refused, when it did. The merge itself is done either way.
    pub refused: Option<String>,
    /// Whether this is the plan rather than the answer.
    pub planned: bool,
}

impl Render for Merged {
    const KIND: &'static str = "merge";

    fn doc(&self) -> Doc {
        let mut doc = Doc::from_iter([Block::fields(self.heading()), Block::table(self.table())]);
        if let Some(conflict) = &self.conflict {
            doc.push(Block::blank());
            doc.push(Block::line(format!(
                "the rebase stopped in {}: {}",
                conflict.home.display(),
                names(&conflict.paths)
            )));
            doc.push(Block::line(format!("resolve them and run `{}`", conflict.resume)));
            doc.push(Block::line(format!("or run `{}` to put the branch back", conflict.abort)));
        }
        if let Some(why) = &self.refused {
            doc.push(Block::blank());
            doc.push(Block::line(format!("the unit was not removed: {why}")));
        }
        doc
    }
}

impl Merged {
    /// The plan, before a person has agreed to it.
    #[must_use]
    pub fn planned(
        subject: (&Unit, &str),
        stages: Vec<StageLine>,
        premerge: Option<String>,
    ) -> Self {
        let (unit, target) = subject;
        Self {
            now: Timestamp::now(),
            slug: unit.slug.to_string(),
            branch: unit.branch.to_string(),
            target: Some(target.to_owned()),
            premerge,
            stages,
            conflict: None,
            hooks: Vec::new(),
            reclaimed: None,
            refused: None,
            planned: true,
        }
    }

    /// What an abort did.
    #[must_use]
    pub fn aborted(unit: &Unit, stages: Vec<StageLine>) -> Self {
        Self {
            now: Timestamp::now(),
            slug: unit.slug.to_string(),
            branch: unit.branch.to_string(),
            target: None,
            premerge: None,
            stages,
            conflict: None,
            hooks: Vec::new(),
            reclaimed: None,
            refused: None,
            planned: false,
        }
    }

    /// Whether everything the merge was asked to do happened.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.conflict.is_none() && self.refused.is_none()
    }

    /// The lines above the table: which unit, which branches, and what is kept.
    fn heading(&self) -> Vec<Field> {
        let mut fields = vec![
            Field::new(if self.planned { "plan" } else { "merge" }, self.slug.clone()),
            Field::new("branch", self.headline()),
        ];
        if let Some(reference) = &self.premerge {
            fields.push(Field::new("kept", format!("{reference} holds the branch as it was")));
        }
        if !self.hooks.is_empty() {
            let phases: Vec<String> = self.hooks.iter().map(|ran| ran.phase.to_string()).collect();
            fields.push(Field::new("hooks", names(&phases)));
        }
        fields
    }

    /// The branch, and the branch it goes into when one was resolved.
    fn headline(&self) -> String {
        match &self.target {
            Some(target) => format!("{} into {target}", self.branch),
            None => self.branch.clone(),
        }
    }

    /// One row per stage.
    fn table(&self) -> Table {
        let mut table = Table::new(&["stage", "what"]);
        for stage in &self.stages {
            table.push(vec![stage.name.clone(), stage.detail.clone()]);
        }
        table
    }
}

/// Join what a line lists, in the one form this document uses.
fn names(items: &[String]) -> String {
    items.join(", ")
}
