//! The process table, read for the variables, the directory and the command a process
//! carries.
//!
//! A session is not something Nodal is told about; it is something Nodal can see. A
//! shell that entered an activated home carries `NODAL_ID`, and so does every process
//! that shell starts. Reading the process table is therefore how "who is in this unit"
//! is answered, and it is why nothing has to be installed into a person's repository
//! and no shell has to be wrapped.
//!
//! [`Processes`] is the seam, so that a test supplies a table instead of a machine.
//! [`Live`] is the one implementation: on Linux it reads `/proc/<pid>`, and on macOS it
//! asks the kernel through `libproc` and `sysctl`.
//!
//! **What macOS refuses.** Two readings are refused, and a scan says so rather than
//! leaving the process out without a word ([`Withheld`]):
//!
//! - A process of another account. `proc_pidinfo` and `KERN_PROCARGS2` answer `EPERM`,
//!   so its variables, its working directory and its command are not known. Its
//!   identifier, its group and its session are still readable.
//! - A process of this account that runs a restricted binary, which the kernel marks
//!   `CS_RESTRICT`. `KERN_PROCARGS2` gives its command, and zeroes its variables. Most
//!   programs under `/bin` and `/usr/bin` are marked, `/bin/zsh` among them, so a shell
//!   in a home is found by its directory and never by its `NODAL_ID`.
//!
//! The layout of the `KERN_PROCARGS2` buffer is not documented by Apple. It is the one
//! `ps` reads: the argument count, the program's path, padding, the arguments and the
//! variables, each ended by a zero byte. A buffer that does not have this shape gives no
//! command and no variables, and never a guess.
//!
//! A scan keeps four things about a process, and each answers one question:
//!
//! - the variables it was asked for — the `NODAL_*` set and the names that say who the
//!   actor is — which name the unit a process is in outright;
//! - its working directory, which says which home a process stands in when it carries
//!   no variables at all, for example a terminal that was never activated;
//! - a short form of its command, so a person reading `nodal ps` recognises the row;
//! - what it **holds** ([`Held`]): the paths it has open for writing, mapped writably, or
//!   is rooted at. A working directory alone answers where a process is standing and not
//!   what it would keep writing into after the directory moved, and those are different
//!   questions. A test runner started from a terminal that has since changed directory,
//!   writing its database into a home, stands nowhere near that home and occupies it
//!   completely.
//!
//! Everything else a process carries is read and dropped. **A process this account cannot
//! read is kept on both hosts**, with what was withheld said out loud, so that a reader
//! can say how much it could not see. It used to be dropped on Linux, which made the
//! reading silent in exactly the case the safety contract is written for.
//!
//! The command is deliberately short, and not the command line. A command line can hold
//! a credential — `psql postgres://user:password@host/db` — and Nodal writes no secret
//! value anywhere. So a command is the file name of the program, and the word after it
//! only when that word is a plain word: letters, digits, dash, dot or underscore. A
//! subcommand (`next dev`, `pnpm test`) survives that rule; a URL, an assignment or an
//! option does not. Both parts are cut to a column's width, because a command line is
//! written by whatever started the process and its length is not Nodal's to trust.
//!
//! The reading itself is in the `linux` and `macos` modules, one per host, over the one
//! list of variables a scan keeps ([`kept`]).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Result;
use crate::model::{Holding, Timestamp};

/// One process, with what a scan keeps about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The process identifier.
    pub pid: u32,
    /// The variables that were kept, by name.
    pub vars: BTreeMap<String, String>,
    /// Its working directory, when this account may read it.
    pub cwd: Option<PathBuf>,
    /// A short form of its command, when the table has one.
    pub command: Option<String>,
    /// The paths it holds in a way that would survive the directory being renamed under
    /// it: a descriptor opened for writing, a writable mapping, or the root it sees.
    ///
    /// Empty on a host that does not publish them ([`OCCUPANCY`]), which is not the same
    /// fact as a process holding nothing, and is why the evidence record says which
    /// readings the host answered.
    pub held: Vec<Held>,
    /// Where it came from, as far as the host will say. Readable for a process this
    /// account may not otherwise read at all, which is the whole reason it is kept.
    pub lineage: Lineage,
    /// What the host refused to show about it, `None` where nothing was refused.
    pub withheld: Option<Withheld>,
}

/// Where a process came from: what started it, and which group it is in.
///
/// **Read for the process a scan cannot read.** On Linux `/proc/<pid>/stat` stays
/// world-readable when `environ`, `cwd`, `fd` and `maps` do not — a setuid binary is
/// marked undumpable, not invisible — and on macOS `proc_bsdshortinfo` answers across
/// accounts. So the one thing still knowable about a process this account is refused is
/// where it came from, and [`crate::lifecycle::assess`] judges it on that rather than
/// pretending it is not there. Both hosts answer both fields.
///
/// **The session is not here, and that is deliberate.** Both hosts publish one — Linux
/// in `stat`, macOS through `getsid`, for any account — and reading it was a mistake: a
/// shell standing in a home is usually its own session leader, so every unrelated command
/// in that terminal shares the number and nothing else, and matching on it refused
/// reclaims over processes that had never been near the home
/// (`assess::descends_from` states the whole rule). A field nothing reads is a field a
/// later change reads again, so it is gone rather than ignored.
///
/// Every field is optional and every absence means the host did not say. Nothing here
/// is ever read as a name to signal: it takes a process out of the unknown list or
/// leaves it there, exactly as [`crate::lifecycle::assess::Own`]'s groups do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lineage {
    /// The process that started it.
    pub parent: Option<u32>,
    /// The process group it is in.
    pub group: Option<u32>,
}

/// One path a process holds, and the hold it has on it.
///
/// A working directory is not in here. That one is [`Running::cwd`], it has always been
/// read, and the whole point of this list is the occupancy a working directory misses: a
/// process whose directory is `/` and whose descriptor is writing a file two directories
/// inside a home is occupying that home, and `lsof +D` and `fuser -m` have said so for
/// thirty years.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The path, exactly as the host named it.
    pub path: PathBuf,
    /// The hold.
    pub how: How,
}

impl Held {
    /// One hold on one path.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, how: How) -> Self {
        Self { path: path.into(), how }
    }

    /// The hold and the path, in the words a refusal prints.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} {}", self.how.label(), self.path.display())
    }
}

