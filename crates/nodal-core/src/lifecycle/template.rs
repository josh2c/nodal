//! The values a hook command may name, and the substitution that fills them in.
//!
//! A hook is written once and runs for every unit, so the text a project declares has
//! to be able to say "this unit's branch" without knowing which unit. Five names carry
//! that (`docs/contracts.md`):
//!
//! | name          | value                                                        |
//! |---------------|--------------------------------------------------------------|
//! | `{branch}`    | the branch the unit owns                                     |
//! | `{repo_root}` | the project's own checkout                                   |
//! | `{unit_path}` | the unit's home                                              |
//! | `{hash_port}` | a port derived from the branch, the same one every time      |
//! | `{sanitize}`  | the branch reduced to letters, digits and underscores        |
//!
//! This is plain substitution and not a template engine. Each name is replaced by its
//! value wherever it appears. There are no conditionals, no loops and no filters, and a
//! name this list does not hold is left in the text exactly as the project wrote it: a
//! recipe that means a literal brace keeps one.
//!
//! # A value is not allowed to become syntax
//!
//! The filled-in text is handed to `sh -c`, so a value carrying a quote, a backtick, a
//! dollar or a semicolon would stop being a value and start being part of the command.
//! A branch name may hold every one of those characters, and a branch name arrives with
//! a `git pull`. So a value that holds one is refused, naming the variable and the
//! character ([`crate::Error::HookVariable`]), and the hook does not run. Approval pins
//! the text a person read; this is what stops that text meaning something else when it
//! runs.
//!
//! Substitution happens after the approval check, and the approval digest is over the
//! declared text. A hook is therefore approved once, not once per unit.

use crate::Result;
use crate::error::Error;
use crate::lifecycle::hooks::{Context, Phase};
use crate::services::ports;

/// The characters a value may not hold, because `sh` would read them as syntax rather
/// than as text. A space is not among them: a hook quotes the variable it means to
/// pass as one word, and quoting is what these characters would break out of.
const REFUSED: &[char] = &[
    '\'', '"', '`', '$', ';', '&', '|', '<', '>', '(', ')', '\\', '{', '}', '*', '?', '[', ']',
    '\n', '\r',
];

/// What a variable's value is called in the text, and what it is filled in with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variables {
    /// The branch the unit owns.
    branch: String,
    /// The project's own checkout.
    repo_root: String,
    /// The unit's home.
    unit_path: String,
    /// The port the branch hashes to.
    hash_port: String,
    /// The branch as an identifier.
    sanitize: String,
}

impl Variables {
    /// The values one hook context gives.
    ///
    /// # Errors
    /// [`Error::InvalidValue`] when the branch could not be hashed to a port.
    pub fn of(context: &Context) -> Result<Self> {
        let branch = context.branch.to_string();
        Ok(Self {
            hash_port: ports::hashed(&branch)?.to_string(),
            sanitize: sanitize(&branch),
            repo_root: context.source.to_string_lossy().into_owned(),
            unit_path: context.root.to_string_lossy().into_owned(),
            branch,
        })
    }

    /// Every name and the value it stands for, in the order the contract lists them.
    #[must_use]
    pub fn pairs(&self) -> [(&'static str, &str); 5] {
        [
            ("branch", self.branch.as_str()),
            ("repo_root", self.repo_root.as_str()),
            ("unit_path", self.unit_path.as_str()),
            ("hash_port", self.hash_port.as_str()),
            ("sanitize", self.sanitize.as_str()),
        ]
    }

    /// Fill every name this command uses, and refuse a value that would become syntax.
    ///
    /// A name whose value the command does not ask for is not checked, so a home under
    /// a path with a dollar in it stops only the hooks that name `{unit_path}`.
    ///
    /// # Errors
    /// [`Error::HookVariable`] when a value the command asks for holds a character
    /// `sh` would read as syntax.
    pub fn expand(&self, phase: Phase, command: &str) -> Result<String> {
        let mut filled = command.to_owned();
        for (name, value) in self.pairs() {
            let placeholder = format!("{{{name}}}");
            if !filled.contains(&placeholder) {
                continue;
            }
            if let Some(found) = value.chars().find(|char| REFUSED.contains(char)) {
                return Err(Error::HookVariable {
                    phase,
                    variable: name,
                    value: value.to_owned(),
                    character: found,
                });
            }
            filled = filled.replace(&placeholder, value);
        }
        Ok(filled)
    }
}

/// A branch as a name a database, a container or a directory will accept: lowercase
/// letters, digits and underscores, with no two underscores together and none at either
/// end.
///
/// The rule is deliberately narrow rather than clever. A name that is safe everywhere is
/// worth more than a name that keeps a hyphen, because the value is used where the
/// person writing the hook cannot check it.
fn sanitize(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for char in branch.chars() {
        if char.is_ascii_alphanumeric() {
            out.extend(char.to_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{Variables, sanitize};
    use crate::lifecycle::hooks::{Context, Phase};
    use crate::model::{BranchName, Slug};

    fn context(branch: &str) -> Context {
        Context {
            source: "/p".into(),
            root: "/h".into(),
            unit: "01J8Z6H0000000000000000001".parse().unwrap(),
            slug: Slug::parse("worker-import").unwrap(),
            branch: BranchName::parse(branch).unwrap(),
            parent: None,
            environment: "01J8Z6H0000000000000000002".parse().unwrap(),
        }
    }

    #[test]
    fn every_name_is_filled_in_wherever_it_appears() {
        let variables = Variables::of(&context("nodal/worker-import")).unwrap();
        let filled = variables
            .expand(Phase::PreMerge, "echo {branch} {repo_root} {unit_path} {sanitize} {branch}")
            .unwrap();
        assert_eq!(
            filled,
            "echo nodal/worker-import /p /h nodal_worker_import nodal/worker-import"
        );
    }

    #[test]
    fn a_name_nobody_defined_is_left_as_the_project_wrote_it() {
        let variables = Variables::of(&context("main")).unwrap();
        assert_eq!(variables.expand(Phase::PreNew, "echo {codename}").unwrap(), "echo {codename}");
    }

    #[test]
    fn one_branch_always_hashes_to_the_same_port_and_two_rarely_share_one() {
        let port = |branch: &str| {
            Variables::of(&context(branch)).unwrap().expand(Phase::PreNew, "{hash_port}").unwrap()
        };
        assert_eq!(port("nodal/worker-import"), port("nodal/worker-import"));
        assert_ne!(port("nodal/worker-import"), port("nodal/other"));
    }

    #[test]
    fn a_value_that_would_become_shell_syntax_refuses_the_hook() {
        let variables = Variables::of(&context("nodal/$(id)")).unwrap();
        let refused = variables.expand(Phase::PreMerge, "echo {branch}").unwrap_err();
        let told = refused.to_string();
        assert!(told.contains("pre_merge"), "{told}");
        assert!(told.contains("branch"), "{told}");
        // The same value is harmless to a command that does not name it.
        assert_eq!(variables.expand(Phase::PreMerge, "echo {sanitize}").unwrap(), "echo nodal_id");
    }

    #[test]
    fn a_sanitised_branch_is_an_identifier_and_nothing_else() {
        assert_eq!(sanitize("nodal/Fix-Worker.Import"), "nodal_fix_worker_import");
        assert_eq!(sanitize("--lead--"), "lead");
        assert_eq!(sanitize("--"), "");
        assert_eq!(sanitize("a"), "a");
    }
}
