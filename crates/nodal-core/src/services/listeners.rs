//! Which ports have a process listening on them, read from the kernel's own tables.
//!
//! Each host publishes one table, and the scan reads it:
//!
//! - Linux writes `/proc/net/tcp` and `/proc/net/tcp6`, one text row per socket.
//! - macOS answers `net.inet.tcp.pcblist_n`, one buffer of records, which holds IPv4 and
//!   IPv6 together. It is the table Apple's own `netstat` reads.
//!
//! The scan runs no external tool: `ss`, `lsof` and `netstat` are three different output
//! formats across distributions, and two of them are not installed by default. The
//! tables behind them are one format each that the kernel documents.
//!
//! The answer this gives is deliberately not the answer the registry gives. The
//! registry says which ports an environment was granted; this says which ports are
//! bound. An agent that ignores the environment it was given and binds 3000 anyway
//! shows up as the difference between the two.
//!
//! Both readings are host-wide, which is what makes that difference worth reading: they
//! answer "is this port bound by anything", not "is this port bound by a process this
//! account owns". On macOS that was measured — a socket an ordinary account may not
//! open, held by root, is in the buffer an ordinary account reads.

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
/// [`crate::Error::ListenerScanUnsupported`] on a host that publishes neither table,
/// [`crate::Error::Io`] when the host has its table but refused to answer for it.
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
#[cfg(target_os = "macos")]
pub fn listening_ports() -> Result<BTreeSet<u16>> {
    Ok(listening_in_pcblist(&macos::pcblist()?))
}

/// Every port a process on this host listens on.
///
/// # Errors
/// As [`scan`].
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

/// The record of a socket's address and ports, `XSO_INPCB` in `<sys/socketvar.h>`.
const XSO_INPCB: u32 = 0x010;

/// The record of a socket's TCP state, `XSO_TCPCB` in `<sys/socketvar.h>`.
const XSO_TCPCB: u32 = 0x020;

/// `t_state` of a socket that is listening: `TCPS_LISTEN` in `<netinet/tcp_fsm.h>`.
const TCPS_LISTEN: i32 = 1;

/// The size of an `xinpgen`, the header that opens the buffer and the record that closes
/// it. A record this short or shorter is that closing header.
const XINPGEN: usize = 24;

/// The boundary the kernel puts every record on.
const ALIGN: usize = 8;

/// Where each field the parser reads sits in its record.
///
/// The records are `xinpcb_n` and `xtcpcb_n` from Apple's `bsd/netinet/in_pcb.h` and
/// `bsd/netinet/tcp_var.h`, both under `#pragma pack(4)`. Three fields are read, and
/// each offset is the size of the fields before it:
///
/// | Record | Fields before it | Offset | Field |
/// |---|---|---|---|
/// | every one | — | 0 | `xgn_len`, `u32` |
/// | every one | `xgn_len` | 4 | `xgn_kind`, `u32` |
/// | `xinpcb_n` | the two above, `xi_inpp` `u64`, `inp_fport` `u16` | 18 | `inp_lport`, `u16` |
/// | `xtcpcb_n` | the two above, `t_segq` `u64`, `t_dupacks` `i32`, `t_timer[4]` `i32` | 36 | `t_state`, `i32` |
///
/// `inp_lport` is in network order, as `netstat` reading it through `ntohs` says; the
/// other two are in the host's own order.
mod at {
    /// The length of a record, which is also how far the next one is.
    pub(super) const LENGTH: usize = 0;
    /// Which record this is.
    pub(super) const KIND: usize = 4;
    /// The local port of an `xinpcb_n`.
    pub(super) const LOCAL_PORT: usize = 18;
    /// The connection state of an `xtcpcb_n`.
    pub(super) const STATE: usize = 36;
}

/// The listening ports in one `net.inet.tcp.pcblist_n` buffer.
///
/// The buffer is an `xinpgen` header, then one run of records for each socket, then a
/// second `xinpgen`. A socket's records arrive together and its `xinpcb_n` arrives
/// before its `xtcpcb_n`, so the port one record names belongs to the state the next one
/// gives. The closing `xinpgen` ends the walk by its length, not by its second field:
/// that field is a count of sockets, and a host with sixteen of them would otherwise
/// have the count read as `XSO_INPCB`.
///
/// Kept apart from the reading so that the parser is tested on every host, not only on
/// the one whose kernel writes the buffer.
#[must_use]
pub fn listening_in_pcblist(table: &[u8]) -> BTreeSet<u16> {
    let mut bound = BTreeSet::new();
    let Some(header) = length_at(table, at::LENGTH) else { return bound };
    let mut next = rounded(header);
    let mut port = None;
    while let Some(record) = table.get(next..) {
        let Some(length) = length_at(record, at::LENGTH) else { break };
        if length <= XINPGEN {
            break;
        }
        match field::<4>(record, at::KIND).map(u32::from_ne_bytes) {
            Some(XSO_INPCB) => port = field::<2>(record, at::LOCAL_PORT).map(u16::from_be_bytes),
            Some(XSO_TCPCB) => {
                if field::<4>(record, at::STATE).map(i32::from_ne_bytes) == Some(TCPS_LISTEN) {
                    bound.extend(port);
                }
                port = None;
            }
            _ => {}
        }
        next = next.saturating_add(rounded(length));
    }
    bound
}