/// How a process holds a path.
///
/// Only holds that outlive a rename are here, and only the ones that lose a person work.
/// A descriptor opened for **reading** is not one of them and is never kept: an editor, a
/// language server, a `tail` and a `grep` all hold those, none of them writes through
/// one, and a rule that refused over them would refuse every reclaim on a working
/// machine. `/proc/<pid>/fdinfo/<n>` carries the open flags, so the two cases separate at
/// the cost of one file read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    /// A descriptor opened for writing. The process keeps writing into the inode after
    /// the directory above it is renamed, and never learns that it moved.
    Descriptor,
    /// A shared mapping made writable: the same fact reached through memory rather than
    /// through a descriptor, which is how a memory-mapped database holds its file.
    Mapping,
    /// The root directory the process sees. A process rooted inside the home cannot be
    /// moved out from under at all.
    Root,
}

impl How {
    /// The word a refusal and a report use.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Descriptor => "writing",
            Self::Mapping => "mapping",
            Self::Root => "rooted at",
        }
    }
}

/// Which readings of occupancy this host publishes.
///
/// Named rather than inferred, because it is what the evidence record prints: a verdict
/// taken where the descriptors could not be read is a different verdict from one taken
/// where they were read and there were none, and the two must not print alike.
pub const OCCUPANCY: &[&str] =
    if cfg!(target_os = "linux") { &["cwd", "fd_write", "mmap_write", "root"] } else { &["cwd"] };

/// What the host refused to show about one process, and so what a reader cannot know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Withheld {
    /// The process belongs to another account — or to this one and running a binary the
    /// kernel marks undumpable, which is what a setuid program becomes. Its variables,
    /// its working directory and what it holds are all refused.
    AnotherAccount,
    /// The process runs a restricted binary. Its working directory and its command are
    /// shown, and its variables are not.
    Restricted,
}

impl Running {
    /// A process known by its variables alone, which is what a test supplies.
    #[must_use]
    pub fn new(pid: u32, vars: BTreeMap<String, String>) -> Self {
        Self {
            pid,
            vars,
            cwd: None,
            command: None,
            held: Vec::new(),
            lineage: Lineage::default(),
            withheld: None,
        }
    }

    /// The same process, standing in `cwd`.
    #[must_use]
    pub fn in_directory(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// The same process, running `command`.
    #[must_use]
    pub fn running(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// The same process, holding these paths, which is what a test supplies where a
    /// machine would have supplied `/proc`.
    #[must_use]
    pub fn holding(mut self, held: Vec<Held>) -> Self {
        self.held = held;
        self
    }

    /// The same process, with where it came from.
    #[must_use]
    pub const fn from(mut self, lineage: Lineage) -> Self {
        self.lineage = lineage;
        self
    }

    /// The same process, with what the host refused to show about it.
    #[must_use]
    pub const fn withholding(mut self, withheld: Withheld) -> Self {
        self.withheld = Some(withheld);
        self
    }

    /// One variable, when the process carries it.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }
}

/// Where the process table comes from.
pub trait Processes {
    /// Every process this account can read, with what is worth keeping about it.
    ///
    /// # Errors
    /// [`Error::ProcessScanUnsupported`](crate::Error::ProcessScanUnsupported) on a host
    /// with no readable process table, and [`Error::Io`](crate::Error::Io) when the
    /// table itself cannot be listed.
    fn scan(&self) -> Result<Vec<Running>>;

    /// What the table says about identifiers somebody wrote down.
    ///
    /// This is asked of numbers from lock rows rather than of processes a scan found,
    /// and the two readings are not the same question. A scan keeps what it can read,
    /// and it can read neither the variables nor the working directory of another
    /// account's process, so a scan that does not name a process does not prove the
    /// process is gone. [`Live`] therefore overrides this with the one reading that does
    /// cross accounts, and nothing here ever signals anything.
    ///
    /// Every identifier is answered from one reading of the table, because the caller is
    /// a list and a list of eight units must not read the machine eight times.
    ///
    /// The answer supplied here is the right one for a table a test states: that table
    /// is the whole of the machine the test is describing, and it dates nothing.
    ///
    /// # Errors
    /// Whatever [`Processes::scan`] reports, which on a host with no readable process
    /// table is [`Error::ProcessScanUnsupported`](crate::Error::ProcessScanUnsupported):
    /// "I cannot see" is a different answer from "it is gone".
    fn presences(&self, pids: &[u32]) -> Result<BTreeMap<u32, Presence>> {
        let table = self.scan()?;
        Ok(pids
            .iter()
            .map(|pid| {
                let found = table.iter().any(|running| running.pid == *pid);
                (*pid, if found { Presence::Running { started_at: None } } else { Presence::Gone })
            })
            .collect())
    }
}

/// What the process table says about one identifier somebody wrote down.
///
/// The instant matters as much as the answer. Identifiers are reused, so a machine that
/// has been up for a week can hand a lock row's number to something else entirely, and a
/// reader that only asked "is there a process" would report a session that ended months
/// ago as being back at work. A process that started after the hold was taken is not the
/// process that took it ([`crate::runtime::lock::liveness`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// No process carries that identifier now.
    Gone,
    /// One does, and this is when it started, where the host says so.
    Running {
        /// When the process started, `None` on a host that does not say.
        started_at: Option<Timestamp>,
    },
}

/// The processes of this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

impl Processes for Live {
    fn scan(&self) -> Result<Vec<Running>> {
        scan_this_host()
    }

