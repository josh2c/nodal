//! What `nodal init` did, and what it left for a person.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::output::Render;
use crate::output::human::{Block, Doc, Field};
use crate::recipe::InitPlan;
use crate::recipe::change::{Change, Edit};
use crate::recipe::gap::Gap;

/// The recipe that was written or proposed, and the questions it carries.
///
/// The field order is the JSON `nodal init --json` has always answered with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitReport {
    /// The file that was written, or would be.
    pub path: PathBuf,
    /// Whether a recipe was already there.
    pub existed: bool,
    /// The exact contents, so a caller can write the file itself.
    pub contents: String,
    /// The keys inference could not answer.
    pub gaps: Vec<Gap>,
    /// Every line writing this recipe changes in the file that is there now.
    ///
    /// The render writes the template's comments, so a rewrite keeps every key a person
    /// set and keeps none of the notes they wrote around them. A person is told which
    /// lines those are, by the command that is about to drop them, rather than by
    /// `git diff` afterwards.
    ///
    /// Empty for a project with no recipe yet, because nothing there is being changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<Change>,
}

impl InitReport {
    /// The report for a plan.
    #[must_use]
    pub fn from_plan(plan: &InitPlan) -> Self {
        Self {
            path: plan.path.clone(),
            existed: plan.existed(),
            contents: plan.contents.clone(),
            gaps: plan.gaps.clone(),
            changes: if plan.existed() { plan.changes() } else { Vec::new() },
        }
    }
}

impl Render for InitReport {
    const KIND: &'static str = "init report";

    fn doc(&self) -> Doc {
        let wrote = if self.existed { "rewrote" } else { "wrote" };
        let mut fields = vec![Field::new(wrote, self.path.display().to_string())];
        if self.gaps.is_empty() {
            fields.push(Field::new("ready", "every key was inferred"));
        } else {
            fields.push(Field::new("needs you", questions(&self.gaps)));
        }
        Doc::from_iter([Block::fields(fields)])
    }
}

impl InitReport {
    /// The changed lines, as the warning a person reads before the file is written.
    ///
    /// Lines rather than a [`Doc`], because this is not the command's answer. The answer
    /// on standard output is one document about a file that now exists; this is what a
    /// person needs in front of them while the file still says what they wrote, so it
    /// goes to standard error and it goes first (`nodal_cli::commands::init`).
    ///
    /// Empty where nothing changes, so a rewrite that writes the same bytes says
    /// nothing.
    #[must_use]
    pub fn warning(&self) -> Vec<String> {
        if self.changes.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![headline(&self.changes)];
        lines.extend(self.changes.iter().map(line_of));
        lines
    }
}

/// What the block of changed lines is introduced by: how many lines the rewrite takes
/// out of the file that is there, and how many it puts in.
fn headline(changes: &[Change]) -> String {
    let removed = changes.iter().filter(|change| change.edit == Edit::Removed).count();
    let added = changes.len() - removed;
    format!("lines this rewrite changes: {removed} removed, {added} added")
}

/// One changed line, said the way `diff` says it: the mark, the line number in the file
/// the line belongs to, and the line.
fn line_of(change: &Change) -> String {
    format!("  {}{} {}", change.edit.mark(), change.at, change.text)
}

/// One question per line, so they align under the label they share.
fn questions(gaps: &[Gap]) -> String {
    gaps.iter()
        .map(|gap| format!("{}: {}", gap.key.toml_key(), gap.key.question()))
        .collect::<Vec<String>>()
        .join("\n")
}
