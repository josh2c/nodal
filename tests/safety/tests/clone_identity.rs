//! A clone is the same tree however many workers make it.
//!
//! A unit home is a copy of a base, and the copy now runs on several threads. That
//! bought the seconds a create takes, and it put four things at risk that a copy on one
//! thread could not get wrong.
//!
//! | what a break looks like | what it costs a person |
//! |---|---|
//! | two names for one file become two files | a warm package store doubles on disk, and an edit through one name is unseen through the other |
//! | a directory keeps the mode or the time the copier left on it | a build tool redoes work the base already did, and a locked directory is writable in the home |
//! | a file loses its mode or its extended attribute | content the project wrote read-only is writable in the home |
//! | the copier writes into the tree it is reading | every later unit is cloned from a base one worker changed |
//!
//! So the property is stated as an equality rather than as a list of things to look at:
//! the clone at four, eight and sixteen workers is the same tree, byte for byte, as the
//! clone at one, and the report each one returns is the same report. A worker count is
//! not allowed to be visible in the result.
//!
//! The tree is built here rather than taken from the fixture project. The fixture is
//! under a hundred files in a handful of directories, which is one work item per worker
//! and proves nothing about work items running at once. The tree below has many
//! directories, a hard-linked file whose names sit in directories far apart in the
//! walk, and the read-only content with an extended attribute that a real base is
//! mostly made of.
//!
//! Both backends run, as `crates/nodal-core/tests/materialize.rs` does: the one this
//! machine selects and the copying fallback. A clone has to hold the same tree whichever
//! call put it there.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use nodal_fixture::read_only;
use nodal_safety::Snapshot;

use nodal_core::workspace::sharing::Sharing;
use nodal_core::workspace::{Excludes, Materializer, Report, copy, remove, select_backend};

/// The worker counts the property is asserted at.
///
/// One is the copy that used to be the only one. Sixteen is more workers than this
/// machine gives a clone by default, so the property covers a person who sets the
/// variable higher than the ceiling.
const WORKERS: [usize; 4] = [1, 4, 8, 16];

/// How many directories of files the tree holds. More than the highest worker count,
/// so every worker has directories of its own and the passes overlap.
const DIRECTORIES: usize = 24;

/// How many files each of them holds.
const FILES: usize = 12;

/// The file that has three names, and the two other names for it. The first two are far
/// apart in the walk, so the copy holding the bytes and a link to it are made by
/// different workers.
const LINKED: [&str; 3] =
    ["dir-00/store/payload.bin", "dir-23/second-name.bin", "dir-00/store/third.bin"];

/// The directory the tree locks, which is what a package store looks like.
const LOCKED_DIRECTORY: &str = "dir-00/store";

#[test]
fn a_clone_is_the_same_tree_at_every_worker_count() {
    let case = Case::new();
    let source = case.source();
    let before = Snapshot::of(&source);
    assert!(!before.is_empty(), "the source holds nothing, so this proves nothing");
    let times = times_of(&source);

    for (label, backend) in backends() {
        let mut first: Option<(Snapshot, Report)> = None;
        for workers in WORKERS {
            let clone = case.destination(&format!("{label}-{workers}"));
            let report = backend
                .clone_tree_on(&source, &clone, &Excludes::default(), workers)
                .unwrap_or_else(|error| panic!("{label} at {workers} workers: {error}"));
            let taken = Snapshot::of(&clone);

            before.assert_unchanged(&taken, &format!("{label} at {workers} workers"));
            assert_eq!(
                times_of(&clone),
                times,
                "{label} at {workers} workers: a modification time is not the source's"
            );
            one_file(&clone, label, workers);
            attribute_kept(&source, &clone, label, workers);

            match &first {
                None => first = Some((taken, report)),
                Some((made, counted)) => {
                    made.assert_unchanged(
                        &taken,
                        &format!("{label}: {workers} workers made a different tree from one"),
                    );
                    assert_eq!(
                        &report, counted,
                        "{label}: {workers} workers reported a different clone from one"
                    );
                }
            }
            remove::tree(&clone).unwrap();
        }
    }

    before.assert_unchanged(&Snapshot::of(&source), "cloning wrote into the tree it read");
}