    /// On Linux the kernel publishes a directory per process, and the directory is there
    /// whichever account owns the process. So this answers for a process a scan cannot
    /// read, which is the case that matters: a lock row written by another engineer on a
    /// host two people share. Nothing is opened for writing and nothing is signalled.
    ///
    /// Each process is dated as well as found, from the same two files, so a caller can
    /// tell a hold's own process from a later one wearing its number.
    ///
    /// On macOS the kernel dates a process of this account. A process of another account
    /// is found and not dated, which [`Presence::Running`] states as `started_at: None`.
    fn presences(&self, pids: &[u32]) -> Result<BTreeMap<u32, Presence>> {
        #[cfg(target_os = "linux")]
        {
            Ok(linux::presences(pids))
        }
        #[cfg(target_os = "macos")]
        {
            macos::presences(pids)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = pids;
            Err(crate::Error::ProcessScanUnsupported { host: std::env::consts::OS })
        }
    }
}

#[cfg(target_os = "linux")]
fn scan_this_host() -> Result<Vec<Running>> {
    linux::scan()
}

#[cfg(target_os = "macos")]
fn scan_this_host() -> Result<Vec<Running>> {
    macos::scan()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn scan_this_host() -> Result<Vec<Running>> {
    Err(crate::Error::ProcessScanUnsupported { host: std::env::consts::OS })
}

/// The process that started `pid`, when this host publishes it.
///
/// A reading of the machine now, taken once per recorded group rather than once per
/// process: it answers whether a process is the `nodal run` that a group Nodal recorded
/// hangs off ([`crate::lifecycle::assess::Own`]). A host that publishes no process table
/// answers `None`, which leaves the stricter reading standing.
#[must_use]
pub fn parent_of(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        linux::parent_of(pid)
    }
    #[cfg(target_os = "macos")]
    {
        macos::parent_of(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// This process, named by its identifier and pinned to the instant it started.
///
/// What a hold records for its holder ([`crate::model::Holding`]). The identifier is
/// asked of the kernel and the instant is read from this host's own table, so the pin a
/// hold is written with is taken the same way as every later reading it is compared
/// against: one number, one arithmetic, one answer.
///
/// `started_at` is `None` where this host would not date the process. The record then
/// says what was read and no more, and a later reading of that row resolves no identity
/// and proves nothing ([`crate::runtime::lock::liveness`]).
///
/// One process is asked for, so this is two small reads of the table and never a scan.
#[must_use]
pub fn current_process() -> Holding {
    let pid = std::process::id();
    Holding { pid, started_at: started_at(pid) }
}

/// When the process wearing this identifier now started, from this host's own record.
///
/// `None` where this host publishes no process table, where the process has gone, and
/// where the host dates no process it found. Each of those is "I could not read it", and
/// every caller treats it as proof of nothing — which of the two directions that is
/// safe in belongs to the caller, not here. A hold reads it and stands
/// ([`crate::runtime::lock::liveness`]); a reclaim reads it and refuses
/// ([`crate::lifecycle::assess`]).
///
/// One process is asked for, so this is two small reads of the table and never a scan.
#[must_use]
pub fn started_at(pid: u32) -> Option<Timestamp> {
    match Live.presences(&[pid]).ok()?.get(&pid) {
        Some(Presence::Running { started_at }) => *started_at,
        Some(Presence::Gone) | None => None,
    }
}

/// The POSIX session this process is in, `None` where the host will not say.
///
/// This is what a hold records as its lineage ([`crate::model::Lock::session`]). It is
/// the session and not the process group because a shell with job control puts every
/// foreground command in a group of its own: two `nodal run` commands typed one after
/// the other are two groups, and matching on a group would make the second one a
/// stranger. Both are in the session of the shell that started them.
///
/// `getsid` takes no pointer and reads one number about a process this call already is.
#[must_use]
pub fn current_session() -> Option<u32> {
    // SAFETY: `getsid` reads one number about the calling process and takes no pointer.
    // Zero names this process. A failure answers -1, which the conversion rejects.
    let answered = unsafe { libc::getsid(0) };
    u32::try_from(answered).ok()
}

/// Whether the session `sid` names still holds a process on this host.
///
/// `None` is "I cannot see", and it is never "it is gone": the two are different answers
/// and only one of them lets a hold go ([`crate::runtime::lock::enter`]). Three things
/// answer `None` — a host that publishes no process table, a table this call could not
/// list, and a table holding an entry this account may not read. The last one matters on
/// the host the lock exists for: two people on one box, where a reading that treated
/// another account's live session as gone would hand away a hold nobody let go of.
///
/// A session is asked for rather than a process because the process that took a hold
/// exits at the end of its command, while the shell that started it does not. Asking
/// after the process would report every ordinary hold as lapsed a second after it was
/// taken.
#[must_use]
pub fn session_is_live(sid: u32) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        linux::session_is_live(std::path::Path::new(linux::PROC), sid)
    }
    #[cfg(target_os = "macos")]
    {
        macos::session_is_live(sid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = sid;
        None
    }
}

/// The same reading, against a process table a test states instead of this machine's.
///
/// `/proc` cannot be taken away from the host a test runs on, so the directory is the
/// seam, in the way [`Processes`] is the seam for a scan. It is what pins the answers
/// that matter and cannot otherwise be reached: a table that cannot be listed, and a
/// table holding a record this account cannot read.
///
/// # Errors
/// None. `None` is an answer here and not a failure.
#[cfg(target_os = "linux")]
#[must_use]
pub fn session_is_live_in(table: &std::path::Path, sid: u32) -> Option<bool> {
    linux::session_is_live(table, sid)
}

/// Reading the process table of a Linux host, which publishes it as `/proc`.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{Held, How, Lineage, Presence, Running, Timestamp, Withheld, command_of, kept};
    use crate::{Error, Result};

    /// Where the kernel publishes the process table.
    pub(super) const PROC: &str = "/proc";

    /// The name of the file in a process's directory that holds its environment.
    const ENVIRON: &str = "environ";

    /// The link in a process's directory that points at its working directory.
    const CWD: &str = "cwd";

    /// The file in a process's directory that holds its command line, as NUL-separated
    /// arguments.
    const CMDLINE: &str = "cmdline";

    /// Every process this account can read, with what is worth keeping about it.
    pub(super) fn scan() -> Result<Vec<Running>> {
        let directory = Path::new(PROC);
        let entries = std::fs::read_dir(directory).map_err(Error::io(directory))?;
        let mut running = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            if let Some(process) = read(pid, &entry.path()) {
                running.push(process);
            }
        }
        Ok(running)
    }

    /// One process, or nothing where there is no process left to describe.
    ///
    /// **A process this account may not read is kept, and says so.** It used to be
    /// dropped, which made the one case the safety contract is written for — "cannot
    /// see, therefore not safe" — the one case that produced no evidence at all: a
    /// process standing in a home under a setuid binary, or under another account on a
    /// shared host, left the scan with no row, no note and nothing for a verdict to rest
    /// on. It is now a [`Withheld::AnotherAccount`] row: its identifier is known,
    /// its lineage is readable in `stat`, and everything else is stated as refused.
    ///
    /// What is still dropped is what is not a process a person could be working in: an
    /// entry that has gone between the listing and the read, and a kernel thread, which
    /// has no command line, no environment and no working directory by construction and
    /// can hold nothing in anybody's home. On this machine that is the difference
    /// between 36 withheld rows and 429, and a count nobody can act on is not evidence.
    fn read(pid: u32, directory: &Path) -> Option<Running> {
        let vars =
            std::fs::read(directory.join(ENVIRON)).map(|blob| kept(&blob)).unwrap_or_default();
        let cwd = std::fs::read_link(directory.join(CWD)).ok();
        let line = std::fs::read(directory.join(CMDLINE)).ok();
        let command = line.as_deref().and_then(command_of);
        if vars.is_empty() && cwd.is_none() {
            let real = line.is_some_and(|line| !line.is_empty());
            if !real {
                return None;
            }
            return Some(Running {
                pid,
                vars,
                cwd,
                command,
                held: Vec::new(),
                lineage: lineage(directory),
                withheld: Some(Withheld::AnotherAccount),
            });
        }
        let held = holds(directory);
        Some(Running { pid, vars, cwd, command, held, lineage: lineage(directory), withheld: None })
    }

    /// The directory holding one link per open descriptor.
    const FD: &str = "fd";

    /// The directory holding one file per open descriptor, each carrying its open flags.
    const FDINFO: &str = "fdinfo";

    /// The file listing one memory mapping per line.
    const MAPS: &str = "maps";

    /// The link pointing at the root directory the process sees.
    const ROOT: &str = "root";

    /// The line of an `fdinfo` record that carries the flags the descriptor was opened
    /// with, in octal.
    const FLAGS: &str = "flags:";

    /// The low two bits of those flags, which are the access mode.
    const ACCMODE: u32 = 0o3;

    /// The access mode of a descriptor opened for reading only.
    const RDONLY: u32 = 0o0;

    /// Everything one process holds that would outlive a rename of the directory above
    /// it: descriptors opened for writing, writable mappings, and the root it sees.
    ///
    /// Read in the walk the scan already makes, so **every reader of the table pays it**
    /// — the reclaim, its preflight, and `nodal ls`, `nodal ps`, `nodal show` and
    /// `nodal run` alike. That is deliberate and not an oversight: occupancy is one
    /// predicate, and a listing that read less than the preflight would print `clear`
    /// over a home the preflight refuses on. Measured on one machine, the whole walk is
    /// 12.6–18 ms over 132 processes against 2.5–3.6 ms without it. Nothing here opens a
    /// file of another process: every read is of `/proc`, which is the kernel answering
    /// about itself.
    ///
    /// Every failure is silence. A descriptor that closed between the listing and the
    /// read, a mapping file that grew under the read, a process that ended: none of them
    /// is a hold and none of them is an error. What that cannot do is hide a hold that
    /// was there, because a hold the reading missed leaves the process judged on its
    /// working directory alone, which is exactly the rule that stood before.
    fn holds(directory: &Path) -> Vec<Held> {
        let mut held = Vec::new();
        held.extend(descriptors(directory));
        held.extend(mappings(directory));
        held.extend(rooted(directory));
        held.sort_by(|left, right| (&left.path, left.how).cmp(&(&right.path, right.how)));
        held.dedup();
        held
    }

    /// The descriptors this process opened for writing, by the path each names.
    ///
    /// The link is read before the flags are, and that order is the whole of the cost
    /// argument. Most descriptors on a machine are sockets, pipes and anonymous inodes,
    /// whose links are not paths at all; they are dropped on the link alone and never
    /// cost the second read. Only a descriptor naming a real absolute path is asked what
    /// it was opened for.
    ///
    /// **A descriptor on a file that has been unlinked holds nothing a rename could take
    /// from anybody.** The kernel marks that link `(deleted)`, and the ordinary shape is
    /// a dev server writing to a log its own rotation has already replaced: refusing
    /// there would refuse a reclaim over a path that no longer exists. [`mapped_path`]
    /// drops them for the same reason, and both readings have to, or the two halves of
    /// occupancy mean different things.
    fn descriptors(directory: &Path) -> Vec<Held> {
        let Ok(entries) = std::fs::read_dir(directory.join(FD)) else { return Vec::new() };
        let mut held = Vec::new();
        for entry in entries.flatten() {
            let Ok(path) = std::fs::read_link(entry.path()) else { continue };
            if !path.is_absolute() || unlinked(&path) {
                continue;
            }
            let number = entry.file_name();
            let Ok(record) = std::fs::read_to_string(directory.join(FDINFO).join(number)) else {
                continue;
            };
            if writable(&record) {
                held.push(Held::new(path, How::Descriptor));
            }
        }
        held
    }

    /// Whether an `fdinfo` record says its descriptor was opened for writing.
    ///
    /// A record with no flags line, or a line that does not parse, is not read as
    /// writable. That is the direction that under-counts occupancy rather than refusing
    /// over a descriptor nobody can write through, and the process is still judged on
    /// everything else this walk read about it.
    fn writable(record: &str) -> bool {
        record
            .lines()
            .find_map(|line| line.strip_prefix(FLAGS))
            .and_then(|octal| u32::from_str_radix(octal.trim(), 8).ok())
            .is_some_and(|flags| flags & ACCMODE != RDONLY)
    }

    /// The files this process has mapped in a way that writes through to them.
    ///
    /// **Writable and shared, not writable alone.** A `rw-p` mapping is copy-on-write:
    /// the process may write all it likes and the file never changes, which is what every
    /// loaded library's data segment is, on every process on the machine. Counting those
    /// would make every home look occupied by everything. A `rw-s` mapping is the one
    /// that writes through, and it is how a memory-mapped database holds its file — the
    /// hold this lane exists to see.
    ///
    /// A mapping with no file behind it — the stack, the heap, an anonymous mapping —
    /// has no path and is a hold on nothing. A mapping the kernel marks `(deleted)` is
    /// not kept either: its file is already unlinked, so a rename of the directory above
    /// it takes nothing from anybody.
    fn mappings(directory: &Path) -> Vec<Held> {
        let Ok(listing) = std::fs::read_to_string(directory.join(MAPS)) else { return Vec::new() };
        let mut held = Vec::new();
        for line in listing.lines() {
            let Some(perms) = line.split_whitespace().nth(1) else { continue };
            if !(perms.contains('w') && perms.contains('s')) {
                continue;
            }
            let Some(path) = mapped_path(line) else { continue };
            held.push(Held::new(path, How::Mapping));
        }
        held
    }

    /// What the kernel appends to the name of a file that has been unlinked.
    const DELETED: &str = " (deleted)";

    /// Whether this name is the kernel's name for a file that is no longer there.
    ///
    /// A real file may be called `x (deleted)`; the difference cannot be told from the
    /// link alone, so the rare honest file is read as gone and refuses nothing. That is
    /// the direction that under-counts occupancy, and the process is still judged on
    /// everything else the walk read about it.
    fn unlinked(path: &Path) -> bool {
        path.to_str().is_some_and(|name| name.ends_with(DELETED))
    }

    /// How many fields of a `maps` line come before the path.
    ///
    /// The line is `address perms offset dev inode path`, so five.
    const BEFORE_THE_PATH: usize = 5;

    /// The file a `maps` line names, or nothing where it names none.
    ///
    /// The path is taken as the whole remainder of the line rather than as a field,
    /// because a file name may hold spaces and splitting on whitespace would cut one in
    /// half and then refuse over a path that does not exist.
    fn mapped_path(line: &str) -> Option<&str> {
        let mut rest = line.trim_start();
        for _ in 0..BEFORE_THE_PATH {
            let field = rest.find(char::is_whitespace)?;
            rest = rest[field..].trim_start();
        }
        (rest.starts_with('/') && !rest.ends_with(DELETED)).then_some(rest)
    }

    /// The root directory this process sees, when it is not the machine's own.
    ///
    /// A process whose root is `/` is rooted at nothing in particular, and recording
    /// that of every process on the machine would make every home look occupied. A
    /// process rooted anywhere else — a container, a `chroot` — is held to that
    /// directory and cannot be moved out of it.
    fn rooted(directory: &Path) -> Vec<Held> {
        std::fs::read_link(directory.join(ROOT))
            .ok()
            .filter(|root| root != Path::new("/"))
            .map(|root| vec![Held::new(root, How::Root)])
            .unwrap_or_default()
    }

    /// The file that holds one process's own statistics, including when it started.
    const STAT: &str = "stat";

    /// How many fields into `stat`'s tail the parent's identifier is
    /// ([`stat_field`]): `state ppid ...`, so the second.
    const PPID: usize = 1;

    /// One field of a `stat` record's tail, counted from `state`.
    ///
    /// `stat` is `pid (comm) state ...` and the command can hold spaces and brackets, so
    /// every reading of it takes the tail from the last `)` rather than splitting the
    /// whole record. The three callers differ only in which field they want, and each
    /// names its own position.
    fn stat_field<T: std::str::FromStr>(record: &str, nth: usize) -> Option<T> {
        record.rsplit_once(')')?.1.split_whitespace().nth(nth)?.parse().ok()
    }

    /// The process that started this one, `None` when the host will not say.
    pub(super) fn parent_of(pid: u32) -> Option<u32> {
        let record =
            std::fs::read_to_string(Path::new(PROC).join(pid.to_string()).join(STAT)).ok()?;
        stat_field(&record, PPID)
    }

    /// How many fields into `stat`'s tail the process group is ([`stat_field`]):
    /// `state ppid pgrp ...`, so the third.
    const PGRP: usize = 2;

    /// Where one process came from, from the record that stays readable when the rest of
    /// its directory does not.
    ///
    /// `stat` is world-readable for a process this account may not otherwise read, which
    /// is what makes a withheld row worth keeping at all: its identifier, its parent, its
    /// group and its session are facts, and everything else about it is a refusal.
    fn lineage(directory: &Path) -> Lineage {
        let Ok(record) = std::fs::read_to_string(directory.join(STAT)) else {
            return Lineage::default();
        };
        Lineage { parent: stat_field(&record, PPID), group: stat_field(&record, PGRP) }
    }

    /// How many fields into `stat`'s tail the session identifier is
    /// ([`stat_field`]): `state ppid pgrp session ...`, so the fourth.
    const SESSION: usize = 3;

    /// Whether any process in `table` is still in the session `sid` names.
    ///
    /// Every entry is read rather than stopping at the leader, because a session
    /// outlives its leader: a shell that exits while a command it started is still
    /// running leaves the session holding that command. Nothing is opened for writing
    /// and nothing is signalled.
    ///
    /// `None` where the table could not be listed, and `None` where an entry could not
    /// be read and nothing else matched. A `false` here lets a hold go, so it is said
    /// only where the whole table was read and the session was not in it.
    pub(super) fn session_is_live(table: &Path, sid: u32) -> Option<bool> {
        let entries = std::fs::read_dir(table).ok()?;
        let mut hidden = false;
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            match session_of(table, pid) {
                Said::Session(found) if found == sid => return Some(true),
                Said::Session(_) | Said::Gone => {}
                Said::Hidden => hidden = true,
            }
        }
        (!hidden).then_some(false)
    }

