//! The recipe: what `nodal.toml` says a project needs to be a working copy.
//!
//! The type is the contract in `docs/contracts.md`, and it is deliberately sparse:
//! every key is optional, because a recipe is the difference between what Nodal can
//! infer and what only a person knows. The same type carries an explicit file, an
//! inferred proposal and the merge of the two ([`crate::recipe`]), so precedence is one
//! field-wise rule rather than three shapes that have to be kept in step.
//!
//! Accessors such as [`Recipe::backend`] give the default for a key nobody set, so a
//! caller never has to know which defaults exist.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::environment::PortName;
use crate::model::scalar::{self, string_newtype};

string_newtype! {
    /// A shell command line a recipe runs, for example `pnpm run dev`.
    CommandLine, kind = "command line", shape = scalar::LINE
}

string_newtype! {
    /// The name of an environment variable, as it appears left of `=` in a dotenv file.
    EnvName, kind = "environment variable name", shape = scalar::ENV_NAME
}

string_newtype! {
    /// A tool a toolchain pins, for example `node` or `engines.node`.
    ToolName, kind = "tool name", shape = scalar::TOKEN
}

string_newtype! {
    /// The version a toolchain pins a tool to, as written in the pin file.
    ToolVersion, kind = "tool version", shape = scalar::LINE
}

string_newtype! {
    /// A service a project runs, by the name its stack gives it.
    ServiceName, kind = "service name", shape = scalar::SLUG
}

/// Where a unit's commands run.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Directly on the host, with the toolchain the host provides.
    Native,
    /// Inside the project's dev container.
    Devcontainer,
    /// Inside a Nix shell defined by the project.
    Nix,
}

/// The package manager a project installs dependencies with.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    /// `pnpm`.
    Pnpm,
    /// `yarn`.
    Yarn,
    /// `npm`.
    Npm,
    /// `bun`.
    Bun,
    /// `cargo`.
    Cargo,
    /// `uv`.
    Uv,
    /// `poetry`.
    Poetry,
    /// `pip`, which installs from a `requirements.txt`.
    ///
    /// The one manager here that writes no lockfile. A repository whose Python half is a
    /// `requirements.txt` and nothing else had no value to name, so a base could install
    /// no part of it and a warm build that needed the interpreter failed. What `pip`
    /// installs from is committed and is the file CI installs from, which is the
    /// evidence every other value here is chosen on.
    Pip,
}

/// The dependency tree a package manager writes.
///
/// Two managers of one ecosystem write the same tree, so a project installs with at
/// most one of them, and every question of the form "which half of this repository is
/// this manager about" is this one answer. It is not a recipe key: nothing writes it
/// down, because a manager already says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ecosystem {
    /// `node_modules`.
    Node,
    /// The Cargo registry cache.
    Rust,
    /// A virtual environment.
    Python,
}

impl Ecosystem {
    /// Every ecosystem, so that a reader can ask about the ones a recipe left out.
    pub const ALL: &'static [Self] = &[Self::Node, Self::Rust, Self::Python];

    /// The committed files that say a repository has this ecosystem in it.
    ///
    /// A manifest and not a lockfile: the question these answer is "is there a half of
    /// this repository here", which a repository answers whether or not it pins its
    /// dependencies. A table, because it is a list of names and nothing else.
    #[must_use]
    pub const fn manifests(self) -> &'static [&'static str] {
        match self {
            Self::Node => &["package.json"],
            Self::Rust => &["Cargo.toml"],
            Self::Python => &["pyproject.toml", "requirements.txt", "setup.py"],
        }
    }

    /// What this ecosystem is called in a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Rust => "rust",
            Self::Python => "python",
        }
    }
}

impl PackageManager {
    /// Which dependency tree this manager writes.
    #[must_use]
    pub const fn ecosystem(self) -> Ecosystem {
        match self {
            Self::Pnpm | Self::Yarn | Self::Npm | Self::Bun => Ecosystem::Node,
            Self::Cargo => Ecosystem::Rust,
            Self::Uv | Self::Poetry | Self::Pip => Ecosystem::Python,
        }
    }

    /// Whether this manager runs the scripts a `package.json` declares.
    ///
    /// A repository with a Node manager and a Cargo manager has two script vocabularies
    /// in it, and a name out of `package.json` has to be run by the manager that reads
    /// that file.
    #[must_use]
    pub const fn runs_package_json_scripts(self) -> bool {
        matches!(self.ecosystem(), Ecosystem::Node)
    }

