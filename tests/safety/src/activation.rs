//! One activated home: the three files a shell reads when it enters a unit.
//!
//! A test that asks what `nodal env` reports, or what an activated shell carries, needs
//! a home that has been activated. Activating one is not a matter of writing a file:
//! the engine resolves the recipe against what the run produced and against the secret
//! sources, and only then writes. Two suites did all of that themselves, with the same
//! secrets file, the same permission bits and the same call.
//!
//! The recipe and the rows are the caller's, because those are what each test is about.
//! The resolving and the writing are here.

use std::collections::BTreeMap;
use std::path::Path;

use nodal_core::env::secrets::{MachineSecrets, SecretSource, UnitGenerated};
use nodal_core::env::{self, Produced, files};
use nodal_core::model::{EnvName, Environment, Project, Recipe, Unit};

/// One environment name.
///
/// # Panics
///
/// If `text` is not one.
#[must_use]
pub fn name(text: &str) -> EnvName {
    EnvName::parse(text).expect("an environment name")
}

/// Write `secrets` into a per-machine secrets file only its owner may read.
///
/// The engine refuses a file that grants any access to group or other, so a fixture
/// that wrote one with the usual bits would be testing the refusal instead.
///
/// # Panics
///
/// If the file could not be written or its permission bits could not be set.
pub fn write_secrets(path: &Path, secrets: &str) {
    std::fs::write(path, secrets).expect("the secrets file is written");
    owner_only(path);
}

/// Resolve one unit's environment and write the three activation files into its home.
///
/// # Panics
///
/// If the secrets file could not be read, the environment could not be resolved, or
/// the files could not be written.
pub fn write(
    home: &Path,
    secrets_file: &Path,
    recipe: &Recipe,
    produced: BTreeMap<EnvName, String>,
    subject: (&Unit, &Environment, &Project),
) {
    let machine = MachineSecrets::open(secrets_file).expect("the secrets file is readable");
    let generated = UnitGenerated::default();
    let sources: [&dyn SecretSource; 2] = [&generated, &machine];

    let activation = env::resolve(subject, recipe, &Produced::new(produced), &sources)
        .expect("the environment resolves");
    let (unit, environment, project) = subject;
    let manifest = activation.manifest(unit, environment, project);
    files::write(home, &activation, &manifest).expect("the activation files are written");
}

/// Take every access off a path but the owner's.
#[cfg(unix)]
fn owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("the permission bits are set");
}

/// A host with no permission bits to set.
#[cfg(not(unix))]
fn owner_only(_path: &Path) {}
