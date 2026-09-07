//! Gaps: the lines inference cannot write, stated as questions.
//!
//! A gap is not an error and not a warning. It is the part of a recipe that only a
//! person knows, named precisely enough that answering it is one line of `nodal.toml`.
//! `nodal init` writes each gap as a comment directly above the empty key it belongs
//! to, so the file itself is the to-do list, and [`crate::recipe::load`] drops a gap as
//! soon as the file answers it.

use serde::{Deserialize, Serialize};

use crate::model::recipe::Recipe;

/// The key a gap is about. One variant per question inference can be left with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKey {
    /// No toolchain pin file and no `engines` field.
    Toolchain,
    /// No migrations directory was recognised.
    Db,
    /// Which services are shared between units and which are per-unit.
    Services,
    /// Which declared environment names local development actually needs.
    EnvRequiredLocal,
}

/// What each gap asks and which `nodal.toml` key answers it. This table is the whole
/// vocabulary of gaps; nothing else may raise one.
const QUESTIONS: &[(GapKey, &str, &str)] = &[
    (
        GapKey::Toolchain,
        "toolchain",
        "which tool versions this project needs; no pin file and no engines field to read",
    ),
    (
        GapKey::Db,
        "db.migrations_dir",
        "where the migrations live, and the command that applies them",
    ),
    (
        GapKey::Services,
        "services",
        "which services are shared between units and which each unit needs its own copy of",
    ),
    (
        GapKey::EnvRequiredLocal,
        "env.required_local",
        "which declared environment names local development needs; the declared file is a \
         superset of what a working copy uses",
    ),
];

impl GapKey {
    /// The `nodal.toml` key that answers this gap.
    #[must_use]
    pub fn toml_key(self) -> &'static str {
        Self::row(self).1
    }

    /// The question, in the words `nodal init` writes into the file.
    #[must_use]
    pub fn question(self) -> &'static str {
        Self::row(self).2
    }

    /// Whether `recipe` answers this gap, so that it is no longer one.
    #[must_use]
    pub fn is_answered_by(self, recipe: &Recipe) -> bool {
        match self {
            Self::Toolchain => !recipe.toolchain.is_empty(),
            Self::Db => recipe.db.migrations_dir.is_some(),
            Self::Services => {
                !recipe.services.shared.is_empty() || !recipe.services.per_unit.is_empty()
            }
            Self::EnvRequiredLocal => !recipe.env.required_local.is_empty(),
        }
    }

    fn row(self) -> &'static (GapKey, &'static str, &'static str) {
        // The table is exhaustive by construction; the fallback keeps the lookup total
        // without an unwrap, and the `every_key_has_a_question` test proves it is dead.
        QUESTIONS.iter().find(|row| row.0 == self).unwrap_or(&QUESTIONS[0])
    }
}

/// One thing a person still has to say, and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// Which key is unanswered.
    pub key: GapKey,
    /// What is known that narrows the question, for example how many names were
    /// declared. Written as a second comment line.
    pub note: Option<String>,
    /// Values a person is choosing among, when inference can list them.
    pub candidates: Vec<String>,
}

impl Gap {
    /// A gap with nothing known beyond the question itself.
    #[must_use]
    pub fn new(key: GapKey) -> Self {
        Self { key, note: None, candidates: Vec::new() }
    }

    /// Add the sentence that narrows the question.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Add the values a person is choosing among.
    #[must_use]
    pub fn candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{GapKey, QUESTIONS};
    use crate::model::recipe::Recipe;

    const KEYS: &[GapKey] =
        &[GapKey::Toolchain, GapKey::Db, GapKey::Services, GapKey::EnvRequiredLocal];

    #[test]
    fn every_key_has_a_question() {
        for key in KEYS {
            assert!(QUESTIONS.iter().any(|row| row.0 == *key), "{key:?} is not in the table");
            assert!(!key.question().is_empty());
            assert!(!key.toml_key().is_empty());
        }
        assert_eq!(QUESTIONS.len(), KEYS.len());
    }

    #[test]
    fn an_empty_recipe_answers_nothing() {
        let recipe = Recipe::default();
        for key in KEYS {
            assert!(!key.is_answered_by(&recipe), "{key:?}");
        }
    }
}