    /// The binary this package manager is invoked as.
    #[must_use]
    pub fn program(self) -> &'static str {
        match self {
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Npm => "npm",
            Self::Bun => "bun",
            Self::Cargo => "cargo",
            Self::Uv => "uv",
            Self::Poetry => "poetry",
            Self::Pip => "pip",
        }
    }
}

/// Which of a recipe's `[hooks]` commands this is.
///
/// The keys of that table, as a type. It is here and not with the module that runs the
/// hooks, because it is what a recipe writes: `Hooks` holds the six commands and this
/// names them, so the table and its keys are one piece of plain data with no IO
/// (`docs/code-structure.md`). `crate::lifecycle::hooks` re-exports it, because that is
/// the module that acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Before a unit is created.
    PreNew,
    /// After a unit is created.
    PostNew,
    /// Before a unit is merged.
    PreMerge,
    /// After a unit is merged, and before it is removed.
    PostMerge,
    /// Before a unit is reclaimed.
    PreReclaim,
    /// After a unit is reclaimed.
    PostReclaim,
}

/// Every phase, in the order they are declared and approved.
pub const PHASES: &[Phase] = &[
    Phase::PreNew,
    Phase::PostNew,
    Phase::PreMerge,
    Phase::PostMerge,
    Phase::PreReclaim,
    Phase::PostReclaim,
];

impl Phase {
    /// The key this phase has in `nodal.toml` and in the approvals file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PreNew => "pre_new",
            Self::PostNew => "post_new",
            Self::PreMerge => "pre_merge",
            Self::PostMerge => "post_merge",
            Self::PreReclaim => "pre_reclaim",
            Self::PostReclaim => "post_reclaim",
        }
    }

    /// The command a recipe declares for this phase, when it declares one.
    #[must_use]
    pub fn command(self, hooks: &Hooks) -> Option<&CommandLine> {
        match self {
            Self::PreNew => hooks.pre_new.as_ref(),
            Self::PostNew => hooks.post_new.as_ref(),
            Self::PreMerge => hooks.pre_merge.as_ref(),
            Self::PostMerge => hooks.post_merge.as_ref(),
            Self::PreReclaim => hooks.pre_reclaim.as_ref(),
            Self::PostReclaim => hooks.post_reclaim.as_ref(),
        }
    }
}

impl core::fmt::Display for Phase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.key())
    }
}

/// Read the `package_manager` key in either spelling a recipe may write it in.
///
/// Reading only. A list writes itself, so the key always comes out as a list and the
/// two spellings exist on the way in and nowhere else.
///
/// **Why a visitor and not an untagged enum.** An untagged enum answers a value it
/// cannot read with the name of its own type: a word this list does not hold made `data
/// did not match any variant of untagged enum OneOrMany`, which names neither the key,
/// nor the word that was wrong, nor the words that are right. A person reading that has
/// to read this program's source to fix their own file.
///
/// The visitor below reads the two shapes and hands each word to
/// [`PackageManager`]'s own reader, which holds the list of managers already. So the
/// failure names the line, the column, the word and every word that is accepted, and
/// the list of managers is written once: in the enum.
mod written {
    use std::fmt;

    use serde::Deserialize;
    use serde::de::{Deserializer, Error, IntoDeserializer, SeqAccess, Visitor};

    use super::PackageManager;

    /// The two shapes a recipe file may hold: one word, or a list of them.
    struct OneOrMany;

    impl<'de> Visitor<'de> for OneOrMany {
        type Value = Vec<PackageManager>;

        fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(out, "one package manager, or a list of them")
        }

        fn visit_str<E: Error>(self, word: &str) -> Result<Self::Value, E> {
            Ok(vec![PackageManager::deserialize(word.into_deserializer())?])
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut managers = Vec::with_capacity(seq.size_hint().unwrap_or_default());
            while let Some(manager) = seq.next_element()? {
                managers.push(manager);
            }
            Ok(managers)
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        input: D,
    ) -> Result<Vec<PackageManager>, D::Error> {
        input.deserialize_any(OneOrMany)
    }

