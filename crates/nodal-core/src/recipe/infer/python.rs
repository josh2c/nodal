//! The commands a Python tree is driven by.
//!
//! Python states nothing by convention. There is no script table every project has and
//! no test runner every project uses, so every command here needs the project to have
//! named the tool: `pytest` among the declared dependencies, and a `[tool.<name>]`
//! section for a linter or a type checker. A project that has not named one gets no
//! command rather than a command that fails the first time it runs.
//!
//! Each command runs through the manager the lockfile named, because that is what puts
//! the environment the tool is installed in on the path.

use crate::model::recipe::{CommandLine, Ecosystem, PackageManager, Recipe};
use crate::recipe::infer::scripts::SetCommand;
use crate::recipe::infer::{Confidence, Project, Proposal};

/// The manifest a Python project declares itself in.
const MANIFEST: &str = "pyproject.toml";

/// The test runner, and the fields a project may declare it in.
const PYTEST: &str = "pytest";

/// A tool a `[tool.<name>]` section names, the recipe key and field it fills, and the
/// words it is run with. A section is the project saying it configured that tool, which
/// is the evidence this source needs. The order is the order two tools of one kind are
/// preferred in.
const CONFIGURED: &[(&str, &str, SetCommand, &str)] = &[
    ("ruff", "lint", |recipe, line| recipe.commands.lint = Some(line), "ruff check"),
    ("mypy", "typecheck", |recipe, line| recipe.commands.typecheck = Some(line), "mypy"),
    ("pyright", "typecheck", |recipe, line| recipe.commands.typecheck = Some(line), "pyright"),
];

/// Propose the commands a Python tree states.
#[must_use]
pub fn infer(project: &Project, so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    let Some(manager) = python_manager(so_far) else { return proposal };
    let Some(manifest) = project.read_toml(MANIFEST) else { return proposal };
    let run = |what: &str| CommandLine::parse(format!("{} run {what}", manager.program())).ok();

    if let Some(line) = declares(&manifest, PYTEST).then(|| run(PYTEST)).flatten() {
        proposal.recipe.commands.test = Some(line);
        proposal = proposal.sure("commands.test", Confidence::High);
    }
    for (tool, key, set, invocation) in CONFIGURED {
        // The first tool of a kind wins, so a project configuring both mypy and pyright
        // gets the one this table prefers rather than the one it happens to reach last.
        if proposal.confidence.contains_key(&format!("commands.{key}")) {
            continue;
        }
        if manifest.get("tool").and_then(|table| table.get(tool)).is_none() {
            continue;
        }
        let Some(line) = run(invocation) else { continue };
        set(&mut proposal.recipe, line);
        proposal = proposal.sure(&format!("commands.{key}"), Confidence::High);
    }
    proposal
}

/// The Python manager the recipe named, when it named one.
fn python_manager(recipe: &Recipe) -> Option<PackageManager> {
    recipe.package_manager.iter().copied().find(|m| m.ecosystem() == Ecosystem::Python)
}

/// Whether the manifest declares `name` as a dependency, in any of the places the
/// packaging standards and Poetry put one.
///
/// A declaration is a requirement string in a list, or a key in a table of constraints,
/// so both forms are searched for the distribution name at the start of the entry.
fn declares(manifest: &toml::Value, name: &str) -> bool {
    const LISTS: &[&[&str]] = &[
        &["project", "dependencies"],
        &["project", "optional-dependencies"],
        &["dependency-groups"],
        &["tool", "uv", "dev-dependencies"],
    ];
    const TABLES: &[&[&str]] = &[
        &["tool", "poetry", "dependencies"],
        &["tool", "poetry", "dev-dependencies"],
        &["tool", "poetry", "group"],
    ];
    LISTS.iter().any(|path| at(manifest, path).is_some_and(|value| requires(value, name)))
        || TABLES.iter().any(|path| at(manifest, path).is_some_and(|value| keyed(value, name)))
}

/// The value at a path of table keys.
fn at<'a>(manifest: &'a toml::Value, path: &[&str]) -> Option<&'a toml::Value> {
    path.iter().try_fold(manifest, |table, key| table.get(key))
}

/// Whether `value` is, or holds, a requirement list naming `name`.
///
/// One level of nesting is followed, because the standards put the lists of an extra or
/// a group inside a table keyed by that extra's or group's own name.
fn requires(value: &toml::Value, name: &str) -> bool {
    if let Some(list) = value.as_array() {
        return list.iter().filter_map(toml::Value::as_str).any(|entry| names(entry, name));
    }
    value.as_table().is_some_and(|table| table.values().any(|inner| requires(inner, name)))
}

/// Whether `value` is, or holds, a table of constraints keyed by `name`.
fn keyed(value: &toml::Value, name: &str) -> bool {
    let Some(table) = value.as_table() else { return false };
    table.contains_key(name)
        || table
            .values()
            .any(|inner| inner.get("dependencies").is_some_and(|deps| keyed(deps, name)))
}

/// Whether a requirement string asks for `name`.
///
/// The distribution name is what the entry starts with, up to the first character a
/// version, an extra or an environment marker begins with.
fn names(entry: &str, name: &str) -> bool {
    let head = entry.trim().split(['[', '<', '>', '=', '!', '~', ';', ' ']).next().unwrap_or("");
    head.eq_ignore_ascii_case(name)
}