#[test]
fn a_file_that_cannot_be_read_stops_the_clone_and_says_why_at_every_worker_count() {
    let case = Case::new();
    let source = case.source();
    let unreadable = source.join("dir-00/unreadable.bin");
    std::fs::write(&unreadable, "no worker may read this\n").unwrap();
    set_mode(&unreadable, 0o000);

    for (label, backend) in backends() {
        let mut first: Option<String> = None;
        for workers in WORKERS {
            let clone = case.destination(&format!("refused-{label}-{workers}"));
            let error = backend
                .clone_tree_on(&source, &clone, &Excludes::default(), workers)
                .expect_err("a file nothing may read was copied");
            // Each worker count clones into a directory of its own, so the two trees
            // are named out of the message and what is left is the failure itself.
            let said = error
                .to_string()
                .replace(&clone.display().to_string(), "<the clone>")
                .replace(&source.display().to_string(), "<the source>");

            assert!(
                said.contains("unreadable.bin"),
                "{label} at {workers} workers: {said} names no path"
            );
            assert!(
                said.contains("denied") || said.contains("permission"),
                "{label} at {workers} workers: {said} gives no reason"
            );
            match &first {
                None => first = Some(said),
                Some(said_first) => assert_eq!(
                    &said, said_first,
                    "{label}: {workers} workers reported a different failure from one"
                ),
            }
            remove::tree(&clone).unwrap();
        }
    }
}

/// A source tree and the clones made beside it, all thrown away with the test.
///
/// The tree is made under `CARGO_TARGET_TMPDIR`, which is inside the build directory.
/// That puts it on the filesystem the project is checked out on, so the backend the
/// test exercises is the one a person on this machine would get.
struct Case {
    /// The directory holding the source and every clone.
    dir: tempfile::TempDir,
}

impl Case {
    /// An empty case.
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        std::fs::create_dir_all(&root).unwrap();
        Self { dir: tempfile::TempDir::new_in(root).unwrap() }
    }

    /// The tree every clone is made from, built the first time it is asked for.
    fn source(&self) -> PathBuf {
        let source = self.dir.path().join("source");
        build(&source);
        source
    }

    /// Where the clone `name` goes. It does not exist yet.
    fn destination(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

impl Drop for Case {
    /// Open the tree before the temporary directory takes it away.
    ///
    /// The source holds a directory at mode `0555` and files at `0444`, which is the
    /// condition the test is about. `tempfile` removes with the plain call, which such a
    /// directory refuses, so a test that left it locked would leave the build directory
    /// dirtier on every run.
    fn drop(&mut self) {
        drop(remove::tree(self.dir.path()));
    }
}

/// The backends every claim is made on: the one this machine selects, and the fallback.
///
/// Each is labelled by the part it plays rather than by its own name. On a filesystem
/// that cannot share blocks the machine selects the fallback, so the two are one backend
/// under two labels, and a destination named after the backend would collide.
fn backends() -> Vec<(&'static str, Box<dyn Materializer>)> {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let recorded = Sharing::ensure(&root);
    vec![("selected", select_backend(&recorded)), ("fallback", Box::new(copy::CopyFallback))]
}

/// Build the tree every clone is made from.
///
/// It is built once per case and never changed afterwards, so the first claim's reading
/// of it is the reading every later claim compares against.
fn build(root: &Path) {
    if root.is_dir() {
        return;
    }
    for directory in 0..DIRECTORIES {
        let held = root.join(name_of(directory));
        std::fs::create_dir_all(&held).unwrap();
        for file in 0..FILES {
            let path = held.join(format!("file-{file:02}.txt"));
            std::fs::write(&path, format!("{}/{file}\n", held.display())).unwrap();
            if file % 3 == 0 {
                let _marked = read_only::mark(&path);
                read_only::lock(&path);
            }
        }
    }
    links(root);
    read_only::plant(root);
    stamp(root);
    read_only::lock(&root.join(LOCKED_DIRECTORY));
}

/// The name of one of the tree's directories.
fn name_of(directory: usize) -> String {
    format!("dir-{directory:02}")
}

/// Put the three names for one file, and the symbolic links, into the tree.
fn links(root: &Path) {
    let first = root.join(LINKED[0]);
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::write(&first, "one file under three names\n").unwrap();
    for name in &LINKED[1..] {
        std::fs::hard_link(&first, root.join(name)).unwrap();
    }
    std::os::unix::fs::symlink("../dir-00/file-00.txt", root.join("dir-01/points-at-a-file"))
        .unwrap();
    std::os::unix::fs::symlink("../dir-00", root.join("dir-01/points-at-a-directory")).unwrap();
    std::os::unix::fs::symlink("nothing-is-here", root.join("dir-01/points-at-nothing")).unwrap();
}

/// Give every entry a modification time of its own, so a copy that dropped one is a
/// difference rather than a coincidence.
///
/// The times run backwards from an hour ago. A time in the future would make every build
/// tool reading the clone believe its output is stale.
fn stamp(root: &Path) {
    let mut age = 3600;
    let mut pending = vec![root.to_path_buf()];
    let mut made = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                pending.push(path.clone());
            }
            made.push(path);
        }
        made.push(directory);
    }
    for path in made {
        age += 1;
        set_time(&path, age);
    }
}

