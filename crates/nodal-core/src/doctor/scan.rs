//! A walk for `.git` under a root: directories and worktree files.
//!
//! The walk reads and never writes. It does not follow symbolic links. A directory that
//! holds `.git` is a repository, and the walk does not enter it: nested repositories
//! are part of that clone. It skips Nodal's state directory, any registered unit home,
//! and a mount that is not a local filesystem, and it says so.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::lifecycle::guard;
use crate::output::view::machine::Skip;

/// How many directory levels the walk descends from a root when the caller does not say.
pub const DEFAULT_DEPTH: usize = 6;

/// Filesystem types the walk will not enter: they are not local disk.
const REMOTE: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smb",
    "smb3",
    "9p",
    "afs",
    "ceph",
    "fuse.sshfs",
    "fuse.rclone",
    "fuse.davfs",
];

/// What one walk found.
#[derive(Debug, Default)]
pub struct Found {
    /// Repository working trees, resolved, in the order they were met.
    pub repositories: Vec<PathBuf>,
    /// Paths the walk did not enter, and why.
    pub skipped: Vec<Skip>,
    /// Directory entries the walk looked at.
    pub entries: u64,
}

/// One path the walk must not enter, and the reason a report prints for it.
#[derive(Debug, Clone)]
pub struct Avoid {
    /// The directory, resolved.
    pub path: PathBuf,
    /// Why it is skipped.
    pub why: String,
}

/// Find every repository under `roots`.
#[must_use]
pub fn walk(roots: &[PathBuf], depth: usize, avoid: &[Avoid]) -> Found {
    let mounts = Mounts::load();
    let mut found = Found::default();
    for root in roots {
        let root = guard::resolve(root);
        let mut walker = Walker { depth, avoid, mounts: &mounts, found: &mut found };
        walker.root(&root);
    }
    found.repositories.sort();
    found.repositories.dedup();
    found
}

/// The walk's borrowed inputs, so each step stays a method of one type.
struct Walker<'a> {
    depth: usize,
    avoid: &'a [Avoid],
    mounts: &'a Mounts,
    found: &'a mut Found,
}

impl Walker<'_> {
    /// Start at a root. A root that is itself skipped is reported and not entered.
    fn root(&mut self, path: &Path) {
        if self.skip(path) {
            return;
        }
        self.visit(path, 0, device(path));
    }

    /// Look at one directory for `.git`, then at its children while depth remains.
    fn visit(&mut self, path: &Path, depth: usize, parent_dev: Option<u64>) {
        let Ok(entries) = fs::read_dir(path) else {
            self.found.skipped.push(Skip::new(path, "could not be read"));
            return;
        };
        let mut children = Vec::new();
        let mut git = false;
        for entry in entries {
            let Ok(entry) = entry else {
                self.found.entries += 1;
                continue;
            };
            self.found.entries += 1;
            if entry.file_name() == ".git" {
                git = true;
                continue;
            }
            children.push(entry.path());
        }
        if git {
            self.found.repositories.push(guard::resolve(path));
            return;
        }
        if depth >= self.depth {
            return;
        }
        for child in children {
            self.descend(&child, depth, parent_dev);
        }
    }

    /// Enter a child directory, or skip it with a reason.
    fn descend(&mut self, path: &Path, depth: usize, parent_dev: Option<u64>) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return;
        }
        if self.skip(path) {
            return;
        }
        let dev = metadata.dev();
        if parent_dev.is_some_and(|parent| parent != dev)
            && let Some(why) = self.mounts.remote_reason(path)
        {
            self.found.skipped.push(Skip::new(path, why));
            return;
        }
        self.visit(path, depth + 1, Some(dev));
    }

    /// Whether `path` is an avoided directory, and record it when it is.
    fn skip(&mut self, path: &Path) -> bool {
        let resolved = guard::resolve(path);
        let Some(avoid) = self.avoid.iter().find(|avoid| is_under(&resolved, &avoid.path)) else {
            return false;
        };
        self.found.skipped.push(Skip::new(&resolved, avoid.why.clone()));
        true
    }
}

/// Whether `path` is `parent` or a directory under it.
fn is_under(path: &Path, parent: &Path) -> bool {
    path == parent || path.starts_with(parent)
}

/// The device number of a path, when metadata can be read.
fn device(path: &Path) -> Option<u64> {
    fs::symlink_metadata(path).ok().map(|metadata| metadata.dev())
}

/// Mount points and their filesystem types, from `/proc/self/mountinfo`.
struct Mounts {
    mounts: Vec<(PathBuf, String)>,
}

