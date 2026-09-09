//! How this copy of Nodal was installed, and the one command that upgrades it.
//!
//! Nodal has no self-updater and makes no network call of its own. Upgrading is the
//! package manager's job. `nodal upgrade` and `nodal update` therefore answer the
//! same question and nothing else: *what put this binary here, and what do I type to
//! get a newer one?* Both verbs answer identically, because both are one call to
//! [`read`] and one rendering of the answer.
//!
//! The answer is read, not guessed. The path of the running executable says which
//! channel installed it, and `/etc/os-release` says which package manager a system
//! install belongs to. Nothing is fetched, nothing is executed, and no version is
//! compared against anything: a version comparison needs a network call to be worth
//! making, and Nodal does not make one.
//!
//! Detection is pure over its inputs ([`of`]), so the table below is what the tests
//! read rather than the machine they happen to run on.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The channel a copy of Nodal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// `cargo install`, which puts the binary in Cargo's own bin directory.
    Cargo,
    /// Homebrew, which puts it in a cellar and links it onto the path.
    Homebrew,
    /// The system package manager, which puts it in a system bin directory.
    System,
    /// A binary a person put where they wanted it. The release archive, a copy into
    /// `~/bin`, a build from source.
    Binary,
}

/// The directory segments that say a path is inside a Cargo installation.
const CARGO: [&str; 2] = [".cargo", "bin"];

/// The segment Homebrew names every installed version under.
const CELLAR: &str = "Cellar";

/// The prefixes Homebrew links its binaries into.
const BREW_PREFIXES: [&str; 3] =
    ["/opt/homebrew/bin", "/usr/local/Homebrew", "/home/linuxbrew/.linuxbrew"];

/// The directories a system package manager owns.
const SYSTEM: [&str; 4] = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

/// One system package manager, and the command that upgrades one package with it.
struct Manager {
    /// The `ID` or `ID_LIKE` field of `/etc/os-release` that names this family.
    id: &'static str,
    /// What a person types.
    command: &'static str,
}

/// The families Nodal names a command for. Anything else gets the generic line.
const MANAGERS: &[Manager] = &[
    Manager { id: "arch", command: "sudo pacman -Syu nodal" },
    Manager { id: "debian", command: "sudo apt update && sudo apt install --only-upgrade nodal" },
    Manager { id: "ubuntu", command: "sudo apt update && sudo apt install --only-upgrade nodal" },
    Manager { id: "fedora", command: "sudo dnf upgrade nodal" },
    Manager { id: "rhel", command: "sudo dnf upgrade nodal" },
    Manager { id: "alpine", command: "sudo apk upgrade nodal" },
];

/// What a system install says when the family is not one of the rows above.
const UNKNOWN_MANAGER: &str = "upgrade the nodal package with this system's package manager";

/// Where a release archive is published. Printed, never fetched.
pub const RELEASES: &str = "https://github.com/josh2c/nodal/releases";

/// What Nodal knows about how it was installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
    /// The channel that put the binary where it is.
    pub channel: Channel,
    /// The binary that is running.
    pub path: PathBuf,
    /// The one command that upgrades this copy.
    pub command: String,
}

/// Read this machine: where the running binary is, and which package family it is on.
///
/// The two readings are one environment variable and one file, both of them local.
#[must_use]
pub fn read() -> Install {
    let path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("nodal"));
    let os_release = std::fs::read_to_string("/etc/os-release").ok();
    describe(
        &path,
        std::env::var_os("CARGO_HOME").map(PathBuf::from).as_deref(),
        os_release.as_deref(),
    )
}

/// The whole answer for a binary at `path`, with `cargo_home` and the contents of
/// `/etc/os-release` as they were read.
#[must_use]
pub fn describe(path: &Path, cargo_home: Option<&Path>, os_release: Option<&str>) -> Install {
    let channel = of(path, cargo_home);
    Install { channel, path: path.to_path_buf(), command: command(channel, os_release) }
}

/// Which channel a binary at `path` came from.
///
/// The order matters. A Homebrew cellar under `/usr/local` is a Homebrew install and
/// not a system one, so the cellar is looked for before the system directories.
#[must_use]
pub fn of(path: &Path, cargo_home: Option<&Path>) -> Channel {
    if in_cargo(path, cargo_home) {
        return Channel::Cargo;
    }
    if in_homebrew(path) {
        return Channel::Homebrew;
    }
    if path.parent().is_some_and(|parent| SYSTEM.iter().any(|owned| parent == Path::new(owned))) {
        return Channel::System;
    }
    Channel::Binary
}

