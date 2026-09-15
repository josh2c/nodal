//! What the host answered at the moment a base was built.
//!
//! A base records the commands it ran, and a command is only half the story: `cargo
//! fetch` on one host and `cargo fetch` on another are two different builds if the two
//! hosts hold different Cargo. So every tool the recipe names is asked its version once,
//! at the build, and the answers go into the row beside the commands.
//!
//! Asked once and here only. Inference never starts a process, and neither does the
//! warmth check; a build already starts processes for a living, so this is the one place
//! the question can be asked without breaking that rule.
//!
//! A tool that answers nothing is recorded as nothing rather than as absent. The record
//! is what the host said, and a host that said nothing said nothing.

use std::collections::BTreeMap;

use crate::model::recipe::{Recipe, ToolName, ToolVersion};
use crate::substrate::pin::Host;

/// The programs a recipe's own tool names stand for.
///
/// A recipe records a pin under the file it came from — `engines.node`, `cargo.rust`,
/// `pyproject.python` — so the tool is the last part of the name, and this table turns
/// that tool into the program that answers for it. A name not in the table is asked
/// for by its own spelling, which is right for `node`, `go` and every package manager.
const PROGRAMS: &[(&str, &str)] = &[("rust", "rustc"), ("python", "python3"), ("asdf", "asdf")];

/// What every tool the recipe names answered, by the name the recipe records it under.
///
/// The package managers are asked under their own program name, and the toolchain rows
/// under the name the pin was recorded as, so a project that pins Node twice — once in
/// `.node-version` and once in `engines` — gets one answer under each and a mismatch
/// between the two files stays visible.
pub fn of(recipe: &Recipe, host: &dyn Host) -> BTreeMap<ToolName, ToolVersion> {
    let mut asked: BTreeMap<String, Option<ToolVersion>> = BTreeMap::new();
    let mut answers = BTreeMap::new();
    let managers = recipe.package_manager.iter().map(|manager| manager.program().to_owned());
    let pinned = recipe.toolchain.keys().map(|name| name.as_str().to_owned());

    for name in managers.chain(pinned) {
        let program = program_of(&name);
        let answer = asked
            .entry(program.clone())
            .or_insert_with(|| {
                host.version(&program).and_then(|text| ToolVersion::parse(text).ok())
            })
            .clone();
        let (Some(version), Ok(under)) = (answer, ToolName::parse(name)) else { continue };
        answers.insert(under, version);
    }
    answers
}

/// The program that answers for a recipe's tool name.
fn program_of(name: &str) -> String {
    let tool = name.rsplit('.').next().unwrap_or(name);
    PROGRAMS
        .iter()
        .find(|(recorded, _)| *recorded == tool)
        .map_or_else(|| tool.to_owned(), |(_, program)| (*program).to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::cell::RefCell;

    use super::{of, program_of};
    use crate::model::recipe::{PackageManager, Recipe, ToolName, ToolVersion};
    use crate::substrate::pin::Host;

    /// A host that answers every program with its own name and a version, and counts
    /// how many times it was asked.
    struct Counting(RefCell<Vec<String>>);

    impl Host for Counting {
        fn on_path(&self, _program: &str) -> bool {
            true
        }

        fn version(&self, program: &str) -> Option<String> {
            self.0.borrow_mut().push(program.to_owned());
            (program != "missing").then(|| format!("{program} 1.2.3"))
        }
    }

    fn recipe(managers: &[PackageManager], pins: &[(&str, &str)]) -> Recipe {
        let mut recipe = Recipe { package_manager: managers.to_vec(), ..Recipe::default() };
        for (name, version) in pins {
            recipe
                .toolchain
                .insert(ToolName::parse(*name).unwrap(), ToolVersion::parse(*version).unwrap());
        }
        recipe
    }

    #[test]
    fn a_recipes_tool_name_is_asked_of_the_program_that_answers_for_it() {
        assert_eq!(program_of("cargo.rust"), "rustc");
        assert_eq!(program_of("pyproject.python"), "python3");
        assert_eq!(program_of("engines.node"), "node");
        assert_eq!(program_of("gomod.go"), "go");
        assert_eq!(program_of("pnpm"), "pnpm");
    }

    #[test]
    fn every_manager_and_every_pin_is_recorded_under_its_own_name() {
        let recipe = recipe(
            &[PackageManager::Pnpm, PackageManager::Cargo],
            &[("engines.node", "22"), ("cargo.rust", "1.88")],
        );
        let host = Counting(RefCell::new(Vec::new()));
        let answers = of(&recipe, &host);
        let named: Vec<String> = answers.keys().map(ToString::to_string).collect();
        assert_eq!(named, ["cargo", "cargo.rust", "engines.node", "pnpm"]);
        assert_eq!(answers[&ToolName::parse("cargo.rust").unwrap()].to_string(), "rustc 1.2.3");
    }

    /// One project pins Node in two files, and both rows are recorded, but the host is
    /// asked once.
    #[test]
    fn a_program_named_twice_is_asked_once() {
        let recipe = recipe(&[], &[("node", "22.11.0"), ("engines.node", "22.11.0")]);
        let host = Counting(RefCell::new(Vec::new()));
        let answers = of(&recipe, &host);
        assert_eq!(answers.len(), 2, "both rows are recorded");
        assert_eq!(host.0.borrow().len(), 1, "the host was asked more than once");
    }

    #[test]
    fn a_tool_that_answers_nothing_is_recorded_as_nothing() {
        let recipe = recipe(&[], &[("missing", "1")]);
        assert!(of(&recipe, &Counting(RefCell::new(Vec::new()))).is_empty());
    }
}
