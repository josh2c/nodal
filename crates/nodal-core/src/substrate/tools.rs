//! What the project pins its tools to, and what this host actually has.
//!
//! `[toolchain]` in the recipe is read by `nodal init` from the project's own pin files
//! and manifests ([`crate::recipe::infer::toolchain`]). Until this module existed it was
//! recorded there and nothing ever looked at it again: no command compared a pin to the
//! host, and no line said that none did. A person who pinned `node` to 20 and worked on
//! a host with 18 was told nothing, by anything, ever.
//!
//! So every pin is reported, and the report is a reading and never a refusal. Two
//! reasons it is not a refusal:
//!
//! * A pin is written in every notation a package manager accepts — `20`, `^20`,
//!   `>=20.11`, `20.11.1` — and a comparison that tried to honour all of them would
//!   refuse work it should allow. [`crate::substrate::pin`] compares by major series and
//!   only where a *package manager* is about to write a tree from a lockfile, which is
//!   the one place a wrong version does silent damage.
//! * Most rows are not selections at all. `engines.node` and `cargo.rust` are what a
//!   manifest states the project needs, not what a version manager would select, and the
//!   name each is recorded under says which ([`crate::recipe::infer::toolchain`]).
//!
//! # Not checked is a third answer and it is the honest one
//!
//! A pin this module has no program for, and a program this host does not have, are both
//! [`Answer::NotChecked`] with the reason. Neither is "the host disagrees" and neither is
//! "the host agrees". A row that guessed either would be the lie the empty line already
//! was.
//!
//! Nothing here installs, selects or changes a tool. It runs `<program> --version`
//! through the one spawn seam [`crate::substrate::build`] owns, and prints what came
//! back.

use serde::{Deserialize, Serialize};

use crate::model::recipe::{Recipe, ToolName, ToolVersion};
use crate::substrate::pin::Host;

/// The programs that answer for one pinned tool, in the order they are tried.
///
/// The key is the tool, after any prefix naming the file the pin came from is taken off:
/// `engines.node`, `node` and a `.nvmrc` row are one question about one program.
///
/// A tool absent from this table is [`Answer::NotChecked`]. Guessing that a tool is a
/// program of the same name would ask the host about `asdf` as though `asdf --version`
/// said which Node a project selects, which it does not.
const ANSWERS: &[(&str, &[&str])] = &[
    ("node", &["node"]),
    ("npm", &["npm"]),
    ("pnpm", &["pnpm"]),
    ("yarn", &["yarn"]),
    ("bun", &["bun"]),
    ("rust", &["rustc"]),
    ("cargo", &["cargo"]),
    ("python", &["python3", "python"]),
    ("go", &["go"]),
    ("deno", &["deno"]),
];

/// What this host said about one pinned tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "answer")]
pub enum Answer {
    /// A program answered, and this is its first line of output, as printed.
    Host {
        /// What `<program> --version` said.
        version: String,
    },
    /// Nothing on this host answers for the pin, so the pin was not compared to
    /// anything.
    NotChecked {
        /// Why no reading was taken.
        why: String,
    },
}

impl Answer {
    /// The words a report prints for the host's half of the line.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Host { version } => format!("host {version}"),
            Self::NotChecked { why } => format!("not checked: {why}"),
        }
    }
}

/// One pinned tool, and what this host answered about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    /// The tool, under the name the recipe records it by.
    pub tool: ToolName,
    /// The version the project pins it to, as written in the pin file.
    pub pin: ToolVersion,
    /// What this host said.
    pub host: Answer,
}

impl Pinned {
    /// The one line a report prints for this pin.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}: pinned {}, {}", self.tool, self.pin, self.host.label())
    }
}

/// Every pin the recipe records, with this host's answer beside it.
///
/// One process per tool this host has a program for, and none for the rest. A project
/// that pins nothing reads nothing and reports nothing.
#[must_use]
pub fn readings(recipe: &Recipe, host: &dyn Host) -> Vec<Pinned> {
    recipe
        .toolchain
        .iter()
        .map(|(tool, pin)| Pinned {
            tool: tool.clone(),
            pin: pin.clone(),
            host: asked(tool.as_str(), host),
        })
        .collect()
}

