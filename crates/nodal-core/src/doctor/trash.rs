//! How much of this machine is homes that have already been reclaimed.
//!
//! The trash is the one thing doctor reports that a person did ask for. Every other
//! finding is something a tool left behind; a trashed home is there because reclaim put
//! it there and promised to keep it for the retention the project set. So it is a line
//! of the header rather than a row of a section: a fact about the machine, stated
//! before the findings, with nothing to act on.
//!
//! It is still worth stating. A person who reclaims ten units a week is holding ten
//! homes they cannot see, under names that are eight characters of an identifier, and
//! the first time they look for the disk is the first time they learn the trash exists.
//!
//! Read from the filesystem and not from the registry. Doctor answers about a machine
//! whose registry may be a schema this binary will not open ([`super::Registry`]), and
//! the trash is exactly the kind of thing a person is asking about when that is the
//! state they are in. The layout is Nodal's own ([`crate::workspace::home`]), so the
//! walk knows where to look without being told.

use std::path::Path;

use super::size;
use crate::output::view::doctor::Trash;

/// The directory each project keeps its reclaimed homes in.
const TRASH: &str = "trash";

/// What the trash under `state_root` holds: how many homes, and how big they are.
///
/// One entry is one reclaimed home, whatever project it belonged to, because that is
/// the unit a person recognises. A state root that is not there, or that holds no trash
/// directory, is an empty answer and never a failure: doctor runs on a machine that has
/// never made a unit.
#[must_use]
pub fn count(state_root: &Path) -> Trash {
    let mut trash = Trash::default();
    let Ok(projects) = std::fs::read_dir(state_root) else {
        return trash;
    };
    for project in projects.flatten() {
        let Ok(homes) = std::fs::read_dir(project.path().join(TRASH)) else {
            continue;
        };
        for home in homes.flatten() {
            let measured = size::measure(&home.path());
            trash.homes += 1;
            trash.bytes += measured.bytes;
            trash.complete = trash.complete && measured.complete;
        }
    }
    trash
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use super::count;

    /// A state root with two projects, three reclaimed homes between them, and a live
    /// home that is not in the trash.
    fn state_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "nodal/trash/E00M0001/target/app", 4096);
        write(root.path(), "nodal/trash/E00M0002/src/main.rs", 64);
        write(root.path(), "storefront/trash/E00M0003/.env.local", 32);
        write(root.path(), "nodal/e/E00M0004/src/main.rs", 1024);
        root
    }

    fn write(root: &Path, relative: &str, bytes: usize) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0_u8; bytes]).unwrap();
    }

    #[test]
    fn every_reclaimed_home_is_counted_and_a_live_one_is_not() {
        let root = state_root();
        let trash = count(root.path());
        assert_eq!(trash.homes, 3);
        assert_eq!(trash.bytes, 4192, "the live home is not in the figure");
        assert!(trash.complete);
    }

    #[test]
    fn a_machine_with_no_trash_answers_with_nothing() {
        let root = tempfile::tempdir().unwrap();
        let trash = count(root.path());
        assert_eq!(trash.homes, 0);
        assert_eq!(trash.bytes, 0);
    }

    #[test]
    fn a_state_root_that_is_not_there_is_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(count(&root.path().join("gone")).homes, 0);
    }
}
