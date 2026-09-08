//! Integration tests for the materialization backends, against trees they build.
//!
//! Every test runs twice: once on the backend this machine selects, and once on the
//! copying backend that works everywhere. A clone has to hold the same tree whichever
//! call put it there, so a difference between the two is a failure of the backend, not
//! of the machine.
//!
//! Temporary trees are made under `CARGO_TARGET_TMPDIR`, which is inside the build
//! directory. That puts them on the filesystem the project is checked out on, so the
//! backend the tests exercise is the one a person on this machine would get. Nothing
//! here touches a directory it did not create.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use nodal_core::workspace::{
    Excludes, Materializer, Report, copy::CopyFallback, remove, select_backend,
};

/// The backends every test runs on: the one this machine selects, and the fallback.
///
/// Each is labelled by the part it plays rather than by its own name, and the label is
/// what names its destination. On a filesystem that cannot share blocks the machine
/// selects the fallback, so the two are one backend under two labels; naming
/// destinations after the backend would then make the second run collide with the
/// first, and every test would fail for a reason that is not about a clone.
fn backends() -> Vec<(&'static str, Box<dyn Materializer>)> {
    vec![("selected", select_backend(&target_tmp())), ("fallback", Box::new(CopyFallback))]
}

/// The directory temporary trees are made in, on the filesystem of the checkout.
fn target_tmp() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// A source tree and a destination path beside it, both thrown away with the test.
struct Case {
    /// The directory holding both.
    dir: tempfile::TempDir,
}

impl Case {
    /// An empty case.
    fn new() -> Self {
        Self { dir: tempfile::TempDir::new_in(target_tmp()).unwrap() }
    }

    /// The tree a clone is made from.
    fn source(&self) -> PathBuf {
        let source = self.dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        source
    }

    /// Where the clone `name` goes. It does not exist yet.
    fn destination(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// Write a file, making the directories above it.
fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Every path under `root`, relative, sorted, with what it is and what is in it.
fn shape(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            let what = if metadata.is_dir() {
                queue.push(path.clone());
                "dir".to_owned()
            } else if metadata.is_symlink() {
                format!("link -> {}", std::fs::read_link(&path).unwrap().display())
            } else {
                format!("file {:o} {}", metadata.permissions().mode() & 0o777, contents(&path))
            };
            found.insert(relative, what);
        }
    }
    found
}

/// What is in a file, as text.
fn contents(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned()
}

/// Clone `source` into a destination named after the part the backend plays, and
/// report both.
fn clone_with(
    (label, backend): (&str, &dyn Materializer),
    case: &Case,
    source: &Path,
    exclude: &Excludes,
) -> (PathBuf, Report) {
    let destination = case.destination(label);
    let report = backend.clone_tree(source, &destination, exclude).unwrap();
    (destination, report)
}

#[test]
fn the_exclusion_list_leaves_out_what_it_names() {
    let case = Case::new();
    let source = nodal_fixture::write(case.source());
    write(&source.join("node_modules/react/index.js"), "module.exports = {};");
    write(&source.join(".next/cache/webpack/0.pack"), "cache");
    write(&source.join(".claude/worktrees/other/file"), "another checkout");
    for (label, backend) in backends() {
        let (clone, report) =
            clone_with((label, &*backend), &case, &source, &Excludes::default_list());
        assert!(!clone.join("test-results").exists(), "{label}: test output was copied");
        assert!(!clone.join("coverage").exists(), "{label}: coverage was copied");
        assert!(!clone.join(".next/cache").exists(), "{label}: a path-bound cache was copied");
        assert!(!clone.join(".claude/worktrees").exists(), "{label}: nested checkouts were copied");
        assert!(
            clone.join("node_modules/react/index.js").exists(),
            "{label}: dependencies are kept warm, not excluded"
        );
        assert!(clone.join("package.json").exists(), "{label}: the project was not copied");
        assert_eq!(report.excluded, 4, "{label}: one count per entry left out");
    }
}

#[test]
fn a_recipe_adds_to_what_a_clone_leaves_out() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("keep/file"), "kept");
    write(&source.join("var/run/app.sock.data"), "run state");
    let exclude = Excludes::with_recipe(&[PathBuf::from("var/run")]);
    for (label, backend) in backends() {
        let (clone, _) = clone_with((label, &*backend), &case, &source, &exclude);
        assert!(clone.join("keep/file").exists(), "{}", label);
        assert!(!clone.join("var/run").exists(), "{label}: the recipe path was copied");
        assert!(clone.join("var").exists(), "{label}: the directory above it was not");
    }
}