    /// What the published schema says the key accepts.
    ///
    /// Written here rather than derived from the reading type above, so that the schema
    /// describes the shape a recipe file may hold and names nothing of this program's
    /// own.
    pub(super) fn schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let manager = generator.subschema_for::<PackageManager>();
        let value = serde_json::json!({
            "description": "One package manager, or several with the primary first.",
            "anyOf": [manager, { "type": "array", "items": manager }],
        });
        schemars::Schema::try_from(value).unwrap_or_default()
    }
}

/// The task runner that caches build output across units, when a project has one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskCache {
    /// Turborepo.
    Turborepo,
    /// Nx.
    Nx,
}

/// The tool that owns a project's migrations.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTool {
    /// The Supabase CLI.
    Supabase,
    /// Prisma Migrate.
    Prisma,
    /// Drizzle Kit.
    Drizzle,
    /// Alembic.
    Alembic,
    /// A migrations directory whose tool could not be named.
    Unknown,
}

/// What kind of database the project develops against.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum DbKind {
    /// A local Supabase stack, with its own fixed ports.
    SupabaseLocal,
    /// A Postgres server the project connects to directly.
    Postgres,
}

/// The commands a project is driven by. Every one is optional: a project that has no
/// test command is a project Nodal still manages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Commands {
    /// Start the development server.
    pub dev: Option<CommandLine>,
    /// Build the project.
    pub build: Option<CommandLine>,
    /// Run the test suite.
    pub test: Option<CommandLine>,
    /// Lint.
    pub lint: Option<CommandLine>,
    /// Type-check.
    pub typecheck: Option<CommandLine>,
    /// Apply outstanding migrations.
    pub migrate: Option<CommandLine>,
    /// Load seed data.
    pub seed: Option<CommandLine>,
    /// Drop and rebuild the database from migrations and seed.
    pub reset: Option<CommandLine>,
}

/// The database a project develops against, and how its schema is applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Db {
    /// What the database is.
    pub kind: Option<DbKind>,
    /// The tool that owns the migrations.
    pub tool: Option<MigrationTool>,
    /// The migrations directory, relative to the project root.
    pub migrations_dir: Option<PathBuf>,
    /// The variables a connection string is published in, in the order tried.
    pub url_var: Vec<EnvName>,
    /// Ports the stack pins, by the name its own configuration gives them. A pinned
    /// port can be held by only one environment at a time.
    pub fixed_ports: BTreeMap<PortName, u16>,
}

/// Which of a project's services are shared between units and which each unit gets its
/// own copy of.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Services {
    /// Services one instance of which serves every unit.
    pub shared: Vec<ServiceName>,
    /// Services each unit gets its own instance of.
    pub per_unit: Vec<ServiceName>,
}

/// The environment variables a unit's home needs.
///
/// A name belongs to exactly one of these lists. `generated` is what Nodal writes per
/// unit; `secrets` is what a secret source supplies and what is never written into a
/// manifest, bundle or log; `required_local` is what a person had to say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Env {
    /// Names local development needs that Nodal cannot work out on its own.
    pub required_local: Vec<EnvName>,
    /// Names Nodal generates for each unit.
    pub generated: Vec<EnvName>,
    /// Names whose values come from a secret source.
    pub secrets: Vec<EnvName>,
    /// The stand-in a generated name takes when no adapter answers it.
    ///
    /// The value is a template. Nodal fills `{slug}` with the unit's handle and
    /// `{port}` with a port derived from the project's block and that handle. A name
    /// with no entry here takes the stand-in its own shape asks for
    /// ([`crate::env::stand_in`]). A name outside `generated` is ignored.
    pub stand_in: BTreeMap<EnvName, String>,
}

/// What a base clone leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BaseSpec {
    /// Paths, relative to the project root, that no unit home receives.
    pub exclude: Vec<PathBuf>,
    /// Paths whose content records the directory it was made in, so that a copy of it
    /// at a new path is removed rather than trusted. A path is matched wherever it
    /// sits under a home, not only at the root, because a repository of more than one
    /// package keeps one such directory under each of them.
    pub invalidate: Vec<PathBuf>,
}

