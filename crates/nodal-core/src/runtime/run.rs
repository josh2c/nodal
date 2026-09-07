//! `nodal run`: start a command inside a unit, and record that it was run.
//!
//! This is the one place that starts a command a person named, as `git::cmd` is the one
//! place that starts Git. The command runs in the home's environment and in the
//! directory the person is in; nothing is wrapped, nothing is intercepted, and the
//! command's own exit status is what `nodal run` exits with.
//!
//! The event it appends is `observed`: Nodal watched the command run. The body names
//! variables rather than values — every value the home carries is replaced by `$NAME`
//! before the body is written — so a credential that appeared on a command line does
//! not reach the log (`docs/contracts.md`).
//!
//! A home whose unit the registry does not know still runs the command. The event is
//! what is lost, not the work.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use rusqlite::Connection;

use crate::env::files;
use crate::model::manifest::Origin;
use crate::model::{Epistemic, Event, EventId, EventKind, Manifest, RefName, Timestamp};
use crate::runtime::actor;
use crate::store::{events, units};
use crate::{Error, Result};

/// The reference naming what the command exited with.
pub const EXIT_CODE: &str = "exit_code";

/// The reference naming how long it ran, in milliseconds.
pub const DURATION_MS: &str = "duration_ms";

/// What one run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The exit code, or `None` when a signal ended the command.
    pub code: Option<i32>,
    /// Whether the run was recorded in the unit's log.
    pub recorded: bool,
}

/// Run `argv` in `home`'s environment, from the directory `cwd`.
///
/// # Errors
/// [`Error::Io`] when the home cannot be read or the command cannot be started, and
/// [`Error::InvalidValue`] when the command is empty.
pub fn execute(home: &Path, cwd: &Path, argv: &[String], conn: Option<&Connection>) -> Result<Ran> {
    let Some((program, arguments)) = argv.split_first() else {
        return Err(Error::InvalidValue { kind: "command", value: String::new() });
    };
    let manifest = files::read_manifest(home)?;
    let pairs = files::read_dotenv(home)?;
    let values: BTreeMap<String, String> =
        pairs.iter().map(|(name, value)| (name.to_string(), value.clone())).collect();

    let started = std::time::Instant::now();
    let status = Command::new(program)
        .args(arguments)
        .envs(&values)
        .current_dir(cwd)
        .stdin(Stdio::inherit())
        .status()
        .map_err(Error::io(program))?;
    let elapsed = started.elapsed();

    let body = redact(&argv.join(" "), &manifest, &values);
    let recorded = match conn {
        Some(conn) => record(conn, &manifest, &body, (status.code(), elapsed))?,
        None => false,
    };
    Ok(Ran { code: status.code(), recorded })
}

/// Replace every value the home carries with the name that holds it.
///
/// Identity values are left alone: `NODAL_ID` and the home path are what makes a
/// command line readable later, and neither is a credential.
#[must_use]
pub fn redact(text: &str, manifest: &Manifest, values: &BTreeMap<String, String>) -> String {
    let mut body = text.to_owned();
    for (name, origin) in &manifest.env {
        if *origin == Origin::Identity {
            continue;
        }
        let Some(value) = values.get(name.as_str()).filter(|value| !value.is_empty()) else {
            continue;
        };
        body = body.replace(value.as_str(), &format!("${name}"));
    }
    body
}

/// Append the command event, when the registry knows the unit.
fn record(
    conn: &Connection,
    manifest: &Manifest,
    body: &str,
    outcome: (Option<i32>, std::time::Duration),
) -> Result<bool> {
    if units::get(conn, manifest.unit)?.is_none() {
        tracing::debug!(unit = %manifest.unit, "the registry has no such unit; the run is not recorded");
        return Ok(false);
    }
    let (code, elapsed) = outcome;
    let mut refs = BTreeMap::new();
    if let Ok(name) = RefName::parse(EXIT_CODE) {
        refs.insert(name, code.map_or_else(|| String::from("signal"), |code| code.to_string()));
    }
    if let Ok(name) = RefName::parse(DURATION_MS) {
        refs.insert(name, elapsed.as_millis().to_string());
    }
    events::append(
        conn,
        &Event {
            id: EventId::from_ulid(ulid::Ulid::new()),
            unit: manifest.unit,
            environment: Some(manifest.environment),
            ts: Timestamp::now(),
            actor: actor::current()?,
            kind: EventKind::Command,
            epistemic: Epistemic::Observed,
            body: body.to_owned(),
            refs,
            raw_ref: None,
        },
    )?;
    Ok(true)
}
