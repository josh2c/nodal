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
//!
//! # The tether
//!
//! `nodal run --tether` starts the command in a process group of its own and writes
//! that group into the registry as a session ([`crate::model::Session`]). The group
//! then belongs to the unit: `nodal reclaim` stops the whole group, and `nodal gc`
//! stops one that outlived a unit already reclaimed.
//!
//! **The row is the record, not this process.** A development server replaces its own
//! leader, starts a compiler that starts a watcher, and keeps running after the
//! `nodal run` that started it has been killed. None of those is a thing to hold a
//! record in. A row in the registry is, and it is what a reclaim reads.
//!
//! So a tether is refused in a home the registry does not know. Every other run carries
//! on there and loses only its event, but a tethered group nothing recorded is a group
//! nothing can ever stop, and starting one would be the opposite of what the flag is
//! for.
//!
//! The row is written as soon as the command has a process identifier, which is the
//! smallest window there is: a `nodal run` killed inside it leaves a group with no row.
//! That group still carries `NODAL_ID`, so attribution still finds it and a reclaim
//! still stops it — one signal weaker, and never nothing.
//!
//! A tethered command is given no terminal input. A process in a group of its own is
//! not the terminal's foreground group, so a read from the terminal would stop it
//! rather than answer it. End of file is the honest answer, and a tethered command is a
//! service rather than a conversation.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use rusqlite::Connection;

use crate::env::files;
use crate::model::manifest::Origin;
use crate::model::{
    EnvId, Epistemic, Event, EventId, EventKind, Manifest, RefName, Session, SessionId, Timestamp,
};
use crate::runtime::actor;
use crate::runtime::stop::{self, Signals as _, Target};
use crate::store::{environments, events, sessions, units};
use crate::{Error, Result};

/// The reference naming what the command exited with.
pub const EXIT_CODE: &str = "exit_code";

/// The reference naming how long it ran, in milliseconds.
pub const DURATION_MS: &str = "duration_ms";

/// How a command is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// In this process's own process group, as any other command a shell starts.
    #[default]
    Plain,
    /// In a process group of its own, recorded as the unit's tether.
    Tether,
}

/// What one run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The exit code, or `None` when a signal ended the command.
    pub code: Option<i32>,
    /// Whether the run was recorded in the unit's log.
    pub recorded: bool,
    /// The process group a tethered run left still running. `None` when the run was not
    /// tethered, or when the whole group had gone by the time the command exited.
    pub tether: Option<u32>,
}

/// Run `argv` in `home`'s environment, from the directory `cwd`.
///
/// # Errors
/// [`Error::Io`] when the home cannot be read or the command cannot be started,
/// [`Error::InvalidValue`] when the command is empty, and [`Error::TetherNotRecorded`]
/// when a tether was asked for in a home the registry does not know.
pub fn execute(
    home: &Path,
    cwd: &Path,
    argv: &[String],
    conn: Option<&Connection>,
    mode: Mode,
) -> Result<Ran> {
    let Some((program, arguments)) = argv.split_first() else {
        return Err(Error::InvalidValue { kind: "command", value: String::new() });
    };
    let manifest = files::read_manifest(home)?;
    let pairs = crate::env::entering(home)?;
    let values: BTreeMap<String, String> =
        pairs.iter().map(|(name, value)| (name.to_string(), value.clone())).collect();
    let tethered = tethered_to(conn, &manifest, home, mode)?;

    let started = std::time::Instant::now();
    let mut command = Command::new(program);
    command.args(arguments).envs(&values).current_dir(cwd);
    grouped(&mut command, mode);
    let mut child = command.spawn().map_err(Error::io(program))?;
    let tether = open(conn, tethered, &child)?;
    let status = child.wait().map_err(Error::io(program))?;
    let elapsed = started.elapsed();

    let body = redact(&argv.join(" "), &manifest, &values);
    let recorded = match conn {
        Some(conn) => record(conn, &manifest, &body, (status.code(), elapsed))?,
        None => false,
    };
    Ok(Ran { code: status.code(), recorded, tether: close(conn, tether) })
}

/// The environment a tether's row belongs to, and the refusal when there is none.
///
/// A tether is the one thing `nodal run` will not do in a home the registry has never
/// heard of, because the row is the only record of the group.
fn tethered_to(
    conn: Option<&Connection>,
    manifest: &Manifest,
    home: &Path,
    mode: Mode,
) -> Result<Option<EnvId>> {
    if mode == Mode::Plain {
        return Ok(None);
    }
    let known = match conn {
        Some(conn) => {
            units::get(conn, manifest.unit)?.is_some()
                && environments::get(conn, manifest.environment)?.is_some()
        }
        None => false,
    };
    if known {
        Ok(Some(manifest.environment))
    } else {
        Err(Error::TetherNotRecorded { home: home.to_path_buf() })
    }
}

/// Put a tethered command in a process group of its own, and give it no terminal input.
#[cfg(unix)]
fn grouped(command: &mut Command, mode: Mode) {
    use std::os::unix::process::CommandExt as _;

    match mode {
        Mode::Plain => command.stdin(Stdio::inherit()),
        // Zero means "a new group, whose identifier is this child's own identifier".
        Mode::Tether => command.stdin(Stdio::null()).process_group(0),
    };
}

/// A host with no process groups runs the command as any other.
#[cfg(not(unix))]
fn grouped(command: &mut Command, _mode: Mode) {
    command.stdin(Stdio::inherit());
}

/// Write the tether's row, as soon as the command has an identifier.
///
/// The group identifier is the child's own process identifier, which is what
/// [`grouped`] asked the system for.
fn open(
    conn: Option<&Connection>,
    environment: Option<EnvId>,
    child: &Child,
) -> Result<Option<(SessionId, u32)>> {
    let (Some(conn), Some(environment)) = (conn, environment) else { return Ok(None) };
    let session = Session {
        id: SessionId::from_ulid(ulid::Ulid::new()),
        environment_id: environment,
        actor: actor::current()?,
        pid: Some(child.id()),
        pgid: Some(child.id()),
        started_at: Timestamp::now(),
        ended_at: None,
    };
    sessions::insert(conn, &session)?;
    tracing::debug!(pgid = child.id(), "the unit holds a tether");
    Ok(Some((session.id, session.pgid.unwrap_or_default())))
}

/// Close the tether's row when its group is empty, and leave it open when it is not.
///
/// An open row is exactly the claim "this group is still the unit's to stop", and it is
/// the claim a reclaim acts on. A command that ends leaving background work behind
/// therefore leaves the row where a later `nodal reclaim` finds it.
fn close(conn: Option<&Connection>, tether: Option<(SessionId, u32)>) -> Option<u32> {
    let (Some(conn), Some((session, pgid))) = (conn, tether) else { return None };
    if stop::Live.alive(Target::Group(pgid)) {
        return Some(pgid);
    }
    if let Err(error) = sessions::end(conn, session, Timestamp::now()) {
        tracing::debug!(%error, "the tether's row was not closed");
    }
    None
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
