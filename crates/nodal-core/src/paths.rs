//! Where a path really is.
//!
//! One function, and it is here rather than inside an operation because every layer asks
//! it: a create, a doctor survey, an attribution scan and a list all compare one path
//! with another, and a comparison of two names for one directory is the fault they all
//! have to avoid.

use std::path::{Path, PathBuf};

/// A path with every symbolic link on it resolved, as far as it exists.
///
/// This is the one place a path is normalised before it is recorded as, or compared
/// with, another path. Two names for one directory must not become two directories: a
/// project recorded at `/var/folders/…` and a command run in `/private/var/folders/…`
/// are the same tree, and macOS gives a process the second name for the first.
///
/// `canonicalize` needs the whole path to be there, and the destination of a create is
/// exactly what is not. The longest existing prefix is resolved and the rest is put
/// back on, which is enough: a link cannot be part of a path that does not exist.
#[must_use]
pub fn resolve(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut head = path;
    loop {
        if let Ok(real) = head.canonicalize() {
            return rest.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (head.file_name(), head.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                head = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use tempfile::TempDir;

    use super::resolve;

    #[test]
    fn a_path_that_does_not_exist_yet_still_resolves_its_existing_part() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("e").join("abcd1234");
        assert!(resolve(&missing).starts_with(dir.path().canonicalize().unwrap()));
        assert!(resolve(&missing).ends_with("e/abcd1234"));
    }
}
