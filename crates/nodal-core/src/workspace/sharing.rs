//! Whether Nodal shares file blocks under the state root, asked once and then read.
//!
//! A unit home is a copy of a base, and it is cheap only where the filesystem can give
//! two files the same blocks. Where it cannot, [`super::copy::CopyFallback`] copies the
//! bytes and the clone report says so, which is one home too late: a person who put the
//! state root on ext4 has already paid for the copy before anything tells them.
//!
//! So the question is asked earlier, and it is asked **once**. The moment is when the
//! state root is made: [`crate::store::Store::open`] creates that directory, and
//! `nodal init` creates it too. The answer is written beside the registry as
//! [`FILE_NAME`], and every command after that reads the record. Asking again on every
//! command would write a file into the state root on every command, which is what
//! `nodal doctor` may not do and what a plan rebuilt after a crash has no business
//! doing either.
//!
//! It is asked the way the materializer asks it, by trying one clone, and not by
//! reading a name. A name cannot answer it: XFS shares blocks only when
//! `mkfs.xfs -m reflink=1` made it, `OpenZFS` shares them from 2.2, and overlayfs shares
//! them when its upper layer does. The name is reported beside the answer, never
//! instead of it, and a filesystem this build cannot name is reported as unnamed. One
//! backend answers both halves ([`super::sharing_backend`]), so the name in the report
//! and the call that makes a home come from the same place.
//!
//! The answer has three values. "Could not ask" is not "cannot share": a state root
//! that is full, read-only, over quota or owned by somebody else refuses the probe's
//! own write, and reading that as "this filesystem copies" demotes every home on a
//! btrfs disk that works perfectly well. The record carries the errno, and every line
//! about it says which of the three it is.
//!
//! The record is re-taken only when there is no record, when the state root's device
//! is not the device the record was taken on, or when `nodal init --reprobe` asks.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{nearest, probe_sharing, sharing_backend};
use crate::model::Timestamp;

/// The record's name, beside the registry in the state root.
pub const FILE_NAME: &str = "sharing.json";

/// The fix, named from the backend this build has. Nodal shares blocks with `FICLONE`
/// on Linux and `clonefile` on macOS, so the filesystems worth naming are the ones
/// those two calls work on and nothing else.
#[cfg(target_os = "linux")]
const FIX: &str = "move the state root to btrfs, to XFS formatted with reflink \
                   (mkfs.xfs -m reflink=1), or to bcachefs.";
/// The same sentence for the backend macOS builds have.
#[cfg(target_os = "macos")]
const FIX: &str = "move the state root to a volume in an APFS container.";
/// The same sentence for a build with no backend that shares blocks at all.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
const FIX: &str = "no backend in this build shares blocks on this platform.";

/// The filesystems that are a Windows disk reached through a translation layer. Both
/// are what a state root under `/mnt` on WSL is on, and neither clones anything.
///
/// The name is what says so, not the machine: a person reading a report on any host
/// gets the sentence when the name is one of these, and a WSL machine whose state root
/// is on a Linux disk does not get a sentence about a disk it is not using.
const WINDOWS_DISK: [&str; 2] = ["9p", "drvfs"];

/// What to do about a state root on a Windows disk, where the ordinary fix cannot
/// reach: no Linux filesystem is under it to move to.
const WINDOWS: &str = "this is a Windows disk mounted into Linux; put the state root on a Linux \
                       disk, not under /mnt.";

/// What `nodal init --reprobe` is for, said wherever a line leaves a person stuck.
const REPROBE: &str = "run nodal init --reprobe to ask again.";

/// Whether Nodal shares blocks, in the three values the question really has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shares {
    /// A clone was tried in the state root and it shared blocks.
    Yes,
    /// A clone was tried in the state root and the filesystem refused to share blocks.
    No,
    /// The question could not be put. The probe's own write failed, so the filesystem
    /// said nothing about sharing at all.
    CouldNotAsk {
        /// The number the operating system gave, and `None` where the failure did not
        /// come from a system call.
        errno: Option<i32>,
        /// The same failure in words.
        why: String,
    },
}

impl Shares {
    /// The answer a failed probe gives: what went wrong, kept as a number and a line.
    #[must_use]
    pub fn could_not_ask(error: &std::io::Error) -> Self {
        Self::CouldNotAsk { errno: error.raw_os_error(), why: error.to_string() }
    }

    /// One word for a log line and a message. Never "false": the third value exists
    /// because "no" is a claim about a filesystem that a failed write cannot make.
    #[must_use]
    pub const fn word(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::CouldNotAsk { .. } => "could-not-ask",
        }
    }
}

