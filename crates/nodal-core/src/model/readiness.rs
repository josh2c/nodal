//! Whether a working copy is ready to be worked in, and which part of it is not.
//!
//! A base is recorded as warm when its row exists and its directory is there. Neither
//! of those says a package manager ever finished: a build that was undone leaves the
//! directory, and a base cloned from a release that installed one manager of three
//! leaves two thirds of the tree empty. So warmth is asked of the files themselves.
//!
//! Two parts, because a base has two things to do: install the dependencies, and run
//! the project's build. They fail separately and they are fixed separately, so they are
//! reported separately.
//!
//! Three states, and the third is the honest one. `cargo fetch` writes into the Cargo
//! home, outside the tree and shared by every base on the host, so a Cargo base that is
//! perfectly warm holds no file that says so. Reporting it cold would be a lie that
//! sends a person to rebuild something that is already there; reporting it ready would
//! be a lie in the other direction. It is [`State::Unknown`], with the reason.
//!
//! Nothing here runs a tool. A part is [`State::Ready`] only when a file proves it.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A part of a working copy that a tool has to have made.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// What the package managers install.
    Dependencies,
    /// What the project's build command produces.
    Build,
}

impl fmt::Display for Part {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(match self {
            Self::Dependencies => "dependencies",
            Self::Build => "build",
        })
    }
}

/// What is known about one part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum State {
    /// A file in the tree proves the part is there.
    Ready,
    /// A file that should be there is not, or nothing the recipe names would put it
    /// there.
    Cold {
        /// What is missing, and which tool would make it, or that no tool would.
        why: String,
    },
    /// Nothing in the tree can answer, and nothing here will run a tool to find out.
    Unknown {
        /// Why the tree cannot answer.
        why: String,
    },
}

impl State {
    /// Whether a file proves this part is there.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// The reason, for the states that carry one.
    #[must_use]
    pub fn why(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::Cold { why } | Self::Unknown { why } => Some(why),
        }
    }

    /// The more serious of two answers about one part.
    ///
    /// A part is as ready as its least ready half: a repository whose Node dependencies
    /// are missing is cold whatever its other managers say, and one that is only
    /// unanswerable is unknown rather than ready.
    #[must_use]
    pub fn worse(self, other: Self) -> Self {
        if self.rank() >= other.rank() { self } else { other }
    }

    /// How serious this state is. Cold beats unknown, which beats ready.
    const fn rank(&self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Unknown { .. } => 1,
            Self::Cold { .. } => 2,
        }
    }
}

/// Both parts of one working copy.
///
/// A struct of two named parts rather than a list, so that a report cannot leave one
/// out and cannot name one twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Readiness {
    /// What the package managers were to install.
    pub dependencies: State,
    /// What the build command was to produce.
    pub build: State,
}

impl Readiness {
    /// Each part with its state, in the order a person reads them.
    #[must_use]
    pub fn parts(&self) -> [(Part, &State); 2] {
        [(Part::Dependencies, &self.dependencies), (Part::Build, &self.build)]
    }

    /// Whether a file proves every part is there.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.dependencies.is_ready() && self.build.is_ready()
    }

    /// Every part a file proves is missing. These are the lines a report writes.
    #[must_use]
    pub fn cold(&self) -> Vec<(Part, &str)> {
        self.parts()
            .into_iter()
            .filter_map(|(part, state)| match state {
                State::Cold { why } => Some((part, why.as_str())),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Part, Readiness, State};

    fn cold() -> State {
        State::Cold { why: String::from("node_modules is not there") }
    }

    fn unknown() -> State {
        State::Unknown { why: String::from("cargo keeps its cache outside the tree") }
    }

    #[test]
    fn the_least_ready_answer_is_the_one_a_part_reports() {
        assert_eq!(State::Ready.worse(unknown()), unknown());
        assert_eq!(unknown().worse(cold()), cold());
        assert_eq!(cold().worse(State::Ready), cold());
        assert_eq!(State::Ready.worse(State::Ready), State::Ready);
    }

    #[test]
    fn a_report_names_only_the_parts_a_file_proves_are_missing() {
        let readiness = Readiness { dependencies: cold(), build: unknown() };
        assert!(!readiness.is_ready());
        assert_eq!(readiness.cold().len(), 1, "unknown is not a missing part");
        assert_eq!(readiness.cold()[0].0, Part::Dependencies);

        let ready = Readiness { dependencies: State::Ready, build: State::Ready };
        assert!(ready.is_ready());
        assert!(ready.cold().is_empty());
    }

    #[test]
    fn an_unknown_part_is_not_ready_and_carries_its_reason() {
        let readiness = Readiness { dependencies: unknown(), build: State::Ready };
        assert!(!readiness.is_ready());
        assert!(readiness.dependencies.why().is_some_and(|why| why.contains("outside the tree")));
        assert_eq!(State::Ready.why(), None);
    }
}
