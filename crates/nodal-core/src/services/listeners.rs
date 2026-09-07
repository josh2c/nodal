//! Which ports have a process listening on them, read from the kernel's own tables.
//!
//! The scan reads `/proc/net/tcp` and `/proc/net/tcp6`. It runs no external tool: `ss`,
//! `lsof` and `netstat` are three different output formats across distributions, and
//! two of them are not installed by default. The files behind them are one format that
//! the kernel documents.
//!
//! The answer this gives is deliberately not the answer the registry gives. The
//! registry says which ports an environment was granted; this says which ports are
//! bound. An agent that ignores the environment it was given and binds 3000 anyway
//! shows up as the difference between the two.

use std::collections::BTreeSet;

use crate::Result;
use crate::model::{PortName, Ports};

/// The tables the scan reads, in the order it reads them.
#[cfg(target_os = "linux")]
const SOURCES: &[&str] = &["/proc/net/tcp", "/proc/net/tcp6"];

/// The `st` column of a socket in the LISTEN state. The column is a hexadecimal
/// `TCP_LISTEN`, which is 10.
const LISTEN: &str = "0A";

/// One granted port, and whether anything on this host listens on it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Listening {
    /// What the recipe calls the port.
    pub name: PortName,
    /// The port itself.
    pub port: u16,
    /// Whether a process on this host has it bound and listening.
    pub listening: bool,
}

/// Report, for every port an environment was granted, whether it has a listener.
///
/// # Errors
/// [`crate::Error::ListenerScanUnsupported`] on a host without `/proc/net/tcp`,
/// [`crate::Error::Io`] when a table is there but could not be read.
pub fn scan(granted: &Ports) -> Result<Vec<Listening>> {
    let bound = listening_ports()?;
    Ok(granted
        .0
        .iter()
        .map(|(name, port)| Listening {
            name: name.clone(),
            port: *port,
            listening: bound.contains(port),
        })
        .collect())
}

/// Every port a process on this host listens on.
///
/// # Errors
/// As [`scan`].
#[cfg(target_os = "linux")]
pub fn listening_ports() -> Result<BTreeSet<u16>> {
    let mut bound = BTreeSet::new();
    for source in SOURCES {
        let path = std::path::Path::new(source);
        match std::fs::read_to_string(path) {
            Ok(table) => bound.extend(listening_in(&table)),
            // A kernel built without IPv6 has no tcp6 table. That is an answer, not a
            // failure: there is nothing listening on a stack that is not there.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(crate::Error::io(path)(error)),
        }
    }
    Ok(bound)
}

/// Every port a process on this host listens on.
///
/// # Errors
/// As [`scan`].
#[cfg(not(target_os = "linux"))]
pub fn listening_ports() -> Result<BTreeSet<u16>> {
    Err(crate::Error::ListenerScanUnsupported { host: std::env::consts::OS })
}

/// The listening ports in the text of one `/proc/net/tcp` table.
///
/// Kept apart from the reading so that the parser is tested on every host, not only on
/// the one whose kernel writes the file.
#[must_use]
pub fn listening_in(table: &str) -> BTreeSet<u16> {
    table.lines().filter_map(local_port).collect()
}

/// The local port of one row, when that row is a listening socket.
///
/// The columns are `sl`, `local_address`, `rem_address`, `st`, and the rest. An address
/// is `<address in hexadecimal>:<port in hexadecimal>`. The header row parses as a row
/// whose state is not `0A`, so it needs no case of its own.
fn local_port(row: &str) -> Option<u16> {
    let mut columns = row.split_whitespace().skip(1);
    let local = columns.next()?;
    if columns.nth(1)? != LISTEN {
        return None;
    }
    u16::from_str_radix(local.rsplit_once(':')?.1, 16).ok()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{listening_in, local_port};

    /// A table as the kernel writes it: a header, a listener on 8080 (0x1F90), an
    /// established connection on 4000 (0x0FA0) which is not a listener, and a listener
    /// bound to one address rather than to all of them.
    const TABLE: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 26551 1
   1: 0100007F:0FA0 0100007F:C1B2 01 00000000:00000000 00:00000000 00000000  1000        0 26552 1
   2: 0100007F:4E20 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 26553 1
";

    #[test]
    fn only_listening_rows_are_reported() {
        let bound = listening_in(TABLE);
        assert!(bound.contains(&8080), "{bound:?}");
        assert!(bound.contains(&20000), "{bound:?}");
        assert!(!bound.contains(&4000), "an established connection is not a listener");
        assert_eq!(bound.len(), 2);
    }

    #[test]
    fn a_row_that_is_not_a_row_is_skipped() {
        assert_eq!(local_port("  sl  local_address rem_address   st tx_queue"), None);
        assert_eq!(local_port(""), None);
        assert_eq!(local_port("   0: 00000000 00000000:0000 0A"), None);
        assert_eq!(local_port("   0: 00000000:ZZZZ 00000000:0000 0A"), None);
        assert!(listening_in("").is_empty());
    }
}