#[test]
fn two_names_for_one_file_stay_two_names_for_one_file() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("store/package/index.js"), "shared");
    std::fs::hard_link(source.join("store/package/index.js"), source.join("store/linked.js"))
        .unwrap();
    for (label, backend) in backends() {
        let (clone, report) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        let first = std::fs::metadata(clone.join("store/package/index.js")).unwrap();
        let second = std::fs::metadata(clone.join("store/linked.js")).unwrap();
        assert_eq!(
            (first.dev(), first.ino()),
            (second.dev(), second.ino()),
            "{label}: the two names became two files"
        );
        assert_eq!(report.hardlinks, 1, "{label}");
        assert_eq!(report.files, 1, "{label}: the bytes were put across once");
    }
}

#[test]
fn a_symbolic_link_is_recreated_and_never_followed() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("real/file"), "content");
    std::os::unix::fs::symlink("real", source.join("to-directory")).unwrap();
    std::os::unix::fs::symlink("real/file", source.join("to-file")).unwrap();
    std::os::unix::fs::symlink("nowhere", source.join("to-nothing")).unwrap();
    for (label, backend) in backends() {
        let (clone, report) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        for (link, target) in
            [("to-directory", "real"), ("to-file", "real/file"), ("to-nothing", "nowhere")]
        {
            let path = clone.join(link);
            assert!(path.symlink_metadata().unwrap().is_symlink(), "{label}: {link}");
            assert_eq!(std::fs::read_link(&path).unwrap(), Path::new(target), "{label}");
        }
        assert_eq!(report.symlinks, 3, "{label}");
        assert_eq!(report.files, 1, "{label}: a link was followed and copied");
    }
}

#[test]
fn an_extended_attribute_survives_the_clone() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("file"), "content");
    let name = "user.nodal.test";
    if !set_attribute(&source.join("file"), name, b"kept") {
        eprintln!("this filesystem holds no extended attributes; nothing to prove here");
        return;
    }
    for (label, backend) in backends() {
        let (clone, report) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        assert_eq!(read_attribute(&clone.join("file"), name).as_deref(), Some(&b"kept"[..]));
        assert_eq!(report.attributes, 1, "{label}");
    }
}

#[test]
fn modes_and_modification_times_are_the_ones_the_source_had() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("bin/run.sh"), "#!/bin/sh\necho run\n");
    std::fs::set_permissions(source.join("bin/run.sh"), std::fs::Permissions::from_mode(0o755))
        .unwrap();
    std::fs::set_permissions(source.join("bin"), std::fs::Permissions::from_mode(0o750)).unwrap();
    for (label, backend) in backends() {
        let (clone, _) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        for relative in ["bin", "bin/run.sh"] {
            let from = std::fs::symlink_metadata(source.join(relative)).unwrap();
            let to = std::fs::symlink_metadata(clone.join(relative)).unwrap();
            assert_eq!(
                to.permissions().mode() & 0o777,
                from.permissions().mode() & 0o777,
                "{label}: {relative} has another mode"
            );
            assert_eq!(
                (to.mtime(), to.mtime_nsec()),
                (from.mtime(), from.mtime_nsec()),
                "{label}: {relative} has another modification time"
            );
        }
    }
}

#[test]
fn both_backends_produce_the_same_tree() {
    let case = Case::new();
    let source = nodal_fixture::write(case.source());
    write(&source.join("node_modules/react/index.js"), "module.exports = {};");
    std::os::unix::fs::symlink("../react", source.join("node_modules/aliased")).unwrap();
    let shapes: Vec<_> = backends()
        .iter()
        .map(|(label, backend)| {
            let (clone, _) =
                clone_with((label, &**backend), &case, &source, &Excludes::default_list());
            (*label, shape(&clone))
        })
        .collect();
    for (name, found) in &shapes[1..] {
        assert_eq!(found, &shapes[0].1, "{name} and {} disagree", shapes[0].0);
    }
}

#[test]
fn a_read_only_file_that_carries_an_attribute_is_cloned() {
    let case = Case::new();
    let source = case.source();
    let locked = source.join("objects/pack/pack-0123456789abcdef.idx");
    write(&locked, "an index git wrote once");
    let name = "user.nodal.test";
    if !set_attribute(&locked, name, b"provenance") {
        eprintln!("this filesystem holds no extended attributes; nothing to prove here");
        return;
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).unwrap();

    for (label, backend) in backends() {
        let (clone, report) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        let copy = clone.join("objects/pack/pack-0123456789abcdef.idx");
        assert_eq!(
            std::fs::symlink_metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o444,
            "{label}: the copy kept another mode than the source had"
        );
        assert_eq!(
            read_attribute(&copy, name).as_deref(),
            Some(&b"provenance"[..]),
            "{label}: the attribute did not survive"
        );
        assert_eq!(report.attributes, 1, "{label}");
        assert_eq!(contents(&copy), "an index git wrote once", "{label}");
    }
}

