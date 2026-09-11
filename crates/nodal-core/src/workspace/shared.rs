//! Shared mode: one registry for every person on a host, and the modes that allow it.
//!
//! Nodal's claim is one list of every unit. On a box where two engineers log in as two
//! accounts, one list means one registry, and one registry means a file both accounts
//! may write. That is a question about modes and about nothing else, so it is answered
//! here and read everywhere.
//!
//! # What says a host is shared
//!
//! The state root's own mode does. A directory with the setgid bit set hands its group
//! to everything created under it, which is exactly the arrangement a shared registry
//! needs, and it is a fact the filesystem already keeps. Nodal reads it and writes no
//! configuration key of its own: a key would be a second answer to a question the
//! directory has already answered, and the two would drift.
//!
//! `nodal init --shared <group>` is what *makes* a root like that. It is not what makes
//! Nodal treat one as shared. A root that a person set up with `chgrp` and `chmod g+s`
//! themselves is shared on the next command, with no flag typed.
//!
//! # What shared mode changes
//!
//! Three things, and nothing else.
//!
//! - The registry is [`REGISTRY_MODE`], and so are its `-wal` and `-shm` files. SQLite
//!   writes all three, and a group member who cannot write the write-ahead log cannot
//!   write the registry however the database file is moded.
//! - The process umask is [`UMASK`], so a directory or a file Nodal creates under the
//!   root keeps the group's write bit instead of having it masked away.
//! - A home under the root gets a group-readable `.nodal/env` ([`env_mode`]).
//!
//! What it does not change is where a person's secrets live. Those move out of the
//! state root altogether ([`crate::env::secrets`]), because one registry for a host is
//! the point and one secrets file for a host is the opposite of it.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The mode a shared state root is set to: the owner and the group may write, and the
/// setgid bit hands the group to everything created under it.
pub const ROOT_MODE: u32 = 0o2775;

/// The mode the registry and its two sidecar files are set to on a shared host.
pub const REGISTRY_MODE: u32 = 0o660;

/// The umask Nodal runs under on a shared host: nothing masked from the group.
pub const UMASK: u32 = 0o002;

/// The suffixes SQLite gives the two files it writes beside the database in WAL mode.
///
/// Both are moded with it. A group member who may write `registry.db` and not
/// `registry.db-wal` cannot commit a transaction, and the failure names the sidecar
/// rather than the registry, which sends a person to look at the wrong file.
pub const SIDECARS: [&str; 2] = ["-wal", "-shm"];

/// The mode `.nodal/env` is given in a home the state root does not hold.
pub const ENV_OWNER_ONLY: u32 = 0o600;

/// The mode `.nodal/env` is given in a home under a shared state root.
pub const ENV_SHARED: u32 = 0o660;

/// Whether this host keeps one registry for several accounts.
///
/// The setgid bit on the state root is the whole answer. A root that is not there yet
/// is not shared: the first command makes it, and `nodal init --shared` is what makes
/// it shared.
#[cfg(unix)]
#[must_use]
pub fn is_shared(root: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::metadata(root)
        .is_ok_and(|meta| meta.is_dir() && meta.permissions().mode() & SETGID != 0)
}

/// A host with no Unix modes keeps one registry per account.
#[cfg(not(unix))]
#[must_use]
pub fn is_shared(_root: &Path) -> bool {
    false
}

/// The setgid bit, as the permission bits carry it.
#[cfg(unix)]
const SETGID: u32 = 0o2000;

/// Bring this process and the registry files into line with the root's own mode.
///
/// [`crate::store::Store::open`] calls this on every open, and it is the one place
/// shared mode takes effect. On a root that is not shared it does nothing at all, so a
/// person with one account pays a `stat` and no behaviour change.
///
/// Nothing here fails. A registry another account owns cannot be moded by this one, and
/// it does not need to be: the account that created it created it under the same rule.
/// Failing an ordinary command over a mode that is already right would take the tool
/// away from the person who is holding it correctly.
pub fn adopt(registry: &Path) {
    let Some(root) = registry.parent() else { return };
    if !is_shared(root) {
        return;
    }
    set_umask(UMASK);
    relax(registry);
    for suffix in SIDECARS {
        relax(&sidecar(registry, suffix));
    }
}

/// The path of one of SQLite's sidecar files beside `registry`.
#[must_use]
pub fn sidecar(registry: &Path, suffix: &str) -> PathBuf {
    let mut name = registry.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The mode `.nodal/env` is created with for a home at `home`.
///
/// A home under a shared state root is a home two accounts may enter, and the identity
/// and generated values in its `.nodal/env` are what the second one needs to read. A
/// home anywhere else is a person's own checkout, adopted where it stands, and nothing
/// about a shared registry entitles the group to read inside it.
///
/// The file holds no secret either way ([`crate::env::files::dotenv`]).
#[must_use]
pub fn env_mode(home: &Path) -> u32 {
    let Ok(root) = crate::workspace::home::directory() else { return ENV_OWNER_ONLY };
    if home.starts_with(&root) && is_shared(&root) { ENV_SHARED } else { ENV_OWNER_ONLY }
}

/// Set one file's mode, and say nothing when it cannot be set.
#[cfg(unix)]
fn relax(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    if !path.exists() {
        return;
    }
    drop(std::fs::set_permissions(path, std::fs::Permissions::from_mode(REGISTRY_MODE)));
}

#[cfg(not(unix))]
fn relax(_path: &Path) {}

/// Put this process's umask where a shared root needs it.
#[cfg(unix)]
fn set_umask(mask: u32) {
    // SAFETY: `umask` takes a mode and returns the old one. It touches no memory.
    unsafe {
        libc::umask(mask as libc::mode_t);
    }
}

#[cfg(not(unix))]
fn set_umask(_mask: u32) {}

/// A group on this host: the name a person typed and the number the kernel uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// What the person typed, for the line that reports what was done.
    pub name: String,
    /// The number the filesystem records.
    pub gid: u32,
}

