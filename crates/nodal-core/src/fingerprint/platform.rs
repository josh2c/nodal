//! The platform triple, which is part of every workspace fingerprint.
//!
//! A base is a checkout with dependencies installed and a build warmed. Native modules,
//! downloaded binaries and build caches in it are built for one platform, so a base
//! built on `aarch64-apple-darwin` is not warm on `x86_64-unknown-linux-gnu` even
//! though the tree is identical. The triple is in the key so the two cannot collide.

use crate::Result;
use crate::model::Platform;

/// The triple this process is running as.
///
/// Composed from the constants the standard library exposes rather than read from a
/// build script, so it holds for a binary however it was built.
///
/// # Errors
/// [`crate::Error::InvalidValue`] if a target ever reports a component with a space
/// in it; no supported target does.
pub fn current() -> Result<Platform> {
    let (vendor, system) = vendor_and_system(std::env::consts::OS);
    Platform::parse(format!("{arch}-{vendor}-{system}", arch = std::env::consts::ARCH))
}

/// The vendor and system halves of the triple for an operating system, as a table.
/// An unknown target still gets a well-formed triple naming itself.
fn vendor_and_system(os: &str) -> (&'static str, &'static str) {
    match os {
        "macos" => ("apple", "darwin"),
        "ios" => ("apple", "ios"),
        "windows" => ("pc", WINDOWS_ABI),
        "linux" => ("unknown", LINUX_ABI),
        "android" => ("linux", "android"),
        _ => ("unknown", OTHER_SYSTEM),
    }
}

/// The C ABI half of a Linux triple.
const LINUX_ABI: &str = if cfg!(target_env = "musl") { "linux-musl" } else { "linux-gnu" };

/// The C ABI half of a Windows triple.
const WINDOWS_ABI: &str = if cfg!(target_env = "gnu") { "windows-gnu" } else { "windows-msvc" };

/// The system half for a target this table does not name; the OS names itself.
const OTHER_SYSTEM: &str = std::env::consts::OS;

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{current, vendor_and_system};

    #[test]
    fn the_current_triple_has_three_parts_and_names_this_architecture() {
        let triple = current().unwrap();
        let text = triple.as_str();
        assert!(text.starts_with(std::env::consts::ARCH), "{text}");
        // Three fields, the last of which may itself carry an ABI (`linux-gnu`).
        assert!(matches!(text.split('-').count(), 3 | 4), "{text}");
        assert!(!text.contains(' '));
    }

    #[test]
    fn the_triple_is_the_same_every_time_it_is_asked_for() {
        assert_eq!(current().unwrap(), current().unwrap());
    }

    #[test]
    fn known_systems_get_their_conventional_vendor() {
        assert_eq!(vendor_and_system("macos"), ("apple", "darwin"));
        assert_eq!(vendor_and_system("linux").0, "unknown");
        assert_eq!(vendor_and_system("windows").0, "pc");
        assert_eq!(vendor_and_system("android"), ("linux", "android"));
    }
}
