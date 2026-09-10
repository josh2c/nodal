//! Activation: the environment a unit's home carries, and the files that deliver it.
//!
//! A home is activated when a shell that enters it has the unit's variables. Nodal
//! spawns no subshell for that: it writes two files and lets the tools a person already
//! has read them. `.nodal/env` is a dotenv file with the unit's identity and the values
//! its own services generated, and `.envrc` reads it and then evaluates `nodal env
//! --export`. A shell with no direnv runs the same command through the rc hook.
//!
//! The person's own secrets are in neither file. They are resolved when somebody enters
//! the home ([`entering`]), from that person's own secrets file, so two accounts on one
//! host entering one home get two answers and neither reads the other's.
//!
//! [`resolve`] is the assembly and it is nearly pure: it takes the registry rows, the
//! recipe and the sources, and returns an [`Activation`]. [`files::write`] is the only
//! part that touches a disk.
//!
//! A declared name that no source answers is a line of the report and never a failure
//! ([`crate::model::Missing`]). A working copy whose mail credential is absent still
//! runs everything that does not send mail, and refusing to create it would be the
//! wrong trade every time.
//!
//! One class of name is the exception, and it is the exception because leaving it empty
//! breaks a step rather than one feature. A name under `env.generated` that no adapter
//! answered takes a stand-in ([`stand_in`]), so a project's generate step runs in a unit
//! whose services are not up yet. A stand-in is reported everywhere the name appears.

pub mod files;
pub mod secrets;
pub mod stand_in;
pub mod vars;

use std::collections::BTreeMap;
use std::path::Path;

use crate::Result;
use crate::model::manifest::{Manifest, Missing, Origin, Want};
use crate::model::{EnvName, Environment, Project, Recipe, Timestamp, Unit};
use crate::workspace::home;

pub use crate::env::secrets::{MachineSecrets, SecretSource, UnitGenerated};
pub use crate::env::stand_in::StandIns;

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
    /// Every name that holds a stand-in rather than a produced value, in file order.
    ///
    /// This is what the create reports and what every other surface reads back out of
    /// the manifest. A stand-in is never silent ([`crate::env::stand_in`]).
    #[must_use]
    pub fn stand_ins(&self) -> Vec<EnvName> {
        self.vars
            .iter()
            .filter(|var| var.origin == Origin::StandIn)
            .map(|var| var.name.clone())
            .collect()
    }

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
/// connection string. Names the recipe lists under `env.generated` are filled from it
/// first, because what a real generated value has to be is the adapter's knowledge
/// (`ServiceAdapter`). A name it does not carry falls to a stand-in ([`stand_in`]), and
/// to the missing list when the caller passes no minter.
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
    stand_ins: Option<&StandIns>,
    sources: &[&dyn SecretSource],
) -> Result<Activation> {
    let (unit, environment, project) = subject;
    let mut vars: Vec<EnvVar> = vars::identity(unit, environment, project)?
        .into_iter()
        .map(|(name, value)| EnvVar::new(name, vars::ORIGIN, secrets::Value::new(value)))
        .collect();
    let mut missing = Vec::new();

    for name in &recipe.env.generated {
        if let Some(value) = produced.0.get(name) {
            vars.push(EnvVar::new(name.clone(), Origin::Generated, secrets::Value::new(value)));
            continue;
        }
        match stand_ins.map(|minter| minter.mint(name)).transpose()?.flatten() {
            Some(value) => {
                vars.push(EnvVar::new(name.clone(), Origin::StandIn, secrets::Value::new(value)));
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

/// Every variable a shell entering `home` should carry, for the person running this.
///
/// Two halves. The first is `.nodal/env`, which says what the unit is and what its own
/// services generated; it is the same for everybody who enters. The second is this
/// person's secrets, resolved now from their own file, for each name the home was
/// activated with that a person supplies.
///
/// The names come from the manifest, which records an origin per name and the names
/// nothing answered. A name that was missing when the home was made is asked for again:
/// the person entering may hold a value the person who created it did not, and the
/// alternative is a home that stays half-activated until it is rebuilt.
///
/// A value already in `.nodal/env` is never replaced. Identity is Nodal's and a
/// generated value is the unit's, and neither is a person's to override.
///
/// # Errors
/// [`crate::Error::Io`] when `.nodal/env` cannot be read, [`crate::Error::Recipe`] when
/// the manifest is not one, [`crate::Error::SecretsPermissions`] when this person's
/// secrets file is readable by anybody else, and [`crate::Error::NoHomeDirectory`] when
/// nothing says where the state directory is.
pub fn entering(home: &Path) -> Result<Vec<(EnvName, String)>> {
    let mut pairs = files::read_dotenv(home)?;
    let manifest = files::read_manifest(home)?;
    let source = MachineSecrets::open(MachineSecrets::path_in(&home::directory()?))?;
    for name in wanted(&manifest) {
        if pairs.iter().any(|(held, _)| *held == name) {
            continue;
        }
        if let Some(value) = source.lookup(&name)? {
            pairs.push((name, value.expose().to_owned()));
        }
    }
    Ok(pairs)
}

/// The names a person's own secrets file is asked for, in one order every run repeats.
///
/// The manifest's own order: the names it recorded as coming from a secrets file, then
/// the names it recorded as answered by nothing. Both are sorted collections, so the
/// answer is the same bytes on every run and a diff of an export means something moved.
fn wanted(manifest: &Manifest) -> Vec<EnvName> {
    let held = manifest
        .env
        .iter()
        .filter(|(_, origin)| **origin == Origin::Machine)
        .map(|(name, _)| name.clone());
    let unanswered = manifest
        .missing
        .iter()
        .filter(|missing| matches!(missing.want, Want::Secret | Want::RequiredLocal))
        .map(|missing| missing.name.clone());
    let mut names: Vec<EnvName> = held.chain(unanswered).collect();
    names.dedup();
    names
}