/// Commands the project runs around lifecycle operations. Each receives the `NODAL_*`
/// context variables, runs in the directory `docs/contracts.md` gives for its phase, and
/// may name the template variables that document lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    /// Before a unit is created.
    pub pre_new: Option<CommandLine>,
    /// After a unit is created.
    pub post_new: Option<CommandLine>,
    /// Before a unit is merged.
    pub pre_merge: Option<CommandLine>,
    /// After a unit is merged, and before it is removed.
    pub post_merge: Option<CommandLine>,
    /// Before a unit is reclaimed.
    pub pre_reclaim: Option<CommandLine>,
    /// After a unit is reclaimed.
    pub post_reclaim: Option<CommandLine>,
}

/// How much of a sync runs without being asked twice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Sync {
    /// Whether irreversible steps run without `--apply`. Off unless a project says so.
    pub auto_irreversible: Option<bool>,
}

/// How long reclaimed homes stay recoverable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Reclaim {
    /// Days a trashed home is kept before `nodal gc` may delete it.
    pub trash_retention: Option<u32>,
}

/// How long a unit's write lock survives with nobody entering its home.
///
/// Named for the policy rather than the thing, because [`crate::model::Lock`] is the
/// hold itself and a recipe holds no locks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LockPolicy {
    /// Hours with no entry after which the lock lapses and anybody may take it.
    pub idle_hours: Option<u32>,
}

/// A project's recipe: `nodal.toml`, the inference of it, or the merge of both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    /// Where commands run.
    pub backend: Option<Backend>,
    /// Every package manager the project installs with, the primary first.
    ///
    /// Written as one name or as a list of them. A repository of more than one
    /// ecosystem has more than one, and a base installs each of them in this order.
    /// The first is the primary: the one a bare script name resolves against.
    #[serde(deserialize_with = "written::deserialize")]
    #[schemars(schema_with = "written::schema")]
    pub package_manager: Vec<PackageManager>,
    /// The exact package-manager version the project pins, as written in its manifest.
    pub package_manager_pin: Option<ToolVersion>,
    /// Whether the repository holds more than one package.
    pub monorepo: Option<bool>,
    /// The cross-unit task cache, when there is one.
    pub task_cache: Option<TaskCache>,
    /// The image definition, when the project has one.
    pub dockerfile: Option<PathBuf>,
    /// Compose files the project defines services in.
    pub compose: Vec<PathBuf>,
    /// Tool versions the project pins, by tool.
    pub toolchain: BTreeMap<ToolName, ToolVersion>,
    /// The commands the project is driven by.
    pub commands: Commands,
    /// The database, when the project has one.
    pub db: Db,
    /// Service topology.
    pub services: Services,
    /// Environment variables.
    pub env: Env,
    /// What a base clone leaves out.
    pub base: BaseSpec,
    /// Lifecycle hooks.
    pub hooks: Hooks,
    /// Sync behaviour.
    pub sync: Sync,
    /// Reclaim behaviour.
    pub reclaim: Reclaim,
    /// How long a write lock survives idle.
    pub lock: LockPolicy,
}

/// Days a trashed home is kept when a recipe does not say.
pub const DEFAULT_TRASH_RETENTION_DAYS: u32 = 14;

impl Recipe {
    /// Where commands run; [`Backend::Native`] unless the recipe says otherwise.
    #[must_use]
    pub fn backend(&self) -> Backend {
        self.backend.unwrap_or(Backend::Native)
    }

    /// The manager that runs what a `package.json` declares, when the project has one.
    #[must_use]
    pub fn script_manager(&self) -> Option<PackageManager> {
        self.package_manager.iter().copied().find(|m| m.runs_package_json_scripts())
    }

    /// Whether the repository holds more than one package.
    #[must_use]
    pub fn monorepo(&self) -> bool {
        self.monorepo.unwrap_or(false)
    }

    /// Whether irreversible sync steps run without `--apply`.
    #[must_use]
    pub fn auto_irreversible(&self) -> bool {
        self.sync.auto_irreversible.unwrap_or(false)
    }

    /// Hours a write lock survives with nothing entering the home.
    #[must_use]
    pub fn lock_idle_hours(&self) -> u32 {
        self.lock.idle_hours.unwrap_or(crate::model::lock::DEFAULT_IDLE_HOURS)
    }

    /// Days a trashed home is kept before `nodal gc` may delete it.
    #[must_use]
    pub fn trash_retention_days(&self) -> u32 {
        self.reclaim.trash_retention.unwrap_or(DEFAULT_TRASH_RETENTION_DAYS)
    }
}