    /// What one entry of the table said about the session its process is in.
    ///
    /// Three answers and not two. A process that ended between the listing and the read
    /// is `Gone`, which is ordinary on a busy machine and says nothing about any other
    /// session; folding it in with `Hidden` would make "I cannot see" the answer almost
    /// every time and leave a lapsed hold standing for the whole idle window.
    enum Said {
        /// The process is in this session.
        Session(u32),
        /// There is no such process any more, which is an answer.
        Gone,
        /// This account may not read it, or its record did not parse, so nothing is
        /// said about it. `hidepid` and another account's process both land here.
        Hidden,
    }

    /// What one process's record says about the session it is in.
    fn session_of(table: &Path, pid: u32) -> Said {
        match std::fs::read_to_string(table.join(pid.to_string()).join(STAT)) {
            Ok(record) => stat_field(&record, SESSION).map_or(Said::Hidden, Said::Session),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Said::Gone,
            Err(_) => Said::Hidden,
        }
    }

    /// How many fields into `stat`'s tail the start time is ([`stat_field`]): field 22
    /// of the whole record, which is the twentieth of the tail.
    const STARTTIME: usize = 19;

    /// The line of the kernel's own statistics that dates the boot.
    const BTIME: &str = "btime ";

    /// What the table says about each of these identifiers, dated where it can be.
    ///
    /// The boot instant and the tick rate are read once for the whole set, because both
    /// are properties of the machine and neither changes between two identifiers.
    pub(super) fn presences(pids: &[u32]) -> BTreeMap<u32, Presence> {
        let clock = Clock::of_this_host();
        pids.iter()
            .map(|pid| {
                let directory = Path::new(PROC).join(pid.to_string());
                if !directory.is_dir() {
                    return (*pid, Presence::Gone);
                }
                (*pid, Presence::Running { started_at: clock.started(&directory) })
            })
            .collect()
    }