/// What this host answers about one tool, or why it answers nothing.
fn asked(tool: &str, host: &dyn Host) -> Answer {
    let program = tool.rsplit('.').next().unwrap_or(tool);
    let Some((_, candidates)) = ANSWERS.iter().find(|(name, _)| *name == program) else {
        return Answer::NotChecked { why: format!("no program here answers for {program}") };
    };
    let Some(found) = candidates.iter().find(|program| host.on_path(program)) else {
        return Answer::NotChecked {
            why: format!("{} is not on the path", candidates.join(" or ")),
        };
    };
    match host.version(found) {
        Some(version) => Answer::Host { version },
        None => Answer::NotChecked { why: format!("{found} printed no version") },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::collections::BTreeMap;

    use super::{Answer, readings};
    use crate::model::recipe::{Recipe, ToolName, ToolVersion};

    /// A host that has exactly the programs it was given, each with one version line.
    struct Fake(BTreeMap<&'static str, &'static str>);

    impl crate::substrate::pin::Host for Fake {
        fn on_path(&self, program: &str) -> bool {
            self.0.contains_key(program)
        }

        fn version(&self, program: &str) -> Option<String> {
            self.0.get(program).map(|line| (*line).to_owned())
        }
    }

    /// A recipe pinning each of these tools.
    fn recipe(pins: &[(&str, &str)]) -> Recipe {
        let mut recipe = Recipe::default();
        for (tool, version) in pins.iter().copied() {
            recipe
                .toolchain
                .insert(ToolName::parse(tool).unwrap(), ToolVersion::parse(version).unwrap());
        }
        recipe
    }

    /// The line a person reads carries all three parts: the pin, the host, and which
    /// tool they are about.
    #[test]
    fn a_pin_the_host_answers_for_names_the_pin_and_the_host_version() {
        let host = Fake(BTreeMap::from([("node", "v20.11.1")]));
        let read = readings(&recipe(&[("node", "20")]), &host);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].host, Answer::Host { version: String::from("v20.11.1") });
        assert_eq!(read[0].line(), "node: pinned 20, host v20.11.1");
    }

    /// The prefix names the file a pin came from, not a different tool. `engines.node`
    /// and `cargo.rust` are questions about `node` and `rustc`.
    #[test]
    fn a_prefixed_pin_asks_the_program_the_tool_half_names() {
        let host = Fake(BTreeMap::from([("node", "v20.11.1"), ("rustc", "rustc 1.89.0")]));
        let read = readings(&recipe(&[("engines.node", ">=20"), ("cargo.rust", "1.85")]), &host);
        let lines: Vec<String> = read.iter().map(super::Pinned::line).collect();
        assert!(lines.iter().any(|line| line.starts_with("cargo.rust: pinned 1.85, host rustc")));
        assert!(lines.iter().any(|line| line.contains("engines.node: pinned >=20, host v20.11.1")));
    }

    /// Three ways a reading is not taken, and each says which. None of them is a
    /// comparison and none of them is silence.
    #[test]
    fn a_pin_nothing_answers_for_is_not_checked_and_says_why() {
        let bare = Fake(BTreeMap::new());
        let read = readings(&recipe(&[("node", "20"), ("asdf", "0.14")]), &bare);
        let absent = read.iter().find(|one| one.tool.as_str() == "node").unwrap();
        assert_eq!(
            absent.host,
            Answer::NotChecked { why: String::from("node is not on the path") }
        );
        let unknown = read.iter().find(|one| one.tool.as_str() == "asdf").unwrap();
        assert!(unknown.line().contains("not checked: no program here answers for asdf"));
    }

    /// A project that pins nothing gets no lines, rather than a line saying nothing.
    #[test]
    fn a_project_that_pins_nothing_reports_nothing() {
        assert!(readings(&Recipe::default(), &Fake(BTreeMap::new())).is_empty());
    }
}