#[test]
fn a_read_only_file_in_a_read_only_directory_is_cloned() {
    let case = Case::new();
    let source = case.source();
    let store = source.join("vendor/store");
    let locked = store.join("library");
    write(&locked, "bytes a package manager wrote");
    let name = "user.nodal.test";
    let marked = set_attribute(&locked, name, b"provenance");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o555)).unwrap();

    for (label, backend) in backends() {
        let (clone, _) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        let copy = clone.join("vendor/store/library");
        assert_eq!(
            std::fs::symlink_metadata(clone.join("vendor/store")).unwrap().permissions().mode()
                & 0o777,
            0o555,
            "{label}: the directory kept another mode than the source had"
        );
        assert_eq!(
            std::fs::symlink_metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o444,
            "{label}: the file kept another mode than the source had"
        );
        if marked {
            assert_eq!(read_attribute(&copy, name).as_deref(), Some(&b"provenance"[..]), "{label}");
        }
        remove::tree(&clone).unwrap();
    }
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_fixture_a_clone_is_measured_against_carries_the_condition() {
    let case = Case::new();
    let source = nodal_fixture::write(case.source());
    let locked = source.join(nodal_fixture::read_only::LOCKED);
    if !nodal_fixture::read_only::marked(&locked) {
        eprintln!("this filesystem holds no extended attributes; nothing to prove here");
        return;
    }
    assert_eq!(
        std::fs::symlink_metadata(&locked).unwrap().permissions().mode() & 0o777,
        0o444,
        "the fixture no longer carries a file that denies every write"
    );

    for (label, backend) in backends() {
        let (clone, _) = clone_with((label, &*backend), &case, &source, &Excludes::default());
        let copy = clone.join(nodal_fixture::read_only::LOCKED);
        assert!(nodal_fixture::read_only::marked(&copy), "{label}: the attribute did not survive");
        assert_eq!(
            std::fs::symlink_metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o444,
            "{label}: the copy kept another mode"
        );
    }
}

#[test]
fn a_destination_that_is_already_there_is_refused() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("file"), "content");
    let destination = case.destination("taken");
    std::fs::create_dir_all(&destination).unwrap();
    for (label, backend) in backends() {
        let refused = backend.clone_tree(&source, &destination, &Excludes::default());
        assert!(refused.is_err(), "{label}: it wrote into a directory that was there");
    }
}

#[test]
fn a_destination_inside_the_source_is_refused() {
    let case = Case::new();
    let source = case.source();
    write(&source.join("file"), "content");
    for (label, backend) in backends() {
        let inside = source.join("nested/clone");
        let refused = backend.clone_tree(&source, &inside, &Excludes::default());
        assert!(refused.is_err(), "{label}: it cloned a tree into itself");
    }
}

/// Put an extended attribute on a path, and report whether the filesystem took it.
fn set_attribute(path: &Path, name: &str, value: &[u8]) -> bool {
    let (path, name) = (c_string(path.as_os_str().as_bytes()), c_string(name.as_bytes()));
    // SAFETY: both strings are NUL terminated and outlive the call, and the value is a
    // slice of the length given.
    let answer = unsafe {
        #[cfg(target_os = "linux")]
        {
            libc::lsetxattr(path.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0)
        }
        #[cfg(target_os = "macos")]
        {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        }
    };
    answer == 0
}

/// Read one extended attribute back.
fn read_attribute(path: &Path, name: &str) -> Option<Vec<u8>> {
    let (path, name) = (c_string(path.as_os_str().as_bytes()), c_string(name.as_bytes()));
    let mut buffer = vec![0_u8; 256];
    // SAFETY: both strings are NUL terminated and outlive the call, and the buffer is
    // writable for the length given.
    let read = unsafe {
        #[cfg(target_os = "linux")]
        {
            libc::lgetxattr(path.as_ptr(), name.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len())
        }
        #[cfg(target_os = "macos")]
        {
            libc::getxattr(
                path.as_ptr(),
                name.as_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                0,
                libc::XATTR_NOFOLLOW,
            )
        }
    };
    let read = usize::try_from(read).ok()?;
    buffer.truncate(read);
    Some(buffer)
}

/// Bytes as the C string these calls take.
fn c_string(bytes: &[u8]) -> std::ffi::CString {
    std::ffi::CString::new(bytes).unwrap()
}