/// The modification time of every path under `root`, relative, to the nanosecond.
fn times_of(root: &Path) -> BTreeMap<PathBuf, (i64, i64)> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                pending.push(path.clone());
            }
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            found.insert(relative, (metadata.mtime(), metadata.mtime_nsec()));
        }
    }
    found
}

/// Insist the three names in the clone are still one file, and one the source does not
/// share.
fn one_file(clone: &Path, label: &str, workers: usize) {
    let inodes: Vec<(u64, u64)> = LINKED
        .iter()
        .map(|name| {
            let metadata = std::fs::symlink_metadata(clone.join(name)).unwrap();
            (metadata.dev(), metadata.ino())
        })
        .collect();
    assert!(
        inodes.windows(2).all(|pair| pair[0] == pair[1]),
        "{label} at {workers} workers: three names for one file became more than one file"
    );
    assert_eq!(
        std::fs::symlink_metadata(clone.join(LINKED[0])).unwrap().nlink(),
        u64::try_from(LINKED.len()).unwrap(),
        "{label} at {workers} workers: the copy is named a different number of times"
    );
}

/// Insist the read-only file in the clone still carries the attribute of its source.
///
/// A filesystem that holds no extended attributes takes none from the source either, so
/// the claim is about the pair rather than about the copy alone.
fn attribute_kept(source: &Path, clone: &Path, label: &str, workers: usize) {
    if !read_only::marked(&source.join(read_only::LOCKED)) {
        return;
    }
    assert!(
        read_only::marked(&clone.join(read_only::LOCKED)),
        "{label} at {workers} workers: the copy lost the extended attribute of its source"
    );
}

/// Set the permission bits of one path.
fn set_mode(path: &Path, bits: u32) {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap();
}

/// Set the modification time of one path to `age` seconds ago, without following a link.
fn set_time(path: &Path, age: i64) {
    use std::os::unix::ffi::OsStrExt as _;

    let now = i64::try_from(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
    )
    .unwrap();
    let when = libc::timespec { tv_sec: now - age, tv_nsec: (age % 1000) * 1_000 };
    let times = [when, when];
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: the path is a NUL-terminated C string that outlives the call, and the
    // array holds the two values `utimensat` reads.
    let answer = unsafe {
        libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), libc::AT_SYMLINK_NOFOLLOW)
    };
    assert_eq!(answer, 0, "{}: {}", path.display(), std::io::Error::last_os_error());
}
