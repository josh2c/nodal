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
//! [`Live`] is the one implementation: on Linux it reads `/proc/<pid>`.
//!
//! **macOS is not implemented, deliberately.** The reading there is
//! `sysctl(KERN_PROCARGS2)`, which is not `/proc` under another name: it returns one
//! packed buffer per process whose layout is undocumented and has changed between
//! releases, it is refused for another user's processes, and on a hardened or
//! System-Integrity-Protected binary it is refused for the caller's own. Nothing in that
//! list can be established from a Linux workstation, and a process scan is what sessions
//! and attribution are both built on: shipping a version that has never run on the
//! hardware would put a guess under two features. So a scan on macOS returns
//! [`Error::ProcessScanUnsupported`](crate::Error::ProcessScanUnsupported), the typed
//! error, and `nodal ps` prints it as a note. The machine reports "I cannot see", which
//! is a different answer from "nobody is attached", and the other signals still answer.
//! The implementation lands with the Mac.
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
//! read contributes nothing rather than failing the scan.
//!
//! The command is deliberately short, and not the command line. A command line can hold
//! a credential — `psql postgres://user:password@host/db` — and Nodal writes no secret
//! value anywhere. So a command is the file name of the program, and the word after it
//! only when that word is a plain word: letters, digits, dash, dot or underscore. A
//! subcommand (`next dev`, `pnpm test`) survives that rule; a URL, an assignment or an
//! option does not. Both parts are cut to a column's width, because a command line is
//! written by whatever started the process and its length is not Nodal's to trust.
//!
//! The reading itself is in the [`linux`] module, because `/proc` is the only source
//! there is; that module is what grows a second host, rather than this file growing a
//! second set of conditions.

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
}

impl Running {
    /// A process known by its variables alone, which is what a test supplies.
    #[must_use]
    pub fn new(pid: u32, vars: BTreeMap<String, String>) -> Self {
        Self { pid, vars, cwd: None, command: None }
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
    fn presences(&self, pids: &[u32]) -> Result<BTreeMap<u32, Presence>> {
        #[cfg(target_os = "linux")]
        {
            Ok(linux::presences(pids))
        }
        #[cfg(not(target_os = "linux"))]
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

#[cfg(not(target_os = "linux"))]
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
    #[cfg(not(target_os = "linux"))]
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
    #[cfg(not(target_os = "linux"))]
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
///
/// Everything a `/proc` scan needs is here, including which variables it keeps: a host
/// with another source keeps the same three things, so the day macOS arrives this
/// becomes two modules over one list rather than one module under two conditions.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{Presence, Running, Timestamp, command_of};
    use crate::runtime::actor;
    use crate::{Error, Result};

    /// The prefix of every variable a home's identity is written with.
    const PREFIX: &str = "NODAL_";

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
        Some(Running { pid, vars, cwd, command })
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

    /// Whether a variable is one a scan keeps.
    fn is_kept(name: &str) -> bool {
        name.starts_with(PREFIX) || actor::signal_names().contains(&name)
    }

    /// The kept variables of one `environ` blob, which is name=value records separated
    /// by a zero byte.
    fn kept(environ: &[u8]) -> BTreeMap<String, String> {
        environ
            .split(|byte| *byte == 0)
            .filter_map(|record| std::str::from_utf8(record).ok())
            .filter_map(|record| record.split_once('='))
            .filter(|(name, _)| is_kept(name))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::kept;

        #[test]
        fn a_scan_keeps_the_nodal_set_and_drops_the_rest() {
            let environ =
                b"NODAL_ID=01ARZ3NDEKTSV4RRFFQ69G5FAV\0USER=josh\0AWS_SECRET_ACCESS_KEY=x\0";
            let vars = kept(environ);
            assert_eq!(
                vars.get("NODAL_ID").map(String::as_str),
                Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            );
            assert_eq!(vars.get("USER").map(String::as_str), Some("josh"));
            assert!(
                !vars.contains_key("AWS_SECRET_ACCESS_KEY"),
                "a scan kept a value it was not asked for"
            );
        }

        #[test]
        fn a_record_that_is_not_text_is_skipped_rather_than_failing() {
            assert!(kept(b"\xff\xfe=x\0").is_empty());
        }
    }
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

    use super::command_of;

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
