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
}

/// The processes of this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live;

impl Processes for Live {
    fn scan(&self) -> Result<Vec<Running>> {
        scan_this_host()
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

/// Reading the process table of a Linux host, which publishes it as `/proc`.
///
/// Everything a `/proc` scan needs is here, including which variables it keeps: a host
/// with another source keeps the same three things, so the day macOS arrives this
/// becomes two modules over one list rather than one module under two conditions.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{Running, command_of};
    use crate::runtime::actor;
    use crate::{Error, Result};

    /// The prefix of every variable a home's identity is written with.
    const PREFIX: &str = "NODAL_";

    /// Where the kernel publishes the process table.
    const PROC: &str = "/proc";

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