/// What the state root is, and whether Nodal shares blocks there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    /// The state root the answer is about.
    pub root: PathBuf,
    /// What the filesystem holding that path is called, and `null` where this build
    /// cannot name it. A name is for a person to read; it decides nothing.
    pub filesystem: Option<String>,
    /// Whether Nodal shares blocks between files there.
    pub shares: Shares,
    /// When the question was put. A record is a reading of one moment, and a person
    /// who moved their state root since is entitled to see when it was taken.
    pub probed_at: Timestamp,
    /// The device the state root was on when the question was put. A different device
    /// under the same path is a different filesystem, and is the one thing that re-asks
    /// the question without a person typing anything.
    pub device: Option<u64>,
}

impl Sharing {
    /// The recorded answer for `root`, and `None` where nothing recorded one.
    ///
    /// This is what `nodal doctor` calls. It reads a file and writes nothing, whatever
    /// it finds, because a report that probed would write into the directory it reports
    /// on.
    #[must_use]
    pub fn read(root: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path_in(root)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The recorded answer for `root`, taking it first where there is none to read.
    ///
    /// Every command that makes a home calls this. It writes only on the run that has
    /// no record to read, or whose record was taken on another device.
    #[must_use]
    pub fn ensure(root: &Path) -> Self {
        match Self::read(root) {
            Some(record) if record.is_current(root) => record,
            _ => Self::reprobe(root),
        }
    }

    /// Ask the question again and write the answer down, whatever was recorded before.
    /// This is `nodal init --reprobe`, and it is what a person runs after they move
    /// the state root or fix what was stopping the probe.
    #[must_use]
    pub fn reprobe(root: &Path) -> Self {
        drop(std::fs::create_dir_all(root));
        let record = Self::probe(root);
        record.write();
        record
    }

    /// Whether this record still describes `root`.
    ///
    /// The path has to be the one the record is about, and the device under it has to
    /// be the device the record was taken on. A state root that is now a different
    /// mount is a different filesystem with the same name.
    fn is_current(&self, root: &Path) -> bool {
        self.root == root && self.device.is_some() && self.device == device_of(root)
    }

    /// Ask the filesystem under `root` the materializer's own question, by trying it.
    ///
    /// The path need not exist yet. One lookup finds the nearest ancestor that is a
    /// directory, and that directory answers both halves: what the filesystem is called
    /// and whether a clone works in it.
    #[must_use]
    pub fn probe(root: &Path) -> Self {
        let taken = |filesystem, shares, device| Self {
            root: root.to_path_buf(),
            filesystem,
            shares,
            probed_at: Timestamp::now(),
            device,
        };
        let Some(existing) = nearest(root) else {
            return taken(None, Shares::could_not_ask(&missing()), None);
        };
        let device = device_of(existing);
        let Some(backend) = sharing_backend() else {
            return taken(None, Shares::No, device);
        };
        tracing::debug!(
            root = %root.display(),
            asked = %existing.display(),
            backend = backend.name(),
            "the nearest existing ancestor of the state root answers for it"
        );
        let filesystem = backend.filesystem(existing);
        let shares = probe_sharing(existing, |source, to| backend.clone_probe(source, to));
        taken(filesystem, shares, device)
    }

    /// Write the record beside the registry.
    ///
    /// A failure is not reported. The record is a reading a probe can take again, and a
    /// state root that cannot be written to is already the case this record describes;
    /// failing a `nodal new` over it would be the wrong end of the same problem.
    fn write(&self) {
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        drop(std::fs::write(path_in(&self.root), text));
    }

    /// The fact, as one line, in whichever of the three cases this machine is in.
    /// `nodal doctor` prints this, so that a person can see which case they are in.
    #[must_use]
    pub fn fact(&self) -> String {
        let (root, filesystem) = (self.root.display(), self.names());
        match &self.shares {
            Shares::Yes => format!(
                "{root} {filesystem}. nodal shares blocks here, so a unit home costs almost no \
                 disk."
            ),
            Shares::No => format!(
                "{root} {filesystem}. nodal does not share blocks here, so each unit home is a \
                 full copy."
            ),
            Shares::CouldNotAsk { errno, why } => format!(
                "{root} {filesystem}. nodal could not ask whether it shares blocks here: {why}{}. \
                 until it can, each unit home is a full copy.",
                errno.map(|number| format!(" (errno {number})")).unwrap_or_default()
            ),
        }
    }

    /// The fact and the fix, as one line, and `None` where there is nothing to fix.
    /// `nodal init` prints this once, at the moment a person sets a project up.
    #[must_use]
    pub fn advice(&self) -> Option<String> {
        let fix = match &self.shares {
            Shares::Yes => return None,
            Shares::No if self.on_a_windows_disk() => WINDOWS,
            Shares::No => FIX,
            Shares::CouldNotAsk { .. } => REPROBE,
        };
        Some(format!("{} {fix}", self.fact()))
    }

    /// How the fact names the filesystem, which is never the subject of the sentence.
    /// A name that is missing has to leave a sentence that still says what happened.
    fn names(&self) -> String {
        self.filesystem.as_ref().map_or_else(
            || String::from("is on a filesystem nodal cannot name"),
            |name| format!("is on {name}"),
        )
    }

    /// Whether the named filesystem is a Windows disk mounted into Linux.
    fn on_a_windows_disk(&self) -> bool {
        self.filesystem.as_ref().is_some_and(|name| WINDOWS_DISK.contains(&name.as_str()))
    }
}

/// Where the record lives: beside the registry, in the state root.
#[must_use]
pub fn path_in(root: &Path) -> PathBuf {
    root.join(FILE_NAME)
}

/// The line for a state root nothing has recorded an answer for.
///
/// `nodal doctor` prints this rather than asking, because asking writes. A machine that
/// was set up by an older Nodal reads this once, and the first command that makes a
/// home records the answer.
#[must_use]
pub fn unrecorded(root: &Path) -> String {
    format!(
        "{root} — nodal has not recorded whether it shares blocks here. {REPROBE}",
        root = root.display()
    )
}

/// The error a path with no directory anywhere above it stands for.
fn missing() -> std::io::Error {
    std::io::Error::from_raw_os_error(libc::ENOENT)
}

/// The device `path` is on, and `None` where it could not be read.
#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;

