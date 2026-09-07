//! What `nodal init` did, and what it left for a person.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::output::Render;
use crate::output::human::{Block, Doc, Field};
use crate::recipe::InitPlan;
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
}

impl InitReport {
    /// The report for a plan.
    #[must_use]
    pub fn from_plan(plan: &InitPlan) -> Self {
        Self {
            path: plan.path.clone(),
            existed: plan.existed,
            contents: plan.contents.clone(),
            gaps: plan.gaps.clone(),
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

/// One question per line, so they align under the label they share.
fn questions(gaps: &[Gap]) -> String {
    gaps.iter()
        .map(|gap| format!("{}: {}", gap.key.toml_key(), gap.key.question()))
        .collect::<Vec<String>>()
        .join("\n")
}
