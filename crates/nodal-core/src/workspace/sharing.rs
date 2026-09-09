//! Whether Nodal shares file blocks under the state root, in words a person reads.
//!
//! A unit home is a copy of a base, and it is cheap only where the filesystem can give
//! two files the same blocks. Where it cannot, [`super::copy::CopyFallback`] copies the
//! bytes and the clone report says so, which is one home too late: a person who put the
//! state root on ext4 has already paid for the copy before anything tells them.
//!
//! So the same question is asked earlier, by `nodal init` once and by `nodal doctor`
//! every time. It is asked the way the materializer asks it, by trying one clone, and
//! not by reading a name. A name cannot answer it: XFS shares blocks only when
//! `mkfs.xfs -m reflink=1` made it, `OpenZFS` shares them from 2.2, and overlayfs shares
//! them when its upper layer does. The name is reported beside the answer, never
//! instead of it, and a filesystem this build cannot name is reported as unnamed.
//!
//! The report says what Nodal does, not what a filesystem is able to do. Those are the
//! same sentence only when this build has a backend for the filesystem in front of it.
//!
//! Nothing here changes what a unit does. It writes one small file, clones it, and
//! removes both. It is one fact and, where the fact is bad news, the one thing that
//! fixes it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{apfs, nearest, reflink};

/// The fix, named from the backends this build has. Nodal shares blocks with `FICLONE`
/// on Linux and `clonefile` on macOS, so the filesystems worth naming are the ones
/// those two calls work on and nothing else.
#[cfg(target_os = "linux")]
const FIX: &str = "move the state root to btrfs, to XFS formatted with reflink \
                   (mkfs.xfs -m reflink=1), or to bcachefs.";
#[cfg(target_os = "macos")]
const FIX: &str = "move the state root to a volume in an APFS container.";
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const FIX: &str = "no backend in this build shares blocks on this platform.";

/// What a person on WSL2 does instead, where the Linux fix does not reach: a state root
/// under `/mnt` is on a Windows disk through a translation layer, which clones nothing.
const WSL: &str = "on WSL2, put the state root on a btrfs virtual disk, not under /mnt.";

/// What the state root is, and whether Nodal shares blocks there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    /// The state root the answer is about.
    pub root: PathBuf,
    /// What the filesystem holding that path is called, and `null` where this build
    /// cannot name it. A name is for a person to read; it decides nothing.
    pub filesystem: Option<String>,
    /// Whether Nodal shares blocks between files there.
    pub shares: bool,
}

impl Sharing {
    /// Ask the filesystem under `root` the materializer's own question, by trying it.
    ///
    /// The path need not exist yet. One lookup finds the nearest ancestor that does,
    /// and that directory answers both halves: what the filesystem is called and
    /// whether a clone works in it. A path with no ancestor at all shares nothing.
    #[must_use]
    pub fn probe(root: &Path) -> Self {
        let root = root.to_path_buf();
        let Some(existing) = nearest(&root) else {
            return Self { root, filesystem: None, shares: false };
        };
        tracing::debug!(
            root = %root.display(),
            asked = %existing.display(),
            "the nearest existing ancestor of the state root answers for it"
        );
        let filesystem = reflink::filesystem(existing).or_else(|| apfs::filesystem(existing));
        let shares = reflink::shares_blocks(existing) || apfs::shares_blocks(existing);
        Self { root, filesystem, shares }
    }

    /// The fact, as one line. `nodal doctor` prints this in both cases, so that a
    /// person can see which case they are in.
    #[must_use]
    pub fn fact(&self) -> String {
        let root = self.root.display();
        let filesystem = self.filesystem.as_ref().map_or_else(
            || String::from("is on a filesystem nodal cannot name"),
            |name| format!("is on {name}"),
        );
        if self.shares {
            return format!(
                "{root} {filesystem}. nodal shares blocks here, so a unit home costs almost no \
                 disk."
            );
        }
        format!(
            "{root} {filesystem}. nodal does not share blocks here, so each unit home is a full \
             copy."
        )
    }

    /// The fact and the fix, as one line, and `None` where there is nothing to fix.
    /// `nodal init` prints this once, at the moment a person sets a project up.
    #[must_use]
    pub fn advice(&self) -> Option<String> {
        self.advice_on(on_wsl())
    }

    /// The same line for a stated host, so that both hosts are read on either one.
    fn advice_on(&self, wsl: bool) -> Option<String> {
        if self.shares {
            return None;
        }
        let mut line = format!("{} {FIX}", self.fact());
        if wsl {
            line.push(' ');
            line.push_str(WSL);
        }
        Some(line)
    }
}