    std::fs::metadata(path).ok().map(|metadata| metadata.dev())
}

/// A host that does not publish a device number answers nothing, which re-asks the
/// question on every run rather than trusting a record it cannot check.
#[cfg(not(unix))]
fn device_of(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Shares, Sharing, path_in, unrecorded};
    use crate::model::Timestamp;

    /// The answer a test forces, for each of the two filesystems it stands for.
    fn answer(shares: bool) -> Sharing {
        record(
            if shares { Shares::Yes } else { Shares::No },
            Some(String::from(if shares { "btrfs" } else { "ext4" })),
        )
    }

    /// A record about a state root no test touches.
    fn record(shares: Shares, filesystem: Option<String>) -> Sharing {
        Sharing {
            root: PathBuf::from("/home/j/.nodal"),
            filesystem,
            shares,
            probed_at: Timestamp::now(),
            device: Some(66),
        }
    }

    /// The failure a read-only state root gives the probe's own write.
    fn refused() -> Shares {
        Shares::could_not_ask(&std::io::Error::from_raw_os_error(libc::EROFS))
    }

    #[test]
    fn init_says_nothing_when_nodal_shares_blocks() {
        assert_eq!(answer(true).advice(), None);
        assert!(answer(true).fact().contains("nodal shares blocks here"));
    }

    #[test]
    fn init_names_the_filesystem_and_the_fix_when_nodal_does_not_share_blocks() {
        let line = answer(false).advice().expect("a line about a state root that copies");
        assert!(line.contains("/home/j/.nodal"), "{line}");
        assert!(line.contains("ext4"), "{line}");
        assert!(line.contains("full copy"), "{line}");
        assert_eq!(line.lines().count(), 1, "one line: {line}");
    }

    /// The fix names the filesystems this build can share blocks on, and no others. A
    /// person on macOS reading about a mount option would go looking for one that is
    /// not there.
    #[test]
    fn the_fix_names_the_backends_this_build_has() {
        let line = answer(false).advice().expect("a line about a state root that copies");
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

    /// The name decides the sentence, not the machine. A state root on a Windows disk
    /// gets the fix that can be carried out; the Linux fix names filesystems that are
    /// not reachable from `/mnt`.
    #[test]
    fn a_windows_disk_is_named_as_one_on_any_host() {
        for name in ["9p", "drvfs"] {
            let line = record(Shares::No, Some(String::from(name)))
                .advice()
                .expect("a line about a state root that copies");
            assert!(line.contains("Windows disk mounted into Linux"), "{line}");
            assert!(!line.contains("mkfs.xfs"), "{line}");
        }
    }

    /// A machine whose filesystem is not a Windows disk reads no sentence about one.
    #[test]
    fn an_ordinary_filesystem_reads_no_windows_sentence() {
        let line = answer(false).advice().expect("a line about a state root that copies");
        assert!(!line.contains("Windows"), "{line}");
    }

    /// A state root the probe could not write in has said nothing about sharing. The
    /// line says that, and carries the errno, and never says the filesystem cannot.
    #[test]
    fn a_state_root_that_could_not_be_asked_is_not_a_filesystem_that_cannot_share() {
        let unasked = record(refused(), Some(String::from("btrfs")));
        let fact = unasked.fact();
        assert!(fact.contains("could not ask"), "{fact}");
        assert!(fact.contains(&format!("errno {}", libc::EROFS)), "{fact}");
        assert!(!fact.contains("does not share blocks"), "{fact}");
        let line = unasked.advice().expect("a line about a state root nobody could ask");
        assert!(line.contains("--reprobe"), "{line}");
    }

    /// A filesystem this build cannot name is said to be unnamed. The old wording put
    /// the name in the subject of the sentence, so an unnamed one blamed a filesystem
    /// that was never identified.
    #[test]
    fn an_unnamed_filesystem_is_reported_as_unnamed() {
        let fact = record(Shares::No, None).fact();
        assert!(fact.contains("a filesystem nodal cannot name"), "{fact}");
        assert!(fact.contains("nodal does not share blocks here"), "{fact}");
        assert!(!fact.contains("unknown filesystem cannot"), "{fact}");
    }

    /// `--json` carries the missing name as `null`, and the answer as one word.
    #[test]
    fn a_record_is_json_a_person_can_read() {
        let json = serde_json::to_string(&record(Shares::No, None)).expect("a serialisable answer");
        assert!(json.contains(r#""filesystem":null"#), "{json}");
        assert!(json.contains(r#""shares":"no""#), "{json}");
        let unasked =
            serde_json::to_string(&record(refused(), None)).expect("a serialisable answer");
        assert!(unasked.contains(r#""could-not-ask""#), "{unasked}");
    }

    /// A record written and read back is the same record. Every reader after the probe
    /// depends on this and on nothing else.
    #[test]
    fn a_record_survives_being_written_and_read() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let taken = Sharing::ensure(directory.path());
        assert!(path_in(directory.path()).is_file(), "the record was not written");
        assert_eq!(Sharing::read(directory.path()), Some(taken));
    }

    /// A second reader takes no probe. The record is the answer, and a probe writes.
    #[test]
    fn a_second_reader_reads_the_record_rather_than_asking_again() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let first = Sharing::ensure(directory.path());
        let again = Sharing::ensure(directory.path());
        assert_eq!(first.probed_at, again.probed_at, "the question was put twice");
    }

    /// A record taken on another device is not about this state root. Somebody who
    /// mounted a disk over their state root gets the question asked again.
    #[test]
    fn a_device_that_changed_forces_the_question_again() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let first = Sharing::ensure(directory.path());
        let mut moved = first.clone();
        moved.device = Some(first.device.unwrap_or_default().wrapping_add(1));
        moved.write();
        let again = Sharing::ensure(directory.path());
        assert_eq!(again.device, first.device, "the record was not taken again");
        assert!(again.probed_at >= first.probed_at);
    }

    /// A state root nothing can be written in is asked and answers "could not ask".
    #[test]
    fn a_state_root_that_cannot_be_written_in_could_not_be_asked() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path().join("read-only");
        std::fs::create_dir(&root).expect("a directory");
        read_only(&root);
        let taken = Sharing::probe(&root);
        writable(&root);
        assert!(
            matches!(taken.shares, Shares::CouldNotAsk { .. }),
            "a read-only state root answered {:?}",
            taken.shares
        );
        assert!(taken.fact().contains("could not ask"), "{}", taken.fact());
    }

    /// Doctor's line for a machine with no record says so and names the command that
    /// takes one. It never says the filesystem copies, which nobody has asked it.
    #[test]
    fn a_state_root_with_no_record_is_reported_as_unrecorded() {
        let line = unrecorded(Path::new("/home/j/.nodal"));
        assert!(line.contains("/home/j/.nodal"), "{line}");
        assert!(line.contains("has not recorded"), "{line}");
        assert!(line.contains("--reprobe"), "{line}");
        assert!(!line.contains("full copy"), "{line}");
    }

    /// Make a directory unwritable, and give it back, so the test leaves the machine
    /// as it found it whichever way it ends.
    fn read_only(path: &Path) {
        set_mode(path, 0o500);
    }

    /// The permissions a temporary directory has.
    fn writable(path: &Path) {
        set_mode(path, 0o700);
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("a directory whose permissions can be set");
    }

    #[cfg(not(unix))]
    fn set_mode(_path: &Path, _mode: u32) {}
}
