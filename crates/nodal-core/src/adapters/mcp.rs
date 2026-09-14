//! Declaring the tool server to an agent, in the file an agent reads it from.
//!
//! The hooks in [`super::claude_code`] are one vendor's. This is not: `nodal mcp` speaks
//! the model context protocol, so any agent that speaks it reaches every tool without a
//! hook of its own. What is vendor-shaped is only where the declaration is written, and
//! the declaration itself is four fields.
//!
//! The file is the project's `.mcp.json`, which is the project-scoped server list, so a
//! machine with three projects declares three servers and none of them reaches another
//! project. It is written with the same marked region the hooks are
//! ([`super::settings::add_member`]): one contiguous piece of text Nodal can find again,
//! so removing it leaves the file byte for byte the file it was, with every other server
//! somebody declared still in it.
//!
//! The command is this binary's own path. A person who has two copies of Nodal gets the
//! one they ran, rather than whichever one a search path finds first.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::settings;
use crate::{Error, Result};

/// The file a project declares its own servers in.
pub const FILE: &str = ".mcp.json";

/// The object the servers live under.
pub const CONTAINER: &str = "mcpServers";

/// What this server is called there.
pub const NAME: &str = "nodal";

/// Where the file of the project at `root` is.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// The declaration: which program to run, and the one argument that makes it a server.
#[must_use]
pub fn entry(binary: &Path) -> Value {
    json!({ "command": binary.display().to_string(), "args": ["mcp"] })
}

/// This binary's own path, for a declaration that names it.
///
/// # Errors
/// [`Error::Io`] when the running program cannot say where it is.
pub fn binary() -> Result<PathBuf> {
    std::env::current_exe().map_err(Error::io("<the running program>"))
}

/// Declare the server in the project at `root`, and answer with the file that changed.
///
/// `Ok(None)` means the file already declares it exactly this way, which is what a
/// second `nodal init --claude-hooks` finds.
///
/// # Errors
/// [`Error::InvalidValue`] when the file is not a JSON object, and [`Error::Io`] when it
/// cannot be read or written.
pub fn install(root: &Path, binary: &Path) -> Result<Option<PathBuf>> {
    let file = path(root);
    let before = super::claude_code::read(&file)?;
    let Some(after) = settings::add_member(&before, CONTAINER, NAME, &entry(binary))? else {
        return Ok(None);
    };
    std::fs::write(&file, after).map_err(Error::io(&file))?;
    Ok(Some(file))
}

/// Take the declaration out of the file at `file`.
///
/// A file that holds nothing else afterwards is removed rather than left as an empty
/// document. A file holding somebody else's servers keeps them and everything else it
/// had, to the byte.
///
/// # Errors
/// [`Error::Io`] when the file cannot be read, written or removed.
pub fn uninstall(file: &Path, binary: &Path) -> Result<bool> {
    let before = super::claude_code::read(file)?;
    let Some(after) = settings::remove_member(&before, CONTAINER, NAME, &entry(binary)) else {
        return Ok(false);
    };
    if settings::is_empty(&after) {
        std::fs::remove_file(file).map_err(Error::io(file))?;
        return Ok(true);
    }
    std::fs::write(file, after).map_err(Error::io(file))?;
    Ok(true)
}

/// Whether the file at `file` declares this server as Nodal wrote it.
///
/// # Errors
/// [`Error::Io`] when the file is there and cannot be read.
pub fn declared(file: &Path, binary: &Path) -> Result<bool> {
    let text = super::claude_code::read(file)?;
    Ok(settings::holds_member(&text, CONTAINER, NAME, &entry(binary)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use super::{CONTAINER, NAME, entry};
    use crate::adapters::settings;

    /// The one property the whole module exists for: what goes in comes out, and the
    /// file that held somebody else's server still holds it, to the byte.
    #[test]
    fn what_is_written_comes_out_and_leaves_the_file_as_it_was() {
        let binary = Path::new("/usr/local/bin/nodal");
        let theirs =
            "{\n  \"mcpServers\": {\n    \"theirs\": {\n      \"command\": \"x\"\n    }\n  }\n}\n";
        for before in ["", "{}\n", theirs] {
            let after =
                settings::add_member(before, CONTAINER, NAME, &entry(binary)).unwrap().unwrap();
            assert!(after.contains("\"nodal\""), "{after}");
            assert!(after.contains("\"mcp\""), "{after}");
            let back = settings::remove_member(&after, CONTAINER, NAME, &entry(binary)).unwrap();
            assert_eq!(
                back,
                if before.is_empty() { String::from("{}\n") } else { before.to_owned() }
            );
        }
    }

    /// A second install finds the declaration already there and writes nothing.
    #[test]
    fn declaring_twice_is_declaring_once() {
        let binary = Path::new("/usr/local/bin/nodal");
        let once = settings::add_member("", CONTAINER, NAME, &entry(binary)).unwrap().unwrap();
        assert!(settings::add_member(&once, CONTAINER, NAME, &entry(binary)).unwrap().is_none());
    }
}
