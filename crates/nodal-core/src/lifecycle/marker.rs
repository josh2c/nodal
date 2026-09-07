//! `.nodal/id`: the one line in a home that says whose home it is.
//!
//! A home carries a manifest as well, and the manifest says far more. The marker is
//! separate because it answers a different question, at a different moment: before an
//! operation removes a directory, it has to be certain the directory is the one the
//! registry told it to remove. A manifest can be edited, copied into a
//! sibling, or left behind by a restored backup; the marker is one line, it is compared
//! against a row, and a mismatch stops the operation rather than starting an argument.
//!
//! The file is inside `.nodal/`, which is in the home's `.git/info/exclude`
//! ([`crate::env::files::hide`]), so it never appears in `git status`.

use std::path::{Path, PathBuf};

use crate::env::files::DIR;
use crate::lifecycle::Step;
use crate::model::UnitId;
use crate::{Error, Result};

/// The marker file, relative to a home.
pub const FILE: &str = ".nodal/id";

/// Where the marker sits inside `home`.
#[must_use]
pub fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

/// Write the marker into `home`, creating `.nodal/` if it is not there.
///
/// Idempotent: the same unit written twice leaves the same byte for byte file.
///
/// # Errors
/// [`Error::Io`] when the directory or the file cannot be written.
pub fn write(home: &Path, unit: UnitId) -> Result<()> {
    let directory = home.join(DIR);
    std::fs::create_dir_all(&directory).map_err(Error::io(&directory))?;
    let path = path(home);
    std::fs::write(&path, format!("{unit}\n")).map_err(Error::io(&path))
}

/// The unit a home says it belongs to, `None` when it carries no marker.
///
/// # Errors
/// [`Error::Io`] when the file is there and cannot be read, and
/// [`Error::InvalidValue`] when its content is not a unit identifier.
pub fn read(home: &Path) -> Result<Option<UnitId>> {
    let path = path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io(&path)(error)),
    };
    UnitId::parse(text.trim()).map(Some)
}

/// Insist that `home` is the home of `unit`.
///
/// This is what every operation that removes or rewrites a home calls first.
///
/// # Errors
/// [`Error::HomeUnmarked`] when the directory carries no marker, [`Error::HomeMarkedFor`]
/// when it carries another unit's, and as [`read`] otherwise.
pub fn verify(home: &Path, unit: UnitId) -> Result<()> {
    match read(home)? {
        Some(found) if found == unit => Ok(()),
        Some(found) => {
            Err(Error::HomeMarkedFor { home: home.to_path_buf(), found, expected: unit })
        }
        None => Err(Error::HomeUnmarked { home: home.to_path_buf() }),
    }
}

/// Remove the marker. Removing one that is not there is not a failure.
///
/// # Errors
/// [`Error::Io`] when the file is there and cannot be removed.
pub fn remove(home: &Path) -> Result<()> {
    crate::remove::file(&path(home))
}

/// Writing a home's marker as one step of an operation.
pub struct WriteMarker {
    /// The home the marker goes in.
    pub home: PathBuf,
    /// The unit the home belongs to.
    pub unit: UnitId,
}

impl Step for WriteMarker {
    fn key(&self) -> String {
        String::from("home.marker")
    }

    fn apply(&self) -> Result<()> {
        write(&self.home, self.unit)
    }

    fn undo(&self) -> Result<()> {
        remove(&self.home)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use tempfile::TempDir;

    use super::{read, remove, verify, write};
    use crate::model::UnitId;

    fn unit(last: char) -> UnitId {
        format!("01J8Z6H000000000000000000{last}").parse().unwrap()
    }

    #[test]
    fn a_marked_home_reads_back_as_its_own_unit() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), unit('1')).unwrap();
        write(dir.path(), unit('1')).unwrap();
        assert_eq!(read(dir.path()).unwrap(), Some(unit('1')));
        verify(dir.path(), unit('1')).unwrap();
    }

    #[test]
    fn another_units_home_is_refused_and_an_unmarked_one_is_named() {
        let dir = TempDir::new().unwrap();
        assert!(verify(dir.path(), unit('1')).unwrap_err().to_string().contains("no marker"));
        write(dir.path(), unit('2')).unwrap();
        let error = verify(dir.path(), unit('1')).unwrap_err();
        assert!(error.to_string().contains(&unit('2').to_string()), "{error}");
    }

    #[test]
    fn removing_a_marker_that_is_not_there_is_not_a_failure() {
        let dir = TempDir::new().unwrap();
        remove(dir.path()).unwrap();
        write(dir.path(), unit('3')).unwrap();
        remove(dir.path()).unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
    }
}