    /// What turns a process's start, which the kernel counts in ticks since the boot,
    /// into an instant.
    struct Clock {
        /// When this machine booted, in seconds since the epoch.
        booted_at: Option<i64>,
        /// How many of the kernel's ticks make one second.
        ticks: i64,
    }

    impl Clock {
        /// This machine's own.
        fn of_this_host() -> Self {
            Self { booted_at: booted_at(), ticks: ticks_per_second() }
        }

        /// When the process in `directory` started, `None` when this cannot be read or
        /// this host does not date its boot.
        fn started(&self, directory: &Path) -> Option<Timestamp> {
            let booted_at = self.booted_at?;
            let record = std::fs::read_to_string(directory.join(STAT)).ok()?;
            let ticks: i64 = stat_field(&record, STARTTIME)?;
            Timestamp::from_unix_seconds(booted_at.checked_add(ticks / self.ticks)?).ok()
        }
    }

    /// When this machine booted, from the kernel's own record of it.
    fn booted_at() -> Option<i64> {
        let record = std::fs::read_to_string(Path::new(PROC).join("stat")).ok()?;
        record.lines().find_map(|line| line.strip_prefix(BTIME))?.trim().parse().ok()
    }

    /// How many kernel ticks make a second on this host.
    ///
    /// One is the floor, because a rate of zero would divide by zero and a host that
    /// answers nonsense must not take a caller down with it.
    fn ticks_per_second() -> i64 {
        // SAFETY: `sysconf` reads one configured value and takes no pointer. A name the
        // host does not know answers -1, which the floor below turns into 1.
        let answered = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        answered.max(1)
    }
}

