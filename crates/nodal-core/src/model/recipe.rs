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
}

impl PackageManager {
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
        }
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
}

/// What a base clone leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BaseSpec {
    /// Paths, relative to the project root, that no unit home receives.
    pub exclude: Vec<PathBuf>,
}

/// Commands the project runs around lifecycle operations. They receive the `NODAL_*`
/// context variables and run in the unit's home.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    /// Before a unit is created.
    pub pre_new: Option<CommandLine>,
    /// After a unit is created.
    pub post_new: Option<CommandLine>,
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

/// A project's recipe: `nodal.toml`, the inference of it, or the merge of both.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    /// Where commands run.
    pub backend: Option<Backend>,
    /// The package manager.
    pub package_manager: Option<PackageManager>,
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
}

/// Days a trashed home is kept when a recipe does not say.
pub const DEFAULT_TRASH_RETENTION_DAYS: u32 = 14;

impl Recipe {
    /// Where commands run; [`Backend::Native`] unless the recipe says otherwise.
    #[must_use]
    pub fn backend(&self) -> Backend {
        self.backend.unwrap_or(Backend::Native)
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

    /// Days a trashed home is kept before `nodal gc` may delete it.
    #[must_use]
    pub fn trash_retention_days(&self) -> u32 {
        self.reclaim.trash_retention.unwrap_or(DEFAULT_TRASH_RETENTION_DAYS)
    }
}
