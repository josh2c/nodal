//! The database: where its migrations live, what applies them, and what it pins.
//!
//! The migrations directory names the tool, and the tool names the command it would run
//! by itself. A project that wraps that command in a script wins over the tool's own
//! form, because the script is what the project's people actually type, and it is the
//! one that carries the project's flags.

use std::collections::BTreeMap;

use crate::model::environment::PortName;
use crate::model::recipe::{CommandLine, DbKind, MigrationTool, Recipe};
use crate::recipe::gap::{Gap, GapKey};
use crate::recipe::infer::package_manager::run_script;
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Migrations directories, the tool each belongs to, and that tool's own migrate
/// command. `None` where the directory is a convention several tools share.
const LAYOUTS: &[(&str, MigrationTool, Option<&str>)] = &[
    ("supabase/migrations", MigrationTool::Supabase, Some("supabase db push")),
    ("prisma/migrations", MigrationTool::Prisma, Some("prisma migrate deploy")),
    ("drizzle", MigrationTool::Drizzle, Some("drizzle-kit migrate")),
    ("alembic", MigrationTool::Alembic, Some("alembic upgrade head")),
    ("migrations", MigrationTool::Unknown, None),
];

/// Where one inferred command line is written on the recipe.
type SetCommand = fn(&mut Recipe, CommandLine);

/// Script names that override the tool's own command, and the field each fills.
const SCRIPTS: &[(&str, SetCommand)] = &[
    ("db:migrate", |recipe, line| recipe.commands.migrate = Some(line)),
    ("db:reset", |recipe, line| recipe.commands.reset = Some(line)),
    ("db:seed", |recipe, line| recipe.commands.seed = Some(line)),
    ("migrate", |recipe, line| recipe.commands.migrate = Some(line)),
    ("seed", |recipe, line| recipe.commands.seed = Some(line)),
];

/// The Supabase local stack's configuration, which pins the ports it listens on.
const SUPABASE_CONFIG: &str = "supabase/config.toml";

/// The variables a Supabase project publishes its connection string in, in the order
/// a client should try them.
const SUPABASE_URL_VARS: &[&str] = &["SUPABASE_DB_URL", "DATABASE_URL"];

/// Propose `db.*` and the migration commands, or raise [`GapKey::Db`].
#[must_use]
pub fn infer(project: &Project, so_far: &Recipe) -> Proposal {
    let Some((directory, tool, own_command)) =
        LAYOUTS.iter().find(|(directory, _, _)| project.exists(directory))
    else {
        return Proposal::default().gap(Gap::new(GapKey::Db));
    };

    let mut proposal = Proposal::default();
    proposal.recipe.db.tool = Some(*tool);
    proposal.recipe.db.migrations_dir = Some(directory.into());
    proposal.recipe.commands.migrate =
        own_command.and_then(|command| CommandLine::parse(command).ok());
    if proposal.recipe.commands.migrate.is_some() {
        proposal = proposal.sure("commands.migrate", Confidence::Medium);
    }
    proposal = from_scripts(project, so_far, proposal);
    supabase(project, proposal)
}

/// Let the project's own scripts override the tool's default commands.
fn from_scripts(project: &Project, so_far: &Recipe, mut proposal: Proposal) -> Proposal {
    let declared = project.scripts();
    for (name, set) in SCRIPTS {
        if !declared.contains_key(*name) {
            continue;
        }
        let Ok(line) = CommandLine::parse(run_script(so_far, name)) else { continue };
        set(&mut proposal.recipe, line);
        proposal.confidence.insert(String::from("commands.migrate"), Confidence::High);
    }
    proposal
}

/// Add what a local Supabase stack states about itself: its kind, its pinned ports and
/// where it publishes a connection string.
fn supabase(project: &Project, mut proposal: Proposal) -> Proposal {
    let Some(config) = project.read_toml(SUPABASE_CONFIG) else { return proposal };
    proposal.recipe.db.kind = Some(DbKind::SupabaseLocal);
    proposal.recipe.db.fixed_ports = pinned_ports(&config);
    proposal.recipe.db.url_var =
        SUPABASE_URL_VARS.iter().filter_map(|name| name.parse().ok()).collect();
    proposal.sure("db.url_var", Confidence::Medium).sure("db.fixed_ports", Confidence::High)
}

/// Every table in `config` that pins a port, by its dotted path.
fn pinned_ports(config: &toml::Value) -> BTreeMap<PortName, u16> {
    let mut ports = BTreeMap::new();
    collect_ports(config, &mut String::new(), &mut ports);
    ports
}

fn collect_ports(value: &toml::Value, path: &mut String, ports: &mut BTreeMap<PortName, u16>) {
    let Some(table) = value.as_table() else { return };
    if let Some(port) = table.get("port").and_then(toml::Value::as_integer)
        && let Ok(port) = u16::try_from(port)
        && let Ok(name) = path.parse::<PortName>()
    {
        ports.insert(name, port);
    }
    for (key, child) in table {
        let restore = path.len();
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(key);
        collect_ports(child, path, ports);
        path.truncate(restore);
    }
}