/// Whether this machine is WSL. `WSL_DISTRO_NAME` is set in every WSL shell, and the
/// kernel release names Microsoft, so a machine that is not WSL never reads the
/// sentence and a machine that is always does.
fn on_wsl() -> bool {
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }
    std::fs::read_to_string("/proc/version").is_ok_and(|version| {
        let version = version.to_ascii_lowercase();
        version.contains("microsoft") || version.contains("wsl")
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::Sharing;

    /// The answer a test forces, for each of the two filesystems it stands for.
    fn answer(shares: bool) -> Sharing {
        Sharing {
            root: PathBuf::from("/home/j/.nodal"),
            filesystem: Some(String::from(if shares { "btrfs" } else { "ext4" })),
            shares,
        }
    }

    #[test]
    fn init_says_nothing_when_nodal_shares_blocks() {
        assert_eq!(answer(true).advice(), None);
        assert!(answer(true).fact().contains("nodal shares blocks here"));
    }

    #[test]
    fn init_names_the_filesystem_and_the_fix_when_nodal_does_not_share_blocks() {
        let line = answer(false).advice_on(false).expect("a line about a state root that copies");
        assert!(line.contains("/home/j/.nodal"), "{line}");
        assert!(line.contains("ext4"), "{line}");
        assert!(line.contains("full copy"), "{line}");
        assert!(!line.contains("WSL2"), "a machine that is not WSL reads no WSL2 line: {line}");
        assert_eq!(line.lines().count(), 1, "one line: {line}");
    }

    /// The fix names the filesystems this build can share blocks on, and no others. A
    /// person on macOS reading about a mount option would go looking for one that is
    /// not there.
    #[test]
    fn the_fix_names_the_backends_this_build_has() {
        let line = answer(false).advice_on(false).expect("a line about a state root that copies");
        if cfg!(target_os = "linux") {
            assert!(line.contains("btrfs"), "{line}");
            assert!(line.contains("bcachefs"), "{line}");
            assert!(line.contains("mkfs.xfs -m reflink=1"), "{line}");
            assert!(!line.contains("APFS"), "{line}");
        } else if cfg!(target_os = "macos") {
            assert!(line.contains("APFS"), "{line}");
            assert!(!line.contains("btrfs"), "{line}");
        }
    }

    #[test]
    fn only_a_wsl_machine_reads_the_wsl_line() {
        let line = answer(false).advice_on(true).expect("a line about a state root that copies");
        assert!(line.contains("WSL2"), "{line}");
        assert!(line.contains("btrfs virtual disk"), "{line}");
    }

    /// A filesystem this build cannot name is said to be unnamed. The old wording put
    /// the name in the subject of the sentence, so an unnamed one blamed a filesystem
    /// that was never identified.
    #[test]
    fn an_unnamed_filesystem_is_reported_as_unnamed() {
        let unnamed =
            Sharing { root: PathBuf::from("/home/j/.nodal"), filesystem: None, shares: false };
        let fact = unnamed.fact();
        assert!(fact.contains("a filesystem nodal cannot name"), "{fact}");
        assert!(fact.contains("nodal does not share blocks here"), "{fact}");
        assert!(!fact.contains("unknown filesystem cannot"), "{fact}");
    }

    /// `--json` carries the missing name as `null`, not as a sentence.
    #[test]
    fn an_unnamed_filesystem_is_null_in_json() {
        let unnamed =
            Sharing { root: PathBuf::from("/home/j/.nodal"), filesystem: None, shares: false };
        let json = serde_json::to_string(&unnamed).expect("a serialisable answer");
        assert!(json.contains(r#""filesystem":null"#), "{json}");
    }

    /// The probe answers about the path it was given, whether or not that path exists.
    /// It is the state root a person set, not the ancestor that happened to answer.
    #[test]
    fn the_probe_answers_about_the_state_root_it_was_given() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let unmade = directory.path().join("state").join("homes");
        assert_eq!(Sharing::probe(&unmade).root, unmade);
        assert_eq!(Sharing::probe(directory.path()).root, directory.path());
    }

    /// A path with no ancestor that exists is not on a filesystem anybody can ask.
    #[test]
    fn a_root_with_no_existing_ancestor_shares_nothing() {
        let answer = Sharing::probe(Path::new("relative/path/that/does/not/exist"));
        assert!(!answer.shares);
        assert_eq!(answer.filesystem, None);
    }
}
