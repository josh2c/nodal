//! Activation: the environment a unit's home carries, and the files that deliver it.
//!
//! A home is activated when a shell that enters it has the unit's variables. Nodal
//! spawns no subshell for that: it writes two files and lets the tools a
//! person already has read them. `.nodal/env` is a dotenv file with the resolved set,
//! and `.envrc` is one line, `dotenv .nodal/env`, which direnv acts on. A shell with no
//! direnv gets the same set from `nodal env --export`, which is the fallback the rc
//! hook uses. Both consume what this module writes.
//!
//! [`resolve`] is the assembly and it is nearly pure: it takes the registry rows, the
//! recipe and the sources, and returns an [`Activation`]. [`files::write`] is the only
//! part that touches a disk.
//!
//! A declared name that no source answers is a line of the report and never a failure
//! ([`crate::model::Missing`]). A working copy whose mail credential is absent still
//! runs everything that does not send mail, and refusing to create it would be the
//! wrong trade every time.

pub mod files;
pub mod secrets;
pub mod vars;

use std::collections::BTreeMap;

use crate::Result;
use crate::model::manifest::{Manifest, Missing, Origin, Want};
use crate::model::{EnvName, Environment, Project, Recipe, Timestamp, Unit};

pub use crate::env::secrets::{MachineSecrets, SecretSource, UnitGenerated};

/// One variable of an activated home.
///
/// The value is private, and it is private for the same reason [`secrets::Value`] is:
/// once one type in this module can be printed, every report that holds it can leak.
/// [`EnvVar::name`] and [`EnvVar::origin`] are what a manifest, a report and a log
/// line are allowed to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    /// The name, as it appears left of `=`.
    name: EnvName,
    /// Who supplied the value.
    origin: Origin,
    /// The value. Never rendered; see [`EnvVar::expose`].
    value: secrets::Value,
}

impl EnvVar {
    /// A variable with a value from `origin`.
    #[must_use]
    pub fn new(name: EnvName, origin: Origin, value: secrets::Value) -> Self {
        Self { name, origin, value }
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &EnvName {
        &self.name
    }

    /// Who supplied it.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }

    /// The value, for the two writers that have to have it: the dotenv file and the
    /// export rendering. Nothing else calls this.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.value.expose()
    }
}

/// What a unit's home is activated with: the variables, and the names nothing answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    /// Every variable, in the order a file lists them: identity first, then the
    /// declared names in the recipe's order.
    pub vars: Vec<EnvVar>,
    /// Every declared name no source answered.
    pub missing: Vec<Missing>,
}

impl Activation {
    /// The variable with this name, if the home has one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&EnvVar> {
        self.vars.iter().find(|var| var.name.as_str() == name)
    }

    /// The names and origins, which is what the manifest keeps.
    #[must_use]
    pub fn origins(&self) -> BTreeMap<EnvName, Origin> {
        self.vars.iter().map(|var| (var.name.clone(), var.origin)).collect()
    }

    /// The manifest for this activation: identity, names, origins, and what is missing.
    #[must_use]
    pub fn manifest(&self, unit: &Unit, environment: &Environment, project: &Project) -> Manifest {
        Manifest {
            unit: unit.id,
            environment: environment.id,
            slug: unit.slug.clone(),
            project: project.name.clone(),
            host: environment.host.clone(),
            home: environment.home.clone(),
            written_at: Timestamp::now(),
            env: self.origins(),
            missing: self.missing.clone(),
        }
    }
}

/// What activation is assembled from, besides the recipe and the registry rows.
///
/// `generated` is what the service adapters produced for this unit — ports, URLs, a
/// connection string. Names the recipe lists under `env.generated` are filled from it;
/// a name it does not carry is reported missing rather than invented here, because what
/// a generated value has to be is the adapter's knowledge (`ServiceAdapter`).
#[derive(Debug, Clone, Default)]
pub struct Produced(BTreeMap<EnvName, String>);

impl Produced {
    /// The values a set of adapters produced.
    #[must_use]
    pub fn new(values: BTreeMap<EnvName, String>) -> Self {
        Self(values)
    }

    /// Whether nothing was produced.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Assemble the environment for one materialisation.
///
/// The order is fixed: the `NODAL_*` identity variables, then `env.generated`, then
/// `env.required_local`, then `env.secrets`, each in the recipe's own order. A fixed
/// order is what makes `.nodal/env` the same bytes for the same inputs, so rewriting it
/// is idempotent and a diff of it means something changed.
///
/// # Errors
/// Whatever a secret source reports while being asked, and
/// [`crate::Error::InvalidValue`] if a `NODAL_*` constant is not a valid name.
pub fn resolve(
    subject: (&Unit, &Environment, &Project),
    recipe: &Recipe,
    produced: &Produced,
    sources: &[&dyn SecretSource],
) -> Result<Activation> {
    let (unit, environment, project) = subject;
    let mut vars: Vec<EnvVar> = vars::identity(unit, environment, project)?
        .into_iter()
        .map(|(name, value)| EnvVar::new(name, vars::ORIGIN, secrets::Value::new(value)))
        .collect();
    let mut missing = Vec::new();

    for name in &recipe.env.generated {
        match produced.0.get(name) {
            Some(value) => {
                vars.push(EnvVar::new(name.clone(), Origin::Generated, secrets::Value::new(value)));
            }
            None => missing.push(Missing { name: name.clone(), want: Want::Generated }),
        }
    }
    for (names, want) in
        [(&recipe.env.required_local, Want::RequiredLocal), (&recipe.env.secrets, Want::Secret)]
    {
        for name in names {
            match secrets::resolve(sources, name)? {
                Some((origin, value)) => vars.push(EnvVar::new(name.clone(), origin, value)),
                None => missing.push(Missing { name: name.clone(), want }),
            }
        }
    }
    dedupe(&mut vars);
    Ok(Activation { vars, missing })
}

/// Keep the first variable with each name.
///
/// The identity variables come first, so a recipe that declares `NODAL_ID` cannot make
/// a home lie about which unit it is, and a name declared in two of the recipe's lists
/// is written once rather than twice.
fn dedupe(vars: &mut Vec<EnvVar>) {
    let mut seen = std::collections::BTreeSet::new();
    vars.retain(|var| seen.insert(var.name.clone()));
}
