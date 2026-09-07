//! The shells Nodal speaks to, and the one thing they disagree about.
//!
//! Three dialects are supported, in the order they are supported *for*: bash first, zsh
//! second, fish as far as it goes. The list is a table rather than a match arm per
//! question, so adding a fourth shell is a row.
//!
//! Everything Nodal asks a shell to do is an assignment, and an assignment is the one
//! place the dialects part: `export NAME='value'` in bash and zsh, `set -gx NAME
//! 'value'` in fish. Both forms quote with a single quote, and each has its own escape
//! for a single quote inside one, so no character of a value is ever interpreted.

use std::path::Path;

use crate::env::files;
use crate::model::EnvName;
use crate::{Error, Result};

/// The variable an activation writes to name the variables it set.
///
/// The prompt hook unsets exactly these on the way out of a home, so it removes what
/// Nodal added and never what the person exported themselves.
pub const EXPORTED: &str = "NODAL_EXPORTED";

/// A shell Nodal can write an activation for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Shell {
    /// bash, and the default when nothing says otherwise.
    #[default]
    Bash,
    /// zsh.
    Zsh,
    /// fish, which is not a POSIX shell and gets its own rendering.
    Fish,
}

/// One shell's answers: what it is called, where its start-up file is, and how a person
/// installs the integration into it.
struct Dialect {
    /// The name a person types and `$SHELL` ends with.
    name: &'static str,
    /// The start-up file the install line belongs in, under a person's home.
    rc_file: &'static str,
    /// The line that installs the integration.
    install: &'static str,
}

/// Every shell, in the order they are supported for.
const DIALECTS: &[(Shell, Dialect)] = &[
    (
        Shell::Bash,
        Dialect {
            name: "bash",
            rc_file: "~/.bashrc",
            install: r#"eval "$(nodal shell-init bash)""#,
        },
    ),
    (
        Shell::Zsh,
        Dialect { name: "zsh", rc_file: "~/.zshrc", install: r#"eval "$(nodal shell-init zsh)""# },
    ),
    (
        Shell::Fish,
        Dialect {
            name: "fish",
            rc_file: "~/.config/fish/config.fish",
            install: "nodal shell-init fish | source",
        },
    ),
];

impl Shell {
    /// The shell this text names.
    ///
    /// # Errors
    /// [`Error::UnknownShell`] when no supported shell has that name.
    pub fn parse(text: &str) -> Result<Self> {
        DIALECTS
            .iter()
            .find(|(_, dialect)| dialect.name == text)
            .map(|(shell, _)| *shell)
            .ok_or_else(|| Error::UnknownShell { name: text.to_owned() })
    }

    /// The shell a path names, for reading `$SHELL`. `None` when it names none of them.
    #[must_use]
    pub fn of_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        Self::parse(name).ok()
    }

    /// The shell `$SHELL` names, when it names a supported one.
    #[must_use]
    pub fn detect() -> Option<Self> {
        Self::of_path(Path::new(&std::env::var_os("SHELL")?))
    }

    /// What this shell is called.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.dialect().name
    }

    /// The start-up file the install line belongs in.
    #[must_use]
    pub fn rc_file(self) -> &'static str {
        self.dialect().rc_file
    }

    /// The line that installs the integration into that file.
    #[must_use]
    pub fn install_line(self) -> &'static str {
        self.dialect().install
    }

    /// The name of every supported shell, in the order they are supported for.
    #[must_use]
    pub fn names() -> Vec<&'static str> {
        DIALECTS.iter().map(|(_, dialect)| dialect.name).collect()
    }

    /// This shell's row.
    fn dialect(self) -> &'static Dialect {
        // Every variant has a row, and the unit test in this file is what keeps that
        // true; a shell with none would still get an answer rather than a panic.
        DIALECTS
            .iter()
            .find(|(shell, _)| *shell == self)
            .map_or(&DIALECTS[0].1, |(_, dialect)| dialect)
    }
}

/// The assignments that give this shell a home's environment.
///
/// The last line assigns [`EXPORTED`], which names every variable the lines before it
/// set. That is what lets a prompt hook leave a home as cleanly as it entered one.
#[must_use]
pub fn assignments(shell: Shell, pairs: &[(EnvName, String)]) -> String {
    let names = pairs.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(" ");
    match shell {
        Shell::Bash | Shell::Zsh => {
            let mut text = files::export_lines(pairs);
            text.push_str("export ");
            text.push_str(EXPORTED);
            text.push_str("='");
            text.push_str(&names);
            text.push_str("'\n");
            text
        }
        Shell::Fish => {
            let mut text = String::new();
            for (name, value) in pairs {
                text.push_str(&fish_assignment(name.as_str(), value));
            }
            text.push_str(&fish_assignment(EXPORTED, &names));
            text
        }
    }
}

/// One `set -gx NAME 'value'` line. fish reads two escapes inside single quotes, a
/// backslash and a single quote, and nothing else.
fn fish_assignment(name: &str, value: &str) -> String {
    let quoted = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("set -gx {name} '{quoted}'\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{DIALECTS, EXPORTED, Shell, assignments};
    use crate::model::EnvName;

    fn pairs() -> Vec<(EnvName, String)> {
        vec![
            (EnvName::parse("PORT").unwrap(), String::from("3011")),
            (EnvName::parse("SECRET").unwrap(), String::from("a'b\\c $d")),
        ]
    }

    #[test]
    fn every_shell_has_a_row() {
        for (shell, dialect) in DIALECTS {
            assert_eq!(shell.name(), dialect.name);
            assert_eq!(Shell::parse(dialect.name).unwrap(), *shell);
        }
    }

    #[test]
    fn a_shell_nodal_does_not_speak_is_a_message() {
        assert!(Shell::parse("tcsh").is_err());
        assert_eq!(Shell::of_path(std::path::Path::new("/usr/bin/zsh")), Some(Shell::Zsh));
    }

    #[test]
    fn the_posix_form_quotes_a_value_and_names_what_it_set() {
        let text = assignments(Shell::Bash, &pairs());
        assert!(text.contains(r"export SECRET='a'\''b\c $d'"), "{text}");
        assert!(text.ends_with(&format!("export {EXPORTED}='PORT SECRET'\n")), "{text}");
    }

    #[test]
    fn the_fish_form_quotes_a_value_and_names_what_it_set() {
        let text = assignments(Shell::Fish, &pairs());
        assert!(text.contains(r"set -gx SECRET 'a\'b\\c $d'"), "{text}");
        assert!(text.ends_with(&format!("set -gx {EXPORTED} 'PORT SECRET'\n")), "{text}");
    }
}
