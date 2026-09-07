//! `nodal shell-init`: the shell function and the prompt hook, as text.
//!
//! The scripts themselves are in `shims/`, one file per shell, so that a person can
//! read what they are about to put in their start-up file and a linter can check it.
//! This module renders one of them: it fills in the path of the binary that is running
//! and the name of the file the hook looks for, and returns the text.
//!
//! What the script does is bounded on purpose. It wraps `nodal` so that the two
//! commands which name a directory can move the current shell into it, and it exports a
//! home's environment on entry and unsets it on exit. It starts no shell, it writes
//! nothing into a person's repository, and `nodal` without it remains a program that
//! prints paths for a script to use.

use std::path::Path;

use crate::env::files;
use crate::runtime::shells::Shell;

/// The token the binary's path replaces.
const BIN: &str = "@NODAL_BIN@";

/// The token the manifest's path replaces.
const MANIFEST: &str = "@NODAL_MANIFEST@";

/// The script for bash.
const BASH: &str = include_str!("../../../../shims/nodal.bash");

/// The script for zsh.
const ZSH: &str = include_str!("../../../../shims/nodal.zsh");

/// The script for fish.
const FISH: &str = include_str!("../../../../shims/nodal.fish");

/// The integration for `shell`, with `binary` as the program it calls.
///
/// `binary` is the path of the running executable, so a shell that has not got `nodal`
/// on its `PATH` yet still calls the right one. The script falls back to the name
/// `nodal` when that path is no longer executable, which is what a person meets after
/// an upgrade moved it.
#[must_use]
pub fn script(shell: Shell, binary: &Path) -> String {
    let template = match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    };
    template.replace(BIN, &binary.to_string_lossy()).replace(MANIFEST, files::MANIFEST)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{BASH, BIN, FISH, MANIFEST, ZSH, script};
    use crate::runtime::shells::{EXPORTED, Shell};

    /// What every script has to do, whichever shell it is written for.
    const CLAIMS: &[&str] = &["nodal cd", "NODAL_CD_FILE", EXPORTED, "env --export"];

    #[test]
    fn every_script_makes_the_same_claims() {
        for template in [BASH, ZSH, FISH] {
            for claim in CLAIMS {
                assert!(template.contains(claim), "a script does not mention {claim}");
            }
        }
    }

    #[test]
    fn rendering_leaves_no_token_behind() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let text = script(shell, Path::new("/opt/nodal/bin/nodal"));
            assert!(!text.contains(BIN) && !text.contains(MANIFEST), "{text}");
            assert!(text.contains("/opt/nodal/bin/nodal"), "{text}");
            assert!(text.contains(".nodal/manifest.toml"), "{text}");
        }
    }

    #[test]
    fn no_script_starts_a_shell_or_asks_a_question() {
        for template in [BASH, ZSH, FISH] {
            for banned in ["exec $SHELL", "read -p", "read -q", "$SHELL -i"] {
                assert!(!template.contains(banned), "a script contains {banned}");
            }
        }
    }
}