/// The `u32` at `offset` read as a length, when the record reaches that far.
fn length_at(record: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(u32::from_ne_bytes(field::<4>(record, offset)?)).ok()
}

/// The `N` bytes at `offset`, when the record reaches that far.
fn field<const N: usize>(record: &[u8], offset: usize) -> Option<[u8; N]> {
    record.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

/// A length rounded up to the boundary the kernel puts the next record on.
fn rounded(length: usize) -> usize {
    length.next_multiple_of(ALIGN)
}

/// Asking macOS for its table of sockets, which the kernel answers as one buffer.
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, c_void};
    use std::path::PathBuf;

    use crate::{Error, Result};

    /// The table the scan reads.
    const TABLE: &CStr = c"net.inet.tcp.pcblist_n";

    /// How much more room the read asks for than the probe said. A socket that opens
    /// between the two calls then still fits, rather than the read answering `ENOMEM`.
    /// A record of a socket is about 600 bytes, so this is room for a hundred of them.
    const SLACK: usize = 64 * 1024;

    /// The table as the kernel writes it.
    ///
    /// # Errors
    /// [`Error::Io`] with the reason the kernel set, for the probe or for the read.
    pub(super) fn pcblist() -> Result<Vec<u8>> {
        let mut buffer = vec![0_u8; size()?.saturating_add(SLACK)];
        let mut written = buffer.len();
        // SAFETY: `TABLE` is a name that ends in a zero byte. `buffer` is `written`
        // bytes, the kernel writes at most that many into it and sets `written` to the
        // count it wrote. The last two arguments are the null that sets no new value.
        let answered = unsafe {
            libc::sysctlbyname(
                TABLE.as_ptr(),
                buffer.as_mut_ptr().cast::<c_void>(),
                &raw mut written,
                std::ptr::null_mut(),
                0,
            )
        };
        if answered != 0 {
            return Err(failed());
        }
        buffer.truncate(written);
        Ok(buffer)
    }

    /// How many bytes the table needs, which a read with no buffer asks for.
    fn size() -> Result<usize> {
        let mut size = 0_usize;
        // SAFETY: `TABLE` is a name that ends in a zero byte. A null buffer asks for the
        // size alone, so nothing is written to it and the answer is in `size`. The last
        // two arguments are the null that sets no new value.
        let answered = unsafe {
            libc::sysctlbyname(
                TABLE.as_ptr(),
                std::ptr::null_mut(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if answered == 0 { Ok(size) } else { Err(failed()) }
    }

    /// An error for a kernel call that failed, with the reason the kernel set.
    fn failed() -> Error {
        Error::Io {
            path: PathBuf::from(TABLE.to_string_lossy().as_ref()),
            source: std::io::Error::last_os_error(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{at, listening_in, listening_in_pcblist, local_port};

    /// A table as the kernel writes it: a header, a listener on 8080 (0x1F90), an
    /// established connection on 4000 (0x0FA0) which is not a listener, and a listener
    /// bound to one address rather than to all of them.
    const TABLE: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 26551 1
   1: 0100007F:0FA0 0100007F:C1B2 01 00000000:00000000 00:00000000 00000000  1000        0 26552 1
   2: 0100007F:4E20 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 26553 1
";

    /// A buffer this Mac's kernel wrote, cut to its header, three of its sockets and its
    /// closing header. Two of the three listen, on 61216 and on 7000; the third is an
    /// established connection on 61409. Every byte the parser does not read is zeroed,
    /// so the fixture carries no kernel address, process identifier or account of the
    /// machine it was taken from, and the record lengths and the three fields the parser
    /// reads are exactly as the kernel wrote them.
    const CAPTURED: &[u8] = include_bytes!("pcblist_n.bin");

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

    #[test]
    fn only_listening_records_are_reported() {
        let bound = listening_in_pcblist(CAPTURED);
        assert!(bound.contains(&61_216), "{bound:?}");
        assert!(bound.contains(&7_000), "{bound:?}");
        assert!(!bound.contains(&61_409), "an established connection is not a listener");
        assert_eq!(bound.len(), 2);
    }

    /// The closing `xinpgen` carries a count of sockets where a record carries its kind.
    /// A host with sixteen sockets writes the count `XSO_INPCB` there, and the walk must
    /// still end at that record rather than read a port out of it.
    #[test]
    fn the_closing_header_is_not_read_as_a_record() {
        let mut table = CAPTURED.to_vec();
        let kind = table.len() - super::XINPGEN + at::KIND;
        table[kind..kind + 4].copy_from_slice(&super::XSO_INPCB.to_ne_bytes());
        assert_eq!(listening_in_pcblist(&table), listening_in_pcblist(CAPTURED));
    }

    #[test]
    fn a_buffer_that_is_not_a_buffer_is_skipped() {
        assert!(listening_in_pcblist(&[]).is_empty());
        assert!(listening_in_pcblist(&[0; 7]).is_empty());
        assert!(listening_in_pcblist(&CAPTURED[..super::XINPGEN]).is_empty());
        // A record whose length runs past the end of the buffer ends the walk.
        assert!(listening_in_pcblist(&CAPTURED[..super::XINPGEN + 8]).is_empty());
    }
}
