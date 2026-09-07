//! Write a recipe out as the `nodal.toml` a person will edit.
//!
//! The file is rendered rather than serialised so that each gap can sit directly above
//! the empty key it belongs to. That placement is the point of the whole module: a
//! recipe with three gaps should read as a three-line to-do list, in a file where every
//! other line is already correct.
//!
//! What is rendered round-trips: [`crate::recipe::parse`] reads it back to the same
//! [`Recipe`], which is the property `render_round_trips` holds it to.

use std::fmt::Write as _;

use serde::Serialize;

use crate::model::recipe::{DEFAULT_TRASH_RETENTION_DAYS, Recipe};
use crate::recipe::gap::Gap;

/// The comment at the top of every generated file.
const HEADER: &str = "\
# nodal.toml — what this project needs to be a working copy.
#
# Written by `nodal init` from the project's own files. Lines marked GAP are the ones
# it could not read anywhere: answer those. Everything else is a confirmation, so
# correct what is wrong and leave what is right.
";

/// Render `recipe` with `gaps` written above the keys they are about.
#[must_use]
pub fn render(recipe: &Recipe, gaps: &[Gap]) -> String {
    let mut out = String::from(HEADER);
    top_level(&mut out, recipe);
    toolchain(&mut out, recipe, gaps);
    commands(&mut out, recipe);
    db(&mut out, recipe, gaps);
    services(&mut out, recipe, gaps);
    env(&mut out, recipe, gaps);
    base(&mut out, recipe);
    policy(&mut out, recipe);
    out
}

fn top_level(out: &mut String, recipe: &Recipe) {
    out.push('\n');
    key_enum(out, "backend", &recipe.backend());
    if let Some(manager) = recipe.package_manager {
        key_enum(out, "package_manager", &manager);
    }
    if let Some(pin) = &recipe.package_manager_pin {
        key_string(out, "package_manager_pin", pin.as_str());
    }
    key_bool(out, "monorepo", recipe.monorepo());
    if let Some(cache) = recipe.task_cache {
        key_enum(out, "task_cache", &cache);
    }
    if let Some(dockerfile) = &recipe.dockerfile {
        key_string(out, "dockerfile", &dockerfile.display().to_string());
    }
    if !recipe.compose.is_empty() {
        key_paths(out, "compose", &recipe.compose);
    }
}

fn toolchain(out: &mut String, recipe: &Recipe, gaps: &[Gap]) {
    section(out, "toolchain");
    write_gaps(out, gaps, "toolchain");
    for (tool, version) in &recipe.toolchain {
        key_string(out, tool.as_str(), version.as_str());
    }
}

fn commands(out: &mut String, recipe: &Recipe) {
    let table = [
        ("dev", &recipe.commands.dev),
        ("build", &recipe.commands.build),
        ("test", &recipe.commands.test),
        ("lint", &recipe.commands.lint),
        ("typecheck", &recipe.commands.typecheck),
        ("migrate", &recipe.commands.migrate),
        ("seed", &recipe.commands.seed),
        ("reset", &recipe.commands.reset),
    ];
    section(out, "commands");
    for (name, line) in table {
        if let Some(line) = line {
            key_string(out, name, line.as_str());
        }
    }
}

fn db(out: &mut String, recipe: &Recipe, gaps: &[Gap]) {
    section(out, "db");
    write_gaps(out, gaps, "db.migrations_dir");
    if let Some(kind) = recipe.db.kind {
        key_enum(out, "kind", &kind);
    }
    if let Some(tool) = recipe.db.tool {
        key_enum(out, "tool", &tool);
    }
    if let Some(directory) = &recipe.db.migrations_dir {
        key_string(out, "migrations_dir", &directory.display().to_string());
    }
    if !recipe.db.url_var.is_empty() {
        key_strings(out, "url_var", recipe.db.url_var.iter().map(ToString::to_string));
    }
    if recipe.db.fixed_ports.is_empty() {
        return;
    }
    section(out, "db.fixed_ports");
    for (name, port) in &recipe.db.fixed_ports {
        let _ = writeln!(out, "{} = {port}", quote_key(name.as_str()));
    }
}