/// Reading the process table of a macOS host, which the kernel answers per process.
///
/// Three calls, and each gives one of the three things a scan keeps: `proc_listallpids`
/// lists the identifiers, `proc_pidinfo` with `PROC_PIDVNODEPATHINFO` gives the working
/// directory, and `sysctl` with `KERN_PROCARGS2` gives the command and the variables.
/// `csops` says whether the kernel zeroed the variables of a restricted binary, so that
/// an empty block is not read as a process that carries nothing.
#[cfg(target_os = "macos")]
mod macos {
    use std::collections::BTreeMap;
    use std::ffi::{CStr, OsStr, c_int};
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::PathBuf;

    use super::{Lineage, Presence, Running, Timestamp, Withheld, command_of, kept};
    use crate::{Error, Result};

    unsafe extern "C" {
        /// The code-signing status of a process, from `<sys/codesign.h>`. The `libc`
        /// crate does not declare it.
        fn csops(pid: libc::pid_t, ops: u32, useraddr: *mut libc::c_void, size: usize) -> c_int;
    }

    /// The `csops` operation that reads the status flags of a process.
    const CS_OPS_STATUS: u32 = 0;

    /// The status flag of a restricted binary, whose variables `KERN_PROCARGS2` zeroes.
    const CS_RESTRICT: u32 = 0x800;

    /// How many more identifiers the listing has room for than the count said. A process
    /// that starts between the count and the listing then still fits.
    const SLACK: usize = 64;

    /// Every process this host lists, with what this account may read about it.
    pub(super) fn scan() -> Result<Vec<Running>> {
        let mut buffer = vec![0_u8; argument_limit()?];
        Ok(pids()?.into_iter().filter_map(|pid| read(pid, &mut buffer)).collect())
    }

    /// One process, or nothing when it ended during the scan or shows nothing to read.
    fn read(pid: libc::pid_t, buffer: &mut [u8]) -> Option<Running> {
        let pid_number = u32::try_from(pid).ok()?;
        let cwd = match directory(pid) {
            Ok(cwd) => cwd,
            Err(Unanswered::AnotherAccount) => {
                let hidden = Running::new(pid_number, BTreeMap::new()).from(lineage(pid));
                return Some(hidden.withholding(Withheld::AnotherAccount));
            }
            Err(Unanswered::Gone) => return None,
        };
        let (command, environ) = arguments(pid, buffer).unwrap_or((None, &[]));
        let restricted = is_restricted(pid);
        let vars = if restricted { BTreeMap::new() } else { kept(environ) };
        if vars.is_empty() && cwd.is_none() && !restricted {
            return None;
        }
        Some(Running {
            pid: pid_number,
            vars,
            cwd,
            command,
            // macOS publishes no per-descriptor open flags for a vnode, so the read/write
            // split that keeps an editor from refusing a reclaim cannot be made here.
            // Occupancy on this host is the working directory, and [`OCCUPANCY`] says so
            // rather than leaving a reader to infer it from an empty list.
            held: Vec::new(),
            lineage: lineage(pid),
            withheld: restricted.then_some(Withheld::Restricted),
        })
    }

    /// Why the kernel gave no working directory for a process.
    enum Unanswered {
        /// The process belongs to another account.
        AnotherAccount,
        /// The process ended, or it has no working directory to give.
        Gone,
    }

    /// Every identifier the kernel lists, without the kernel's own zero.
    fn pids() -> Result<Vec<libc::pid_t>> {
        // SAFETY: a null buffer of size zero asks for the count alone, and nothing is
        // written.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let count = usize::try_from(count).map_err(|_| failed("proc_listallpids"))?;
        let mut pids: Vec<libc::pid_t> = vec![0; count + SLACK];
        let bytes = c_int::try_from(std::mem::size_of_val(pids.as_slice()))
            .map_err(|_| failed("proc_listallpids"))?;
        // SAFETY: `pids` is `bytes` bytes of `pid_t`, and the kernel writes at most that
        // many. The answer is how many identifiers it wrote.
        let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        let listed = usize::try_from(listed).map_err(|_| failed("proc_listallpids"))?;
        pids.truncate(listed);
        pids.retain(|pid| *pid > 0);
        Ok(pids)
    }

