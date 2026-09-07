//! Secrets: three tiers behind one seam, and a value type that cannot be printed.
//!
//! A recipe declares the *names* a working copy needs and never a value ([`crate::model::Env`]).
//! The values come from a [`SecretSource`], and there are two of those in V1: the
//! per-machine file at `~/.nodal/secrets.env`, and the values a unit minted for its own
//! services. A source is asked in the order of [`Origin`], so a unit-generated value
//! wins over a machine-wide one of the same name: the generated value is bound to
//! resources only this unit has, and the machine-wide one cannot be.
//!
//! Adding a password-manager CLI is one more implementation of the trait and no change
//! anywhere else. That is what the seam is for.
//!
//! # Why the value type is opaque
//!
//! [`Value`] holds its text in a private field. It has no [`std::fmt::Display`], its
//! [`std::fmt::Debug`] prints a placeholder, and its `serde` form is a placeholder too,
//! so a secret cannot reach a manifest, a log line, an error message or `--json` by
//! accident. The one way to read it is [`Value::expose`], which two functions call:
//! the writer of `.nodal/env` and the renderer of `nodal env --export`. A reviewer
//! checks that rule by grepping for one identifier.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::EnvName;
use crate::model::manifest::Origin;
use crate::{Error, Result};

/// The file a machine keeps its own values in, under the Nodal home.
pub const FILE_NAME: &str = "secrets.env";

/// The variable that moves the per-machine file, for tests and for a second profile.
pub const PATH_VAR: &str = "NODAL_SECRETS_FILE";

/// The mode the per-machine file is created with, and the only mode it is read at:
/// owner read and write, nothing for group or other.
pub const OWNER_ONLY: u32 = 0o600;

/// The permission bits that must be clear. Any of them means another account on this
/// machine can read the file.
pub const SHARED_BITS: u32 = 0o077;

/// A secret value. Opaque by construction; see the module documentation.
#[derive(Clone, PartialEq, Eq)]
pub struct Value(String);

/// What a redacted value renders as, wherever one is rendered.
pub const REDACTED: &str = "<redacted>";

impl Value {
    /// Wrap text as a secret value.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text itself, for the two writers that have to have it.
    ///
    /// Every other caller wants the name and the [`Origin`], which are not secret.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for Value {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(REDACTED)
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> core::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

/// Somewhere values can come from.
///
/// Both methods are cheap for the two V1 sources, which read a file once and answer
/// from memory. A source that has to run a program answers from a cache it fills on
/// first use, so that activating one home does not unlock a vault once per name.
pub trait SecretSource {
    /// Which tier this source is, which is what a report and a manifest record.
    fn tier(&self) -> Origin;

    /// The value this source has for `name`, if it has one.
    ///
    /// # Errors
    /// Whatever reaching the source reports. A source that simply does not hold the
    /// name answers `Ok(None)`: that is a report line, not a failure.
    fn lookup(&self, name: &EnvName) -> Result<Option<Value>>;
}

/// The per-machine file: `~/.nodal/secrets.env`, dotenv, owner-only.
#[derive(Debug, Clone, Default)]
pub struct MachineSecrets {
    /// The file it was read from, for a report that names it.
    path: PathBuf,
    /// What the file assigned.
    values: BTreeMap<EnvName, String>,
}

impl MachineSecrets {
    /// Where the file is: `NODAL_SECRETS_FILE` if it is set, else `<nodal_home>/secrets.env`.
    #[must_use]
    pub fn path_in(nodal_home: &Path) -> PathBuf {
        std::env::var_os(PATH_VAR).map_or_else(|| nodal_home.join(FILE_NAME), PathBuf::from)
    }

    /// Read the file. An absent file is an empty source, not a failure: a machine that
    /// has no secrets to give is a machine whose report lists every declared name.
    ///
    /// # Errors
    /// [`Error::SecretsPermissions`] when the file grants any access to group or other,
    /// and [`Error::Io`] when it exists and cannot be read.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { path, values: BTreeMap::new() });
            }
            Err(error) => return Err(Error::io(&path)(error)),
        };
        check_permissions(&path, &metadata)?;
        let text = std::fs::read_to_string(&path).map_err(Error::io(&path))?;
        Ok(Self { path, values: parse(&text) })
    }

    /// Read the file, creating an empty owner-only one first if it is not there.
    ///
    /// This is what a person's own `nodal env` calls, so that the first run on a
    /// machine leaves a file with the right mode to fill in. Activation calls
    /// [`MachineSecrets::open`] instead: writing a unit's home never touches a file
    /// every other unit shares.
    ///
    /// # Errors
    /// Whatever [`MachineSecrets::open`] reports, and [`Error::Io`] when the file or
    /// its directory cannot be created.
    pub fn open_or_create(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        create_owner_only(&path)?;
        Self::open(path)
    }

    /// The file these values came from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many names the file assigns. Its length is not a secret; its contents are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the file assigns nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl SecretSource for MachineSecrets {
    fn tier(&self) -> Origin {
        Origin::Machine
    }

    fn lookup(&self, name: &EnvName) -> Result<Option<Value>> {
        Ok(self.values.get(name).map(Value::new))
    }
}