/// The command that upgrades a copy from `channel`.
#[must_use]
pub fn command(channel: Channel, os_release: Option<&str>) -> String {
    match channel {
        Channel::Cargo => String::from("cargo install nodal --force"),
        Channel::Homebrew => String::from("brew upgrade nodal"),
        Channel::System => manager(os_release).to_owned(),
        Channel::Binary => format!("download the release for this platform from {RELEASES}"),
    }
}

/// Whether a path is inside a Cargo installation.
fn in_cargo(path: &Path, cargo_home: Option<&Path>) -> bool {
    if cargo_home.is_some_and(|home| path.starts_with(home.join("bin"))) {
        return true;
    }
    path.parent().is_some_and(|parent| {
        let segments: Vec<_> = parent.iter().collect();
        segments.windows(CARGO.len()).any(|pair| pair == CARGO.map(std::ffi::OsStr::new))
    })
}

/// Whether a path is inside a Homebrew installation.
fn in_homebrew(path: &Path) -> bool {
    path.iter().any(|segment| segment == CELLAR)
        || BREW_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// The upgrade command for the family `/etc/os-release` names.
///
/// `ID` is read first and `ID_LIKE` second, so a derivative that declares its parent
/// gets the parent's command instead of the generic line.
fn manager(os_release: Option<&str>) -> &'static str {
    let Some(text) = os_release else { return UNKNOWN_MANAGER };
    for field in ["ID", "ID_LIKE"] {
        for name in values(text, field) {
            if let Some(found) = MANAGERS.iter().find(|manager| manager.id == name) {
                return found.command;
            }
        }
    }
    UNKNOWN_MANAGER
}

/// The words one `/etc/os-release` field holds, unquoted.
fn values<'a>(text: &'a str, field: &str) -> Vec<&'a str> {
    text.lines()
        .filter_map(|line| line.strip_prefix(field)?.strip_prefix('='))
        .flat_map(|value| value.trim_matches(['"', '\'']).split_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Channel, RELEASES, command, describe, of};

    #[test]
    fn a_binary_in_cargos_bin_directory_is_a_cargo_install() {
        let path = Path::new("/home/dev/.cargo/bin/nodal");
        assert_eq!(of(path, None), Channel::Cargo);
        assert_eq!(command(Channel::Cargo, None), "cargo install nodal --force");
    }

    #[test]
    fn a_cargo_home_somewhere_else_is_still_a_cargo_install() {
        let path = Path::new("/opt/rust/bin/nodal");
        assert_eq!(of(path, Some(Path::new("/opt/rust"))), Channel::Cargo);
    }

    #[test]
    fn a_binary_in_a_cellar_is_a_homebrew_install_and_not_a_system_one() {
        for path in ["/usr/local/Cellar/nodal/0.1.0/bin/nodal", "/opt/homebrew/bin/nodal"] {
            assert_eq!(of(Path::new(path), None), Channel::Homebrew, "{path}");
        }
        assert_eq!(command(Channel::Homebrew, None), "brew upgrade nodal");
    }

    #[test]
    fn a_binary_in_a_system_directory_names_that_systems_package_manager() {
        assert_eq!(of(Path::new("/usr/bin/nodal"), None), Channel::System);
        assert_eq!(command(Channel::System, Some("ID=arch\n")), "sudo pacman -Syu nodal");
        assert!(command(Channel::System, Some("ID=ubuntu\n")).contains("apt"));
        assert!(command(Channel::System, Some("ID=\"fedora\"\n")).contains("dnf"));
    }

    #[test]
    fn a_derivative_that_declares_its_parent_gets_the_parents_command() {
        let text = "ID=cachyos\nID_LIKE=arch\n";
        assert_eq!(command(Channel::System, Some(text)), "sudo pacman -Syu nodal");
    }

    #[test]
    fn a_system_nodal_does_not_know_is_told_to_use_its_own_package_manager() {
        let told = command(Channel::System, Some("ID=plan9\n"));
        assert!(told.contains("package manager"), "{told}");
        assert!(!told.contains("sudo"), "and it guesses no command: {told}");
    }

    #[test]
    fn a_binary_a_person_placed_is_pointed_at_the_release_page() {
        let install = describe(Path::new("/home/dev/bin/nodal"), None, None);
        assert_eq!(install.channel, Channel::Binary);
        assert!(install.command.contains(RELEASES), "{}", install.command);
    }

    #[test]
    fn every_channel_answers_with_one_command_and_never_with_nothing() {
        for channel in [Channel::Cargo, Channel::Homebrew, Channel::System, Channel::Binary] {
            assert!(!command(channel, None).is_empty(), "{channel:?} has no command");
        }
    }
}