    /// The working directory of a process, `Ok(None)` where it has none.
    fn directory(pid: libc::pid_t) -> std::result::Result<Option<PathBuf>, Unanswered> {
        // SAFETY: `proc_vnodepathinfo` is plain integers and byte arrays, so all zeroes is
        // a valid value of it.
        let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_vnodepathinfo>();
        // SAFETY: `info` is `size` bytes, and the kernel writes at most that many. The
        // answer is the number of bytes written, or zero with `errno` set.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&raw mut info).cast(),
                c_int::try_from(size).unwrap_or(0),
            )
        };
        if usize::try_from(written).ok() != Some(size) {
            let refused = std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
            return Err(if refused { Unanswered::AnotherAccount } else { Unanswered::Gone });
        }
        // SAFETY: the path is `[[c_char; 32]; 32]`, which is 1024 bytes in one block. The
        // kernel ends it with a zero byte, and `from_bytes_until_nul` stops at the first
        // one or refuses a block that has none.
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(info.pvi_cdir.vip_path.as_ptr().cast(), 32 * 32) };
        let path = CStr::from_bytes_until_nul(bytes).map(CStr::to_bytes).unwrap_or_default();
        Ok((!path.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(path))))
    }

    /// The size of the largest argument block the kernel gives, which `KERN_PROCARGS2`
    /// needs as its buffer.
    fn argument_limit() -> Result<usize> {
        let mut limit: c_int = 0;
        let mut size = size_of::<c_int>();
        let mut name = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `limit` is `size` bytes, the kernel writes one `c_int` into it, and no
        // new value is set.
        let answered = unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                2,
                (&raw mut limit).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if answered != 0 {
            return Err(failed("kern.argmax"));
        }
        usize::try_from(limit).map_err(|_| failed("kern.argmax"))
    }

    /// The short command and the variable block of a process, `None` where the kernel
    /// refuses the reading.
    fn arguments(pid: libc::pid_t, buffer: &mut [u8]) -> Option<(Option<String>, &[u8])> {
        let mut size = buffer.len();
        let mut name = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        // SAFETY: `buffer` is `size` bytes, the kernel writes at most that many and sets
        // `size` to the count, and no new value is set.
        let answered = unsafe {
            libc::sysctl(
                name.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if answered != 0 {
            return None;
        }
        let (argv, environ) = super::split_procargs(buffer.get(..size)?)?;
        Some((command_of(argv), environ))
    }

    /// Whether the kernel marks this process as a restricted binary. A process whose
    /// status cannot be read is not called restricted, and its variables are read.
    fn is_restricted(pid: libc::pid_t) -> bool {
        let mut flags: u32 = 0;
        // SAFETY: `flags` is four bytes, `CS_OPS_STATUS` writes one `u32`, and the size
        // passed is the size of `flags`.
        let answered =
            unsafe { csops(pid, CS_OPS_STATUS, (&raw mut flags).cast(), size_of::<u32>()) };
        answered == 0 && flags & CS_RESTRICT != 0
    }

    /// What the kernel says about each of these identifiers, dated where it will.
    pub(super) fn presences(pids: &[u32]) -> Result<BTreeMap<u32, Presence>> {
        pids.iter().map(|pid| Ok((*pid, presence(*pid)?))).collect()
    }

    /// One identifier. A process of another account is found and not dated.
    fn presence(pid: u32) -> Result<Presence> {
        let Ok(number) = libc::pid_t::try_from(pid) else { return Ok(Presence::Gone) };
        match bsd_info(number) {
            Ok(info) => Ok(Presence::Running {
                started_at: i64::try_from(info.pbi_start_tvsec)
                    .ok()
                    .and_then(|seconds| Timestamp::from_unix_seconds(seconds).ok()),
            }),
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(Presence::Gone),
            Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
                Ok(Presence::Running { started_at: None })
            }
            Err(source) => Err(Error::Io { path: PathBuf::from("proc_pidinfo"), source }),
        }
    }

    /// The kernel's record of one process of this account.
    fn bsd_info(pid: libc::pid_t) -> std::io::Result<libc::proc_bsdinfo> {
        // SAFETY: `proc_bsdinfo` is plain integers and byte arrays, so all zeroes is a
        // valid value of it.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_bsdinfo>();
        // SAFETY: `info` is `size` bytes, and the kernel writes at most that many. The
        // answer is the number of bytes written, or zero with `errno` set.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&raw mut info).cast(),
                c_int::try_from(size).unwrap_or(0),
            )
        };
        if usize::try_from(written).ok() == Some(size) {
            return Ok(info);
        }
        let error = std::io::Error::last_os_error();
        // A short answer with no error set is a record that is not there.
        Err(if error.raw_os_error() == Some(0) {
            std::io::Error::from_raw_os_error(libc::ESRCH)
        } else {
            error
        })
    }

    /// The process that started this one, `None` when the kernel will not say.
    ///
    /// The short record answers for every account, which the full record does not.
    pub(super) fn parent_of(pid: u32) -> Option<u32> {
        short_info(libc::pid_t::try_from(pid).ok()?).map(|info| info.pbsi_ppid)
    }

    /// The short record itself, which is what both readings of lineage are taken from.
    fn short_info(pid: libc::pid_t) -> Option<libc::proc_bsdshortinfo> {
        // SAFETY: `proc_bsdshortinfo` is plain integers and byte arrays, so all zeroes is
        // a valid value of it.
        let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
        let size = size_of::<libc::proc_bsdshortinfo>();
        // SAFETY: `info` is `size` bytes, and the kernel writes at most that many.
        let written = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDT_SHORTBSDINFO,
                0,
                (&raw mut info).cast(),
                c_int::try_from(size).ok()?,
            )
        };
        (usize::try_from(written).ok() == Some(size)).then_some(info)
    }

    /// Where one process came from, for a process of any account.
    ///
    /// The short record answers across accounts, which is what makes this readable for
    /// exactly the process the rest of this module is refused: the one
    /// [`Withheld::AnotherAccount`] is about. One call and no `getsid`: the session is
    /// not part of [`Lineage`], for the reason that type states.
    fn lineage(pid: libc::pid_t) -> Lineage {
        let short = short_info(pid);
        Lineage {
            parent: short.map(|info| info.pbsi_ppid),
            group: short.map(|info| info.pbsi_pgid),
        }
    }

    /// Whether any process on this host is in the session `sid` names.
    ///
    /// `getsid` answers for every account on macOS. A process that ended after the
    /// listing answers `ESRCH`, which says nothing about this session. Any other refusal
    /// hides a process, and then `false` is not said, as on Linux. `None` also where the
    /// listing itself failed.
    pub(super) fn session_is_live(sid: u32) -> Option<bool> {
        let sid = libc::pid_t::try_from(sid).ok()?;
        let mut hidden = false;
        for pid in pids().ok()? {
            // SAFETY: `getsid` takes one integer and no pointer. A failure answers -1
            // with `errno` set.
            let found = unsafe { libc::getsid(pid) };
            if found == sid {
                return Some(true);
            }
            let ended = std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            hidden |= found < 0 && !ended;
        }
        (!hidden).then_some(false)
    }

    /// An error for a kernel call that failed, with the reason the kernel set.
    fn failed(call: &str) -> Error {
        Error::Io { path: PathBuf::from(call), source: std::io::Error::last_os_error() }
    }
}

/// The prefix of every variable a home's identity is written with.
const PREFIX: &str = "NODAL_";

/// Whether a variable is one a scan keeps.
fn is_kept(name: &str) -> bool {
    name.starts_with(PREFIX) || crate::runtime::actor::signal_names().contains(&name)
}

/// The kept variables of an environment block, which is `name=value` records separated
/// by a zero byte. Both hosts give the block in this shape.
fn kept(environ: &[u8]) -> BTreeMap<String, String> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|record| std::str::from_utf8(record).ok())
        .filter_map(|record| record.split_once('='))
        .filter(|(name, _)| is_kept(name))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

/// The arguments and the variable block of one macOS `KERN_PROCARGS2` buffer, `None`
/// where the buffer does not have that shape.
///
/// The buffer is the argument count as a native `i32`, the program's path, zero bytes of
/// padding, the arguments, and the variables. Each record ends with a zero byte. The
/// variables end at the first empty record, and what follows is the kernel's own. Kept
/// apart from the call so that the parse is tested on every host.
#[cfg(any(target_os = "macos", test))]
fn split_procargs(block: &[u8]) -> Option<(&[u8], &[u8])> {
    let (count, rest) = block.split_first_chunk::<4>()?;
    let count = usize::try_from(i32::from_ne_bytes(*count)).ok()?;
    let path = rest.iter().position(|byte| *byte == 0)?;
    let rest = rest.get(path..)?;
    let rest = rest.get(rest.iter().position(|byte| *byte != 0)?..)?;
    let mut arguments = 0;
    for _ in 0..count {
        arguments += rest.get(arguments..)?.iter().position(|byte| *byte == 0)? + 1;
    }
    let (argv, tail) = rest.split_at_checked(arguments)?;
    let mut variables = 0;
    while let Some(length) =
        tail.get(variables..).and_then(|left| left.iter().position(|b| *b == 0))
    {
        if length == 0 {
            break;
        }
        variables += length + 1;
    }
    Some((argv, tail.get(..variables)?))
}