/// The values a unit minted for its own services: a database role's password, a signing
/// key for its own stack. Service adapters fill it in ([`crate::model::Environment`]);
/// nothing here generates a value, because what a value has to be is the adapter's
/// knowledge and not this module's.
#[derive(Debug, Clone, Default)]
pub struct UnitGenerated(BTreeMap<EnvName, String>);

impl UnitGenerated {
    /// A source over what an adapter produced.
    #[must_use]
    pub fn new(values: BTreeMap<EnvName, String>) -> Self {
        Self(values)
    }

    /// Whether anything was generated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl SecretSource for UnitGenerated {
    fn tier(&self) -> Origin {
        Origin::Generated
    }

    fn lookup(&self, name: &EnvName) -> Result<Option<Value>> {
        Ok(self.0.get(name).map(Value::new))
    }
}

/// The first value any source has for `name`, with the tier that had it.
///
/// # Errors
/// Whatever a source reports while being asked.
pub fn resolve(sources: &[&dyn SecretSource], name: &EnvName) -> Result<Option<(Origin, Value)>> {
    for source in sources {
        if let Some(value) = source.lookup(name)? {
            return Ok(Some((source.tier(), value)));
        }
    }
    Ok(None)
}

/// Refuse a file any other account on the machine can read.
///
/// The check is the point of the file: a value in it is the one thing Nodal handles
/// that a person cannot re-derive. On a platform with no Unix mode there is nothing to
/// check and nothing is refused; the hosts Nodal supports are macOS and Linux, and
/// Windows through WSL2, which is Linux.
#[cfg(unix)]
fn check_permissions(path: &Path, metadata: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = metadata.permissions().mode() & 0o777;
    if mode & SHARED_BITS == 0 {
        return Ok(());
    }
    Err(Error::SecretsPermissions { path: PathBuf::from(path), mode })
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path, _metadata: &std::fs::Metadata) -> Result<()> {
    Ok(())
}

/// Create an empty file at [`OWNER_ONLY`], and its parent directory, if it is not there.
#[cfg(unix)]
fn create_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).mode(OWNER_ONLY).open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

#[cfg(not(unix))]
fn create_owner_only(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// The assignments in a dotenv file: `NAME=value`, one per line.
///
/// A line that is blank, a comment, or not an assignment is skipped, and so is a name
/// that is not an environment name. A value may be quoted with either quote character,
/// which is what a person's own file looks like when a value has a space in it.
fn parse(text: &str) -> BTreeMap<EnvName, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("export ").unwrap_or(line).split_once('='))
        .filter_map(|(name, value)| {
            Some((EnvName::parse(name.trim()).ok()?, unquote(value.trim())))
        })
        .collect()
}

/// Strip one matching pair of quotes from a value.
fn unquote(value: &str) -> String {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].to_owned();
        }
    }
    value.to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{Origin, REDACTED, SecretSource, UnitGenerated, Value, parse, resolve};
    use crate::model::EnvName;

    fn name(text: &str) -> EnvName {
        EnvName::parse(text).unwrap()
    }

    #[test]
    fn a_value_is_redacted_by_every_rendering_it_has() {
        let value = Value::new("hunter2");
        assert_eq!(format!("{value:?}"), REDACTED);
        assert_eq!(serde_json::to_string(&value).unwrap(), format!("\"{REDACTED}\""));
        assert_eq!(value.expose(), "hunter2");
    }

    #[test]
    fn dotenv_lines_that_are_not_assignments_are_skipped() {
        let values = parse(
            "# a comment\n\nSESSION_SECRET='s p a c e'\nexport CRON_SECRET=\"c\"\nnot a line\n",
        );
        assert_eq!(values.get(&name("SESSION_SECRET")).map(String::as_str), Some("s p a c e"));
        assert_eq!(values.get(&name("CRON_SECRET")).map(String::as_str), Some("c"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn a_generated_value_beats_a_machine_wide_one_of_the_same_name() {
        let generated =
            UnitGenerated::new([(name("POSTGRES_PASSWORD"), String::from("unit"))].into());
        let machine =
            UnitGenerated::new([(name("POSTGRES_PASSWORD"), String::from("machine"))].into());
        let sources: [&dyn SecretSource; 2] = [&generated, &machine];
        let (tier, value) = resolve(&sources, &name("POSTGRES_PASSWORD")).unwrap().unwrap();
        assert_eq!(tier, Origin::Generated);
        assert_eq!(value.expose(), "unit");
    }
}
