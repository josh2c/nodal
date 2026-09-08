//! Writing a file the way a reader may be reading it.
//!
//! A unit's memory is rewritten by every command that touches the unit, and it is read
//! by an agent that may be reading it at that moment. A write in place has a window in
//! which the file is half of the old text and half of the new one, and an agent that
//! reads it there is an agent given a fact that was never true.
//!
//! So the text goes to a file beside the destination and is renamed onto it. A rename
//! within one directory is atomic on every filesystem Nodal supports, so a reader sees
//! either the whole of the last answer or the whole of this one, and never a mixture.
//!
//! The temporary name carries the process identifier, so two commands writing one
//! unit's memory at the same time do not write the same temporary file. Whichever
//! renames last wins, and both answers were whole.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Write `text` to `path`, through a file beside it.
///
/// The temporary file is removed when the rename fails, so a failure leaves the
/// directory as it found it rather than seeded with fragments.
///
/// # Errors
/// [`Error::Io`] when the temporary file cannot be written or renamed.
pub fn write(path: &Path, text: &str) -> Result<()> {
    let beside = beside(path);
    std::fs::write(&beside, text).map_err(Error::io(&beside))?;
    match std::fs::rename(&beside, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            drop(std::fs::remove_file(&beside));
            Err(Error::io(path)(error))
        }
    }
}

/// The name the text is written under before it is renamed onto `path`.
fn beside(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| String::from("nodal"), |name| name.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{beside, write};

    #[test]
    fn the_temporary_file_is_in_the_directory_it_will_be_renamed_within() {
        let path = std::path::Path::new("/tmp/unit/WORKUNIT.md");
        let beside = beside(path);
        assert_eq!(beside.parent(), path.parent(), "a rename across directories is not atomic");
        assert_ne!(beside.file_name(), path.file_name());
    }

    #[test]
    fn a_second_write_replaces_the_first_and_leaves_nothing_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("WORKUNIT.md");
        write(&path, "first").unwrap();
        write(&path, "second").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let left = std::fs::read_dir(directory.path()).unwrap().count();
        assert_eq!(left, 1, "the temporary file is renamed away, never left behind");
    }
}