/// The group called `name`, or the group whose number `name` is.
///
/// # Errors
/// [`Error::UnknownGroup`] when this host has no such group.
#[cfg(unix)]
pub fn group(name: &str) -> Result<Group> {
    if let Ok(gid) = name.parse::<u32>() {
        return Ok(Group { name: name.to_owned(), gid });
    }
    let Ok(text) = std::ffi::CString::new(name) else {
        return Err(Error::UnknownGroup { group: name.to_owned() });
    };
    // SAFETY: `getgrnam` reads the C string and returns a pointer into its own static
    // storage. The one field read out of it is copied before anything else is called.
    let gid = unsafe {
        let found = libc::getgrnam(text.as_ptr());
        if found.is_null() { None } else { Some((*found).gr_gid) }
    };
    gid.map(|gid| Group { name: name.to_owned(), gid })
        .ok_or_else(|| Error::UnknownGroup { group: name.to_owned() })
}

/// A host with no Unix groups has none to find.
///
/// # Errors
/// [`Error::UnknownGroup`], always.
#[cfg(not(unix))]
pub fn group(name: &str) -> Result<Group> {
    Err(Error::UnknownGroup { group: name.to_owned() })
}

/// What [`share`] did, so that the command can print it rather than guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The state root.
    pub root: PathBuf,
    /// The group it now belongs to.
    pub group: Group,
    /// Whether this run created the directory.
    pub created: bool,
    /// The registry files whose mode this run set.
    pub relaxed: Vec<PathBuf>,
}

impl Made {
    /// One line per thing that was done, in the words a person can check with `ls`.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut said = vec![format!(
            "{verb} {root} as group {group}, mode {mode:o}",
            verb = if self.created { "created" } else { "set" },
            root = self.root.display(),
            group = self.group.name,
            mode = ROOT_MODE & 0o7777,
        )];
        for path in &self.relaxed {
            said.push(format!("set {path} to mode {REGISTRY_MODE:o}", path = path.display()));
        }
        said
    }
}

/// Make `root` a state root a group shares, and say what that took.
///
/// It is idempotent: a root that is already the right group at the right mode is set to
/// the same group at the same mode, and the report says what it now is rather than what
/// changed. That is what lets a person run it again after they add somebody to the
/// group.
///
/// The owner is not touched. A shared root belongs to whoever made it, and the group is
/// what the second account reaches it through.
///
/// # Errors
/// [`Error::Io`] when the directory could not be created or its mode could not be set,
/// and [`Error::GroupChange`] when the group could not be given to it.
#[cfg(unix)]
pub fn share(root: &Path, group: &Group) -> Result<Made> {
    use std::os::unix::fs::PermissionsExt as _;

    let created = !root.exists();
    std::fs::create_dir_all(root).map_err(Error::io(root))?;
    chown_group(root, group)?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(ROOT_MODE))
        .map_err(Error::io(root))?;
    set_umask(UMASK);
    let registry = root.join(crate::store::FILE_NAME);
    let mut relaxed = Vec::new();
    for path in std::iter::once(registry.clone())
        .chain(SIDECARS.into_iter().map(|suffix| sidecar(&registry, suffix)))
    {
        if !path.exists() {
            continue;
        }
        chown_group(&path, group)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(REGISTRY_MODE))
            .map_err(Error::io(&path))?;
        relaxed.push(path);
    }
    Ok(Made { root: root.to_path_buf(), group: group.clone(), created, relaxed })
}

/// A host with no Unix modes cannot be shared, and says so rather than half-doing it.
///
/// # Errors
/// [`Error::UnknownGroup`], always.
#[cfg(not(unix))]
pub fn share(_root: &Path, group: &Group) -> Result<Made> {
    Err(Error::UnknownGroup { group: group.name.clone() })
}

/// Give `path` to `group`, leaving its owner alone.
#[cfg(unix)]
fn chown_group(path: &Path, group: &Group) -> Result<()> {
    use std::os::unix::ffi::OsStrExt as _;

    let Ok(text) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return Err(Error::GroupChange {
            path: path.to_path_buf(),
            group: group.name.clone(),
            why: String::from("the path cannot be given to a system call"),
        });
    };
    // SAFETY: `chown` reads the C string. `-1` as a uid is the documented way to say
    // that the owner is not being changed.
    let code = unsafe { libc::chown(text.as_ptr(), u32::MAX, group.gid) };
    if code == 0 {
        return Ok(());
    }
    Err(Error::GroupChange {
        path: path.to_path_buf(),
        group: group.name.clone(),
        why: std::io::Error::last_os_error().to_string(),
    })
}