impl Mounts {
    /// The mounts this process can see. An empty list when the table is not there.
    fn load() -> Self {
        let Ok(text) = fs::read_to_string("/proc/self/mountinfo") else {
            return Self { mounts: Vec::new() };
        };
        let mut mounts = Vec::new();
        for line in text.lines() {
            if let Some((path, fstype)) = parse_mount(line) {
                mounts.push((path, fstype));
            }
        }
        mounts.sort_by_key(|(path, _)| std::cmp::Reverse(path.as_os_str().len()));
        Self { mounts }
    }

    /// Why `path` is not local disk, when it is a remote mount.
    fn remote_reason(&self, path: &Path) -> Option<String> {
        let fstype = self.fstype(path)?;
        remote(fstype).then(|| format!("not a local filesystem ({fstype})"))
    }

    /// The filesystem type of the longest matching mount point.
    fn fstype(&self, path: &Path) -> Option<&str> {
        self.mounts
            .iter()
            .find_map(|(mount, fstype)| path.starts_with(mount).then_some(fstype.as_str()))
    }
}

/// One `/proc/self/mountinfo` line as a mount point and a filesystem type.
fn parse_mount(line: &str) -> Option<(PathBuf, String)> {
    let (left, right) = line.split_once(" - ")?;
    let mount = left.split_whitespace().nth(4)?;
    let fstype = right.split_whitespace().next()?;
    Some((PathBuf::from(unescape(mount)), fstype.to_owned()))
}

/// Octal escapes in a mountinfo path, as the kernel writes spaces.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            let some = [bytes.next(), bytes.next(), bytes.next()];
            if let (Some(a), Some(b), Some(c)) = (some[0], some[1], some[2]) {
                let digit = |d: u8| u32::from(d.wrapping_sub(b'0'));
                if let Some(ch) = char::from_u32((digit(a) << 6) | (digit(b) << 3) | digit(c)) {
                    out.push(ch);
                    continue;
                }
            }
            out.push('\\');
        } else if let Some(ch) = char::from_u32(u32::from(byte)) {
            out.push(ch);
        }
    }
    out
}

/// Whether a filesystem type is not local disk.
fn remote(fstype: &str) -> bool {
    let lower = fstype.to_ascii_lowercase();
    REMOTE.contains(&lower.as_str())
        || lower.starts_with("nfs")
        || lower.contains("sshfs")
        || lower.contains("rclone")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{remote, walk};
    use crate::doctor::scan::Avoid;
    use std::fs;
    use std::path::PathBuf;

    fn git_dir(path: &std::path::Path) {
        fs::create_dir_all(path.join(".git")).unwrap();
    }

    #[test]
    fn a_git_directory_and_a_worktree_file_are_both_repositories() {
        let root = tempfile::tempdir().unwrap();
        git_dir(&root.path().join("plain"));
        fs::create_dir_all(root.path().join("linked")).unwrap();
        fs::write(root.path().join("linked/.git"), "gitdir: /somewhere\n").unwrap();
        let found = walk(&[root.path().to_path_buf()], 6, &[]);
        assert_eq!(found.repositories.len(), 2, "{found:?}");
    }

    #[test]
    fn a_repository_deeper_than_the_bound_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        git_dir(&root.path().join("a/b/repo"));
        let shallow = walk(&[root.path().to_path_buf()], 2, &[]);
        assert!(shallow.repositories.is_empty(), "{shallow:?}");
        let deep = walk(&[root.path().to_path_buf()], 3, &[]);
        assert_eq!(deep.repositories.len(), 1, "{deep:?}");
    }

    #[test]
    fn an_avoided_directory_is_skipped_and_named() {
        let root = tempfile::tempdir().unwrap();
        git_dir(&root.path().join("keep"));
        git_dir(&root.path().join("state/hidden"));
        let avoid = [Avoid {
            path: root.path().join("state").canonicalize().unwrap(),
            why: String::from("nodal's state directory"),
        }];
        let found = walk(&[root.path().to_path_buf()], 6, &avoid);
        assert_eq!(found.repositories.len(), 1, "{found:?}");
        assert!(found.skipped.iter().any(|skip| skip.why.contains("state directory")), "{found:?}");
    }

    #[test]
    fn nfs_is_a_remote_filesystem_and_ext4_is_not() {
        assert!(remote("nfs4"));
        assert!(remote("fuse.sshfs"));
        assert!(!remote("ext4"));
        assert!(!remote("tmpfs"));
    }

    #[test]
    fn a_missing_root_is_an_empty_walk() {
        let found = walk(&[PathBuf::from("/this/path/is/not/there")], 6, &[]);
        assert!(found.repositories.is_empty());
        assert_eq!(found.skipped.len(), 1);
    }
}
