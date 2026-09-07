//! The process table, read for the variables a process carries.
//!
//! A session is not something Nodal is told about; it is something Nodal can see. A
//! shell that entered an activated home carries `NODAL_ID`, and so does every process
//! that shell starts. Reading the process table is therefore how "who is in this unit"
//! is answered, and it is why nothing has to be installed into a person's repository
//! and no shell has to be wrapped.
//!
//! [`Processes`] is the seam, so that a test supplies a table instead of a machine.
//! [`Live`] is the one implementation: on Linux it reads `/proc/<pid>/environ`. macOS
//! needs `sysctl` and arrives with the attribution task; until then a scan there
//! reports that it is not supported rather than reporting an empty machine, because
//! "nobody is attached" and "I cannot see" are different answers.
//!
//! A scan keeps only the variables it was asked for: the `NODAL_*` set and the names
//! that say who the actor is. Everything else a process carries is read and dropped.
//! The reading itself is in the [`linux`] module, because `/proc` is the only source
//! there is until `sysctl` arrives with the attribution task; that module is what grows
//! a second host, rather than this file growing a second set of conditions.

use std::collections::BTreeMap;

use crate::Result;

/// One process, with the variables a scan keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The process identifier.
    pub pid: u32,
    /// The variables that were kept, by name.
    pub vars: BTreeMap<String, String>,
}

impl Running {
    /// One variable, when the process carries it.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }
}

/// Where the process table comes from.
pub trait Processes {
    /// Every process this account can read, with the variables worth keeping.
    ///
    /// # Errors
    /// [`Error::ProcessScanUnsupported`] on a host with no readable process table, and
    /// [`Error::Io`] when the table itself cannot be listed.
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
/// with another source keeps the same variables, so the day macOS arrives this becomes
/// two modules over one list rather than one module under two conditions.
#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;

    use super::Running;
    use crate::runtime::actor;
    use crate::{Error, Result};

    /// The prefix of every variable a home's identity is written with.
    const PREFIX: &str = "NODAL_";

    /// Where the kernel publishes the process table.
    const PROC: &str = "/proc";

    /// The name of the file in a process's directory that holds its environment.
    const ENVIRON: &str = "environ";

    /// Every process this account can read, with the variables worth keeping.
    pub(super) fn scan() -> Result<Vec<Running>> {
        let directory = std::path::Path::new(PROC);
        let entries = std::fs::read_dir(directory).map_err(Error::io(directory))?;
        let mut running = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
                continue;
            };
            // A process this account may not read, and a process that ended between the
            // listing and the read, are both "not visible" and neither is a failure.
            let Ok(environ) = std::fs::read(entry.path().join(ENVIRON)) else { continue };
            let vars = kept(&environ);
            if !vars.is_empty() {
                running.push(Running { pid, vars });
            }
        }
        Ok(running)
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
