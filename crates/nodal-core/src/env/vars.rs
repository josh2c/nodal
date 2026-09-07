//! The `NODAL_*` variables: the names, in one place, and the values a home carries.
//!
//! These five names are the contract other tools read (`docs/contracts.md`). A process
//! started in an activated home carries them, which is how attribution knows whose
//! process it is and how an agent knows which unit it is working in.

use std::path::Path;

use crate::Result;
use crate::model::manifest::Origin;
use crate::model::{EnvName, Environment, Project, Unit};

/// The unit's identifier.
pub const ID: &str = "NODAL_ID";

/// The unit's CLI handle.
pub const UNIT: &str = "NODAL_UNIT";

/// The project the unit belongs to.
pub const PROJECT: &str = "NODAL_PROJECT";

/// The host that holds the writable copy.
pub const HOST: &str = "NODAL_HOST";

/// The home directory itself.
pub const ROOT: &str = "NODAL_ROOT";

/// Every identity name, in the order a file lists them.
pub const ALL: &[&str] = &[ID, UNIT, PROJECT, HOST, ROOT];

/// Where an identity variable comes from. One constant, so the assembler does not
/// repeat itself.
pub const ORIGIN: Origin = Origin::Identity;

/// The identity variables for one materialisation, as name and value pairs.
///
/// Every name in [`ALL`] is produced, so a caller never has to check whether one is
/// there. The values come from the registry rows rather than from the machine, which is
/// what makes a home readable after it has been moved.
///
/// # Errors
///
/// [`crate::Error::InvalidValue`] if one of the constants above is not a valid
/// environment name. The unit test in this file is what keeps that from happening.
pub fn identity(
    unit: &Unit,
    environment: &Environment,
    project: &Project,
) -> Result<Vec<(EnvName, String)>> {
    let pairs = [
        (ID, unit.id.to_string()),
        (UNIT, unit.slug.to_string()),
        (PROJECT, project.name.to_string()),
        (HOST, environment.host.to_string()),
        (ROOT, display(&environment.home)),
    ];
    pairs.into_iter().map(|(name, value)| Ok((EnvName::parse(name)?, value))).collect()
}

/// A path as a variable value. A path that is not UTF-8 keeps its lossy form: a home
/// Nodal created is always UTF-8, and an adopted directory that is not still gets a
/// readable value rather than none.
fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::ALL;
    use crate::model::EnvName;

    #[test]
    fn every_identity_name_is_a_valid_environment_name() {
        for text in ALL {
            assert!(EnvName::parse(*text).is_ok(), "{text} is not an environment name");
        }
    }
}
