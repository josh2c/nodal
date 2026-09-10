//! What a state directory holds, read back after a command has run.
//!
//! A state directory is Nodal's own: the registry, and one directory per project
//! holding that project's bases, homes and trash. Every fixture in the workspace makes
//! one and then reads it, and the readings are the same however the fixture built the
//! project, so they are here rather than on each fixture.
//!
//! A test never names the project's directory. It is named for a digest no test knows,
//! so [`segment`] finds it by being the only one there.

use std::path::{Path, PathBuf};

use nodal_core::store::Store;

/// Where the registry file is, whether or not a command has made it yet.
#[must_use]
pub fn registry(state: &Path) -> PathBuf {
    state.join("registry.db")
}

/// The registry, opened for reading what a command wrote.
///
/// # Panics
///
/// If it could not be opened.
#[must_use]
pub fn store(state: &Path) -> Store {
    Store::open(registry(state)).expect("the registry opens")
}

/// One segment of the one project a state directory holds, and nothing until a command
/// has made it.
#[must_use]
pub fn segment(state: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join(name))
        .find(|path| path.is_dir())
}

/// Every live home of the project, in name order.
#[must_use]
pub fn homes(state: &Path) -> Vec<PathBuf> {
    entries(segment(state, "e"))
}

/// Every base of the project, in name order.
///
/// One still being assembled carries a suffix and is not a base yet.
#[must_use]
pub fn bases(state: &Path) -> Vec<PathBuf> {
    entries(segment(state, "b"))
        .into_iter()
        .filter(|path| !path.to_string_lossy().ends_with(".partial"))
        .collect()
}

/// Every directory a build was assembling a base in, in name order.
///
/// What a build that failed leaves. A base is assembled beside its own name and
/// renamed into it by the last step, so one of these is a clone and an install that
/// have been paid for and not yet promoted. A test reads it to say that a failure kept
/// them, and that the attempt after it used the same one.
#[must_use]
pub fn partials(state: &Path) -> Vec<PathBuf> {
    entries(segment(state, "b"))
        .into_iter()
        .filter(|path| path.to_string_lossy().ends_with(".partial"))
        .collect()
}

/// Everything the project's trash holds, in name order.
#[must_use]
pub fn trashed(state: &Path) -> Vec<PathBuf> {
    entries(segment(state, "trash"))
}

/// What is directly inside a directory, in name order, and nothing when there is no
/// such directory yet.
#[must_use]
pub fn entries(directory: Option<PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = directory
        .into_iter()
        .flat_map(|path| std::fs::read_dir(path).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// Something that owns a state directory, and the readings that follow from owning one.
///
/// Both fixtures in this crate answer these, and so does any fixture a suite writes for
/// itself. The answers depend on nothing but where the state directory is, so they are
/// written once here and each fixture says only where its own is.
pub trait InState {
    /// Where this fixture's state directory is.
    fn state_dir(&self) -> &Path;

    /// The registry file, whether or not a command has made it yet.
    #[must_use]
    fn registry(&self) -> PathBuf {
        registry(self.state_dir())
    }

    /// The registry, opened for reading what a command wrote.
    ///
    /// # Panics
    ///
    /// If it could not be opened.
    #[must_use]
    fn store(&self) -> Store {
        store(self.state_dir())
    }

    /// One segment of this project's directory, and nothing until something made it.
    #[must_use]
    fn segment(&self, name: &str) -> Option<PathBuf> {
        segment(self.state_dir(), name)
    }

    /// Every live home of this project, in name order.
    #[must_use]
    fn homes(&self) -> Vec<PathBuf> {
        homes(self.state_dir())
    }

    /// Every base of this project, in name order.
    #[must_use]
    fn bases(&self) -> Vec<PathBuf> {
        bases(self.state_dir())
    }

    /// Every directory a build was assembling a base in, in name order.
    #[must_use]
    fn partials(&self) -> Vec<PathBuf> {
        partials(self.state_dir())
    }

    /// Everything this project's trash holds, in name order.
    #[must_use]
    fn trashed(&self) -> Vec<PathBuf> {
        trashed(self.state_dir())
    }
}
