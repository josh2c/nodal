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
//! A scan keeps three things about a process, and each answers one question:
//!
//! - the variables it was asked for — the `NODAL_*` set and the names that say who the
//!   actor is — which name the unit a process is in outright;
//! - its working directory, which says which home a process stands in when it carries
//!   no variables at all, for example a terminal that was never activated;
//! - a short form of its command, so a person reading `nodal ps` recognises the row.
//!
//! Everything else a process carries is read and dropped. A process this account cannot
//! read contributes nothing to the three on either host. On Linux it is left out of the
//! scan. On macOS it is kept with what was withheld, so that a reader can say how much
//! it could not see.
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
use crate::model::Timestamp;

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
    /// What the host refused to show about it, `None` where nothing was refused.
    pub withheld: Option<Withheld>,
}

/// What the host refused to show about one process, and so what a reader cannot know.
///
/// Only macOS sets this. A Linux scan leaves out a process whose `/proc` entry this
/// account may not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Withheld {
    /// The process belongs to another account. Its variables, its working directory and
    /// its command are refused.
    AnotherAccount,
    /// The process runs a restricted binary. Its working directory and its command are
    /// shown, and its variables are not.
    Restricted,
}

impl Running {
    /// A process known by its variables alone, which is what a test supplies.
    #[must_use]
    pub fn new(pid: u32, vars: BTreeMap<String, String>) -> Self {
        Self { pid, vars, cwd: None, command: None, withheld: None }
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

    use super::{Presence, Running, Timestamp, command_of, kept};
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

    /// One process, or nothing when this account can see neither its variables nor the
    /// directory it stands in.
    ///
    /// A process this account may not read, and a process that ended between the listing
    /// and the read, are both "not visible" and neither is a failure.
    fn read(pid: u32, directory: &Path) -> Option<Running> {
        let vars =
            std::fs::read(directory.join(ENVIRON)).map(|blob| kept(&blob)).unwrap_or_default();
        let cwd = std::fs::read_link(directory.join(CWD)).ok();
        if vars.is_empty() && cwd.is_none() {
            return None;
        }
        let command =
            std::fs::read(directory.join(CMDLINE)).ok().and_then(|line| command_of(&line));
        Some(Running { pid, vars, cwd, command, withheld: None })
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

    use super::{Presence, Running, Timestamp, Withheld, command_of, kept};
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
                let hidden = Running::new(pid_number, BTreeMap::new());
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
        let pid = libc::pid_t::try_from(pid).ok()?;
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
        (usize::try_from(written).ok() == Some(size)).then_some(info.pbsi_ppid)
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