/// The short command of one NUL-separated command line, or nothing when it holds none.
///
/// The file name of the program, and the word after it when that word is a plain one.
/// Kept apart from the reading so that it is tested on every host, and so that the rule
/// that keeps a credential out of the output is one function with its own tests.
#[must_use]
pub fn command_of(cmdline: &[u8]) -> Option<String> {
    let mut words = cmdline
        .split(|byte| *byte == 0)
        .filter(|word| !word.is_empty())
        .filter_map(|word| std::str::from_utf8(word).ok());
    let program = program_name(words.next()?);
    if program.is_empty() {
        return None;
    }
    let program = &program[..cut(program, PROGRAM)];
    match words.next().filter(|word| is_plain(word)) {
        Some(argument) => Some(format!("{program} {argument}")),
        None => Some(program.to_owned()),
    }
}

/// The file name of a program, so that `/usr/bin/node` reads as `node`.
fn program_name(argv0: &str) -> &str {
    argv0.rsplit('/').next().unwrap_or(argv0)
}

/// Whether a word is one a command may show: a subcommand, which is letters, digits,
/// dash, dot or underscore, does not lead with a dash, and is no longer than a column.
/// Anything else may carry a value, and a value is never written anywhere.
fn is_plain(word: &str) -> bool {
    !word.is_empty()
        && !word.starts_with('-')
        && word.len() <= WORD
        && word.chars().all(|letter| letter.is_ascii_alphanumeric() || "-._".contains(letter))
}

/// The longest second word a command shows.
const WORD: usize = 24;

/// The longest program name a command shows. A command line is written by whatever
/// started the process, so its length is not Nodal's to trust: a column has a width
/// whatever the machine is doing.
const PROGRAM: usize = 32;

/// Where to cut `text` so that it is no longer than `limit` and is still text.
fn cut(text: &str, limit: usize) -> usize {
    if text.len() <= limit {
        return text.len();
    }
    (0..=limit).rev().find(|end| text.is_char_boundary(*end)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{command_of, kept, split_procargs};

    #[test]
    fn a_scan_keeps_the_nodal_set_and_drops_the_rest() {
        let environ = b"NODAL_ID=01ARZ3NDEKTSV4RRFFQ69G5FAV\0USER=josh\0AWS_SECRET_ACCESS_KEY=x\0";
        let vars = kept(environ);
        assert_eq!(vars.get("NODAL_ID").map(String::as_str), Some("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(vars.get("USER").map(String::as_str), Some("josh"));
        assert!(
            !vars.contains_key("AWS_SECRET_ACCESS_KEY"),
            "a scan kept a value it was not asked for"
        );
    }

    /// A `KERN_PROCARGS2` buffer as the kernel lays it out.
    fn procargs(count: i32, path: &str, records: &[&str]) -> Vec<u8> {
        let mut block = count.to_ne_bytes().to_vec();
        block.extend(path.as_bytes());
        block.extend([0, 0, 0, 0]);
        for record in records {
            block.extend(record.as_bytes());
            block.push(0);
        }
        block
    }

    #[test]
    fn a_procargs_buffer_splits_into_the_command_and_the_variables() {
        let block = procargs(
            2,
            "/usr/local/bin/node",
            &["node", "dev", "NODAL_ID=01ARZ3NDEKTSV4RRFFQ69G5FAV", "", "executable_path=x"],
        );
        let (argv, environ) = split_procargs(&block).unwrap();
        assert_eq!(command_of(argv).as_deref(), Some("node dev"));
        assert_eq!(environ, b"NODAL_ID=01ARZ3NDEKTSV4RRFFQ69G5FAV\0");
        assert!(
            !kept(environ).contains_key("executable_path"),
            "the kernel's own strings are not variables"
        );
    }

    #[test]
    fn a_procargs_buffer_with_zeroed_variables_has_none() {
        let block = procargs(1, "/bin/zsh", &["-zsh", "", ""]);
        let (argv, environ) = split_procargs(&block).unwrap();
        assert_eq!(command_of(argv).as_deref(), Some("-zsh"));
        assert!(environ.is_empty());
    }

    #[test]
    fn a_procargs_buffer_that_is_cut_short_gives_nothing() {
        assert_eq!(split_procargs(b"\x02\0"), None);
        assert_eq!(split_procargs(&procargs(3, "/bin/sleep", &["sleep", "1"])), None);
        assert_eq!(split_procargs(&(-1_i32).to_ne_bytes()), None);
    }

    #[test]
    fn a_record_that_is_not_text_is_skipped_rather_than_failing() {
        assert!(kept(b"\xff\xfe=x\0").is_empty());
    }

    #[test]
    fn a_command_is_the_program_and_its_subcommand() {
        assert_eq!(command_of(b"/usr/bin/node\0dev\0").as_deref(), Some("node dev"));
        assert_eq!(command_of(b"pnpm\0test\0").as_deref(), Some("pnpm test"));
        assert_eq!(command_of(b"sleep\0").as_deref(), Some("sleep"));
    }

    #[test]
    fn a_word_that_could_carry_a_value_is_left_out() {
        assert_eq!(
            command_of(b"psql\0postgres://user:s3cret@localhost/db\0").as_deref(),
            Some("psql"),
        );
        assert_eq!(command_of(b"env\0TOKEN=s3cret\0").as_deref(), Some("env"));
        assert_eq!(command_of(b"node\0--inspect\0").as_deref(), Some("node"));
        assert_eq!(command_of(b"cat\0/home/j/.nodal/secrets.env\0").as_deref(), Some("cat"));
    }

    #[test]
    fn a_program_name_no_machine_should_have_is_cut_to_a_column() {
        let mut line = b"/usr/bin/".to_vec();
        line.extend(std::iter::repeat_n(b'a', 4_000));
        line.push(0);
        let command = command_of(&line).unwrap();
        assert_eq!(command.len(), 32);
    }

    #[test]
    fn a_program_name_that_is_cut_stays_text() {
        let mut line = "/usr/bin/".as_bytes().to_vec();
        line.extend("é".repeat(40).as_bytes());
        line.push(0);
        assert!(command_of(&line).is_some());
    }

    #[test]
    fn a_command_line_with_nothing_in_it_is_no_command() {
        assert_eq!(command_of(b""), None);
        assert_eq!(command_of(b"\0\0"), None);
        assert_eq!(command_of(b"/\0"), None);
    }
}
