//! The shell script on disk: where it goes, how it is written, how it is removed.
//!
//! `nodal shell-init <shell>` prints the script and always has. An install writes that
//! same text into `<state directory>/shims/nodal.<shell>` and puts one line in the
//! start-up file that sources it ([`super::rc`]). The two routes render one text, so a
//! person who reads what `nodal shell-init bash` prints has read the file they are
//! about to source.
//!
//! The file goes under the state directory rather than beside the binary, for the
//! reason every other per-machine file does: `NODAL_HOME` moves all of Nodal's state at
//! once, and a test that names its own state directory writes nothing anywhere else.

use std::path::{Path, PathBuf};

use crate::runtime::init;
use crate::runtime::shells::Shell;
use crate::{Error, Result};

/// The directory the scripts go in, under the state directory.
pub const DIRECTORY_NAME: &str = "shims";

/// The scripts directory inside `state`.
#[must_use]
pub fn directory(state: &Path) -> PathBuf {
    state.join(DIRECTORY_NAME)
}

/// Where one shell's script goes.
#[must_use]
pub fn path(state: &Path, shell: Shell) -> PathBuf {
    directory(state).join(format!("nodal.{}", shell.name()))
}

/// Write the script for `shell`, with `binary` as the program it calls.
///
/// The directory is made if it is not there. Writing again over an existing file is
/// the ordinary case: it is how an upgrade that moved the binary is picked up.
///
/// # Errors
/// [`Error::Io`] when the directory or the file cannot be written.
pub fn write(state: &Path, shell: Shell, binary: &Path) -> Result<PathBuf> {
    let directory = directory(state);
    std::fs::create_dir_all(&directory).map_err(Error::io(&directory))?;
    let file = path(state, shell);
    std::fs::write(&file, init::script(shell, binary)).map_err(Error::io(&file))?;
    Ok(file)
}

/// Every shell script that is on this machine now, in the order shells are supported
/// for.
#[must_use]
pub fn installed(state: &Path) -> Vec<PathBuf> {
    shells().filter_map(|shell| Some(path(state, shell)).filter(|file| file.is_file())).collect()
}

/// Remove one shell's script. `Ok(false)` when there was none.
///
/// # Errors
/// [`Error::Io`] when the file is there and cannot be removed.
pub fn remove(state: &Path, shell: Shell) -> Result<bool> {
    let file = path(state, shell);
    if !file.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&file).map_err(Error::io(&file))?;
    Ok(true)
}

/// Remove the scripts directory once it holds no script.
///
/// A directory a person put something else in is left exactly as it is.
///
/// # Errors
/// [`Error::Io`] when the directory is empty and cannot be removed.
pub fn tidy(state: &Path) -> Result<bool> {
    let directory = directory(state);
    let Ok(mut entries) = std::fs::read_dir(&directory) else { return Ok(false) };
    if entries.next().is_some() {
        return Ok(false);
    }
    std::fs::remove_dir(&directory).map_err(Error::io(&directory))?;
    Ok(true)
}

/// Every shell Nodal writes a script for.
pub fn shells() -> impl Iterator<Item = Shell> {
    [Shell::Bash, Shell::Zsh, Shell::Fish].into_iter()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use tempfile::TempDir;

    use super::{installed, path, remove, tidy, write};
    use crate::runtime::shells::Shell;

    #[test]
    fn a_script_is_written_where_the_start_up_file_will_look_for_it() {
        let state = TempDir::new().unwrap();
        let file = write(state.path(), Shell::Bash, Path::new("/opt/nodal/bin/nodal")).unwrap();
        assert_eq!(file, path(state.path(), Shell::Bash));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("/opt/nodal/bin/nodal"), "{text}");
        assert_eq!(installed(state.path()), vec![file]);
    }

    #[test]
    fn removing_the_last_script_takes_the_directory_with_it() {
        let state = TempDir::new().unwrap();
        write(state.path(), Shell::Zsh, Path::new("/opt/nodal/bin/nodal")).unwrap();
        assert!(remove(state.path(), Shell::Zsh).unwrap());
        assert!(!remove(state.path(), Shell::Zsh).unwrap(), "removing twice is not an error");
        assert!(tidy(state.path()).unwrap());
        assert!(installed(state.path()).is_empty());
    }

    #[test]
    fn a_directory_that_holds_something_else_is_left_alone() {
        let state = TempDir::new().unwrap();
        write(state.path(), Shell::Fish, Path::new("/opt/nodal/bin/nodal")).unwrap();
        let mine = super::directory(state.path()).join("mine.sh");
        std::fs::write(&mine, "# not nodal's\n").unwrap();
        assert!(remove(state.path(), Shell::Fish).unwrap());
        assert!(!tidy(state.path()).unwrap());
        assert!(mine.is_file());
    }
}