fn services(out: &mut String, recipe: &Recipe, gaps: &[Gap]) {
    section(out, "services");
    write_gaps(out, gaps, "services");
    key_strings(out, "shared", recipe.services.shared.iter().map(ToString::to_string));
    key_strings(out, "per_unit", recipe.services.per_unit.iter().map(ToString::to_string));
}

fn env(out: &mut String, recipe: &Recipe, gaps: &[Gap]) {
    section(out, "env");
    write_gaps(out, gaps, "env.required_local");
    key_strings(out, "required_local", recipe.env.required_local.iter().map(ToString::to_string));
    key_strings(out, "generated", recipe.env.generated.iter().map(ToString::to_string));
    key_strings(out, "secrets", recipe.env.secrets.iter().map(ToString::to_string));
}

fn base(out: &mut String, recipe: &Recipe) {
    section(out, "base");
    out.push_str("# Paths no unit home receives. Regenerated output belongs here.\n");
    key_paths(out, "exclude", &recipe.base.exclude);
    out.push_str("# Paths a home removes because their content records the path it was made at.\n");
    key_paths(out, "invalidate", &recipe.base.invalidate);
}

/// The keys that are policy rather than fact. Anything unset is written as the default
/// it would take, commented out, so the file states its own behaviour.
fn policy(out: &mut String, recipe: &Recipe) {
    let hooks = [
        ("pre_new", &recipe.hooks.pre_new),
        ("post_new", &recipe.hooks.post_new),
        ("pre_reclaim", &recipe.hooks.pre_reclaim),
        ("post_reclaim", &recipe.hooks.post_reclaim),
    ];
    section(out, "hooks");
    out.push_str("# Run in the unit's home, with the NODAL_* variables set.\n");
    for (name, line) in hooks {
        match line {
            Some(line) => key_string(out, name, line.as_str()),
            None => {
                let _ = writeln!(out, "# {name} = \"\"");
            }
        }
    }

    section(out, "sync");
    match recipe.sync.auto_irreversible {
        Some(value) => key_bool(out, "auto_irreversible", value),
        None => out.push_str("# auto_irreversible = false\n"),
    }

    section(out, "reclaim");
    match recipe.reclaim.trash_retention {
        Some(days) => {
            let _ = writeln!(out, "trash_retention = {days}");
        }
        None => {
            let _ = writeln!(out, "# trash_retention = {DEFAULT_TRASH_RETENTION_DAYS}");
        }
    }
}

/// Write every gap about `key` as the comment block above it.
fn write_gaps(out: &mut String, gaps: &[Gap], key: &str) {
    for gap in gaps.iter().filter(|gap| gap.key.toml_key() == key) {
        let _ = writeln!(out, "# GAP: {}", gap.key.question());
        if let Some(note) = &gap.note {
            let _ = writeln!(out, "# {note}.");
        }
        for candidate in &gap.candidates {
            let _ = writeln!(out, "#   {candidate}");
        }
    }
}

fn section(out: &mut String, name: &str) {
    let _ = writeln!(out, "\n[{name}]");
}

fn key_string(out: &mut String, key: &str, value: &str) {
    let _ = writeln!(out, "{} = {}", quote_key(key), quote(value));
}

fn key_bool(out: &mut String, key: &str, value: bool) {
    let _ = writeln!(out, "{key} = {value}");
}

fn key_enum<T: Serialize>(out: &mut String, key: &str, value: &T) {
    if let Some(text) = token(value) {
        key_string(out, key, &text);
    }
}

fn key_paths(out: &mut String, key: &str, paths: &[std::path::PathBuf]) {
    key_strings(out, key, paths.iter().map(|path| path.display().to_string()));
}

fn key_strings(out: &mut String, key: &str, values: impl Iterator<Item = String>) {
    let items: Vec<String> = values.map(|value| quote(&value)).collect();
    if items.len() > 3 {
        let _ = writeln!(out, "{key} = [\n  {}\n]", items.join(",\n  "));
    } else {
        let _ = writeln!(out, "{key} = [{}]", items.join(", "));
    }
}

/// The string a `serde` enum serialises to, which is the word the schema publishes.
fn token<T: Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Some(text),
        _ => None,
    }
}

/// A TOML basic string.
fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 || other as u32 == 0x7f => {
                let _ = write!(out, "\\u{:04X}", other as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A key, bare where TOML allows it and quoted where it does not.
fn quote_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if bare { key.to_owned() } else { quote(key) }
}
