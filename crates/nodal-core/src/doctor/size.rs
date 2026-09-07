//! How big a directory is, read and never written.
//!
//! Every figure `nodal doctor` prints comes from here. The walk reads metadata with
//! `symlink_metadata`, so it never follows a link out of the tree it was given and never
//! counts one twice. It opens no file and writes nothing.
//!
//! An entry the account may not read is skipped rather than reported as an error. A
//! machine a person is cleaning up holds directories from other accounts and from
//! containers, and a size that is short by one unreadable directory is still the answer
//! to the question. [`Measure::complete`] says whether anything was skipped, so a report
//! can say the figure is a floor.

use std::path::Path;
use std::time::SystemTime;

/// What a walk of one directory found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measure {
    /// Apparent bytes: the sum of the sizes of every file and link under the directory,
    /// as the source counts them. A filesystem that shares blocks holds fewer.
    pub bytes: u64,
    /// How many entries were counted.
    pub entries: usize,
    /// The most recent modification anywhere under the directory, `None` when there is
    /// nothing under it. This is what says whether generated content is still in use.
    pub newest: Option<SystemTime>,
    /// Whether every entry was read. `false` means the figure is a floor.
    pub complete: bool,
}

impl Default for MeasureBuilder {
    fn default() -> Self {
        Self { measure: Measure { complete: true, ..Measure::default() } }
    }
}

/// A measure under construction, so that `complete` starts true and only falls.
struct MeasureBuilder {
    measure: Measure,
}

/// Measure the tree at `root`.
///
/// The root itself is not counted; what is under it is. A `root` that is not a directory
/// is measured as the one entry it is.
#[must_use]
pub fn measure(root: &Path) -> Measure {
    let mut builder = MeasureBuilder::default();
    let Ok(metadata) = std::fs::symlink_metadata(root) else {
        return Measure { complete: false, ..Measure::default() };
    };
    if !metadata.is_dir() {
        builder.count(&metadata);
        return builder.measure;
    }
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            builder.measure.complete = false;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                builder.measure.complete = false;
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                builder.measure.complete = false;
                continue;
            };
            builder.count(&metadata);
            if metadata.is_dir() {
                queue.push(entry.path());
            }
        }
    }
    builder.measure
}

impl MeasureBuilder {
    /// Add one entry to the measure.
    fn count(&mut self, metadata: &std::fs::Metadata) {
        self.measure.entries += 1;
        if !metadata.is_dir() {
            self.measure.bytes += metadata.len();
        }
        if let Ok(modified) = metadata.modified() {
            self.measure.newest = Some(match self.measure.newest {
                Some(newest) if newest >= modified => newest,
                _ => modified,
            });
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::measure;

    /// A tree of two files, one of them one level down.
    fn tree() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("inner")).unwrap();
        std::fs::write(root.path().join("top"), "0123456789").unwrap();
        std::fs::write(root.path().join("inner/deep"), "01234").unwrap();
        root
    }

    #[test]
    fn a_directory_is_the_sum_of_what_is_under_it() {
        let root = tree();
        let measured = measure(root.path());
        assert_eq!(measured.bytes, 15);
        assert_eq!(measured.entries, 3, "two files and the directory holding one of them");
        assert!(measured.complete);
        assert!(measured.newest.is_some());
    }

    #[test]
    fn a_link_is_counted_and_not_followed() {
        let root = tree();
        std::os::unix::fs::symlink("inner", root.path().join("link")).unwrap();
        let measured = measure(root.path());
        assert_eq!(measured.entries, 4, "the link is one entry and its target is not walked again");
    }

    #[test]
    fn a_path_that_is_not_there_measures_nothing_and_says_so() {
        let measured = measure(Path::new("/this/path/is/not/there"));
        assert_eq!(measured.bytes, 0);
        assert!(!measured.complete);
    }

    #[test]
    fn a_file_measures_itself() {
        let root = tree();
        let measured = measure(&root.path().join("top"));
        assert_eq!(measured.bytes, 10);
        assert_eq!(measured.entries, 1);
    }
}
