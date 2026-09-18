//! The tools an agent may call, and the ones it may not.
//!
//! # One answer, not two
//!
//! Every tool here builds the value its own command builds and renders it the way
//! `--json` renders it ([`nodal_core::output::render`]). There is no second struct and
//! no second wording: a tool result is the bytes `nodal <verb> --json` writes, and the
//! test that says so compares them.
//!
//! # What is not here, and why that is a test
//!
//! `reclaim`, `merge`, `gc`, `uninstall` and `base` are absent from the listing and
//! refused by name when they are called. Each of them removes or rewrites something a
//! person has, and the person who runs one is the person who can see what it would take
//! away. An agent that wants one says so in words, and a person types it.
//!
//! `done` is here and it pushes, which is what `nodal done` does. It is the verb that
//! ends an agent's own work, and what it sends is the branch that agent wrote.
//!
//! # Where the tools run
//!
//! In the directory the server was started in, which is the project. A tool takes a
//! unit's handle where the command line does; it never takes a path to work in, because
//! the surface an agent reaches must not be the one that can be pointed anywhere.

use nodal_core::output::Format;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::commands::done::Done;
use crate::commands::handoff::Handoff;
use crate::commands::ls::Ls;
use crate::commands::new::New;
use crate::commands::reclaim::Reclaim;
use crate::commands::show::Show;
use crate::mcp::protocol::{Failure, INVALID_PARAMS};

/// One tool, as `tools/list` states it and `tools/call` runs it.
pub struct Tool {
    /// What an agent calls it.
    pub name: &'static str,
    /// What it does, in one sentence.
    pub description: &'static str,
    /// The arguments it takes, as JSON Schema.
    pub schema: fn() -> Value,
    /// The work. The string is the JSON the matching `--json` writes.
    pub call: fn(&Cli, &Value) -> Result<String, Fault>,
}

/// The verbs an agent may not call here, with the reason each one is a person's.
///
/// They are named rather than merely absent, because an agent that asked for one has to
/// be told where the verb is rather than that it does not exist.
pub const WITHHELD: [(&str, &str); 5] = [
    ("reclaim", "it removes a unit's home"),
    ("merge", "it rewrites a branch and removes the unit"),
    ("gc", "it removes trashed homes and warm bases"),
    ("uninstall", "it removes what Nodal installed on this machine"),
    ("base", "it builds and removes the trees homes are cloned from"),
];

/// Every tool, in the order `tools/list` states them: what reads first, what writes
/// after it.
#[must_use]
pub fn all() -> Vec<Tool> {
    let mut tools = reading();
    tools.extend(writing());
    tools
}

/// The tools that read and change nothing.
fn reading() -> Vec<Tool> {
    vec![
        Tool {
            name: "ls",
            description: "List every unit of the project: what each one needs next, its \
                          branch, its integration and who is in it.",
            schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
            call: |cli, _| {
                render(
                    Ls { path: None, json: true }
                        .rendered(store_if_present(cli)?.as_ref(), Format::Json),
                )
            },
        },
        Tool {
            name: "show",
            description: "Report one unit in full: its work, its environment, what was \
                          recorded of its home, and its log.",
            schema: || {
                unit_argument("The unit's handle. Defaults to the unit of the working directory.")
            },
            call: |cli, arguments| {
                let command = Show { unit: text(arguments, "unit"), json: true };
                render(command.rendered(&store(cli)?, Format::Json))
            },
        },
        Tool {
            name: "check",
            description: "Report what reclaiming a unit would take away, and do none of \
                          it: what is only here, what has another copy, and what is \
                          running.",
            schema: || {
                unit_argument("The unit's handle. Defaults to the unit of the working directory.")
            },
            call: |cli, arguments| {
                let command = Reclaim {
                    // The tool surface reads; it never removes build output from a
                    // checkout somebody adopted in place.
                    prune: false,
                    units: text(arguments, "unit").into_iter().collect(),
                    check: true,
                    force: false,
                    json: true,
                    yes: false,
                };
                let request = command.request(!cli.no_hooks);
                let (answer, _) = Reclaim::checked(&store(cli)?, &request, Format::Json)
                    .map_err(|error| refused(&error))?;
                Ok(answer)
            },
        },
    ]
}

/// The tools that write: one makes a unit, one states a handoff, one pushes.
fn writing() -> Vec<Tool> {
    vec![
        Tool {
            name: "new",
            description: "Make a unit: a branch, a home cloned from the project, its own \
                          ports and its own environment.",
            schema: new_schema,
            call: |cli, arguments| {
                let command = New {
                    path: None,
                    objective: Some(required(arguments, "objective")?),
                    name: text(arguments, "name"),
                    from: text(arguments, "from"),
                    carry: flag(arguments, "carry"),
                    json: true,
                };
                let mut store = store(cli)?;
                let (answer, _) = command
                    .made(&mut store, !cli.no_hooks, Format::Json)
                    .map_err(|error| refused(&error))?;
                Ok(answer)
            },
        },
        Tool {
            name: "handoff",
            description: "Leave a note on a unit for whoever continues it, which the \
                          unit's memory carries to the next session.",
            schema: handoff_schema,
            call: |cli, arguments| {
                let command = Handoff {
                    unit: text(arguments, "unit"),
                    text: required(arguments, "text")?,
                    json: true,
                };
                render(command.rendered(&store(cli)?, Format::Json))
            },
        },
        Tool {
            name: "done",
            description: "Push a unit's work for review and report where the change is \
                          opened. It sends the unit's branch and nothing else.",
            schema: done_schema,
            call: |cli, arguments| {
                let command = Done {
                    unit: text(arguments, "unit"),
                    remote: text(arguments, "remote"),
                    // Never from here. `--wip` sends the work-in-progress snapshot,
                    // which carries every uncommitted and untracked file of the home,
                    // and an agent must not be able to put a person's unfinished work
                    // on a remote. The flag stays on the command line, where the person
                    // who types it is the person whose work it is.
                    wip: false,
                    json: true,
                };
                let mut store = store(cli)?;
                render(command.rendered(&mut store, Format::Json))
            },
        },
    ]
}

/// The schema of a tool whose only argument is a unit's handle.
fn unit_argument(about: &str) -> Value {
    json!({
        "type": "object",
        "properties": { "unit": { "type": "string", "description": about } },
        "additionalProperties": false,
    })
}

/// The arguments `new` takes.
fn new_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "objective": { "type": "string", "description": "What the unit is for." },
            "name": { "type": "string", "description": "The handle to give it. Derived from the objective when it is not said." },
            "from": { "type": "string", "description": "The branch to fork from. The project's own default when it is not said." },
            "carry": { "type": "boolean", "description": "Copy the checkout's uncommitted work into the new home." },
        },
        "required": ["objective"],
        "additionalProperties": false,
    })
}

/// The arguments `handoff` takes.
fn handoff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "unit": { "type": "string", "description": "The unit's handle. Defaults to the unit of the working directory." },
            "text": { "type": "string", "description": "What to leave for whoever continues the unit." },
        },
        "required": ["text"],
        "additionalProperties": false,
    })
}

/// The arguments `done` takes.
///
/// No `wip`. The tool sends the unit's branch and nothing else, which is what its
/// description says and what the surface promises; the flag that sends uncommitted work
/// is the command line's.
fn done_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "unit": { "type": "string", "description": "The unit's handle. Defaults to the unit of the working directory." },
            "remote": { "type": "string", "description": "The remote to push to. `origin` when it is not said." },
        },
        "additionalProperties": false,
    })
}

/// Why a call produced no answer.
///
/// The two are not one thing. A **protocol** fault is the caller's message being wrong —
/// an argument of the wrong type, a key no tool takes — and it goes back as a JSON-RPC
/// error, because nothing a model could say would make that message right. A **refusal**
/// is the work itself saying no — no such unit, a held unit, a handoff with nothing in
/// it — and it goes back as the tool's own result, marked as failed, carrying the
/// sentence the command line prints. A model has to read a refusal to act on it, and
/// several clients never show a protocol error to a model at all.
#[derive(Debug)]
pub enum Fault {
    /// The message was wrong.
    Protocol(Failure),
    /// The work said no, in the words a person would be told.
    Refused(String),
}

/// Check one call's arguments against the tool's own published schema.
///
/// The schema is the one `tools/list` states and `schemas/mcp/tools.json` holds, so what
/// a caller reads and what the server enforces cannot differ: there is no second list of
/// keys anywhere in this file. `additionalProperties: false` is enforced rather than
/// documented, a required key that is missing is named, and a key whose value is of the
/// wrong type is named with the type it should be — `"true"` is not `true`.
///
/// # Errors
///
/// [`INVALID_PARAMS`] naming the key at fault.
pub fn check(schema: &Value, arguments: &Value) -> Result<(), Failure> {
    let Some(given) = arguments.as_object() else {
        return Err(Failure::new(INVALID_PARAMS, "arguments must be an object"));
    };
    let properties = schema.get("properties").and_then(Value::as_object);
    for key in given.keys() {
        if !properties.is_some_and(|declared| declared.contains_key(key)) {
            return Err(Failure::new(
                INVALID_PARAMS,
                format!("this tool takes no argument called {key:?}"),
            ));
        }
    }
    for (key, declared) in properties.into_iter().flatten() {
        let wanted = declared.get("type").and_then(Value::as_str).unwrap_or("string");
        match given.get(key) {
            None | Some(Value::Null) => {}
            Some(Value::String(_)) if wanted == "string" => {}
            Some(Value::Bool(_)) if wanted == "boolean" => {}
            Some(held) => {
                return Err(Failure::new(
                    INVALID_PARAMS,
                    format!("{key} must be a {wanted}; it is {held}"),
                ));
            }
        }
    }
    for key in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
        let Some(key) = key.as_str() else { continue };
        if !given.contains_key(key) || given[key].is_null() {
            return Err(Failure::new(INVALID_PARAMS, format!("{key} is required")));
        }
    }
    Ok(())
}

/// One string argument, when it was given.
///
/// The type is settled by [`check`] before any tool runs, so anything but a string here
/// is a value the schema does not describe and the tool asks for none.
fn text(arguments: &Value, name: &str) -> Option<String> {
    arguments.get(name).and_then(Value::as_str).map(ToOwned::to_owned)
}

/// One string argument the tool cannot work without.
///
/// A second guard behind [`check`], which has already refused a call that left this out.
/// It is here because a tool that cannot work without a value must not be able to run
/// without one if a schema and a call ever part company; it is not a place to add a
/// third check of the same thing.
fn required(arguments: &Value, name: &str) -> Result<String, Fault> {
    text(arguments, name)
        .ok_or_else(|| Fault::Protocol(Failure::new(INVALID_PARAMS, format!("{name} is required"))))
}

/// One boolean argument, false where it was not given.
fn flag(arguments: &Value, name: &str) -> bool {
    arguments.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// The registry, made where this machine has none, as every writing command opens it.
fn store(cli: &Cli) -> Result<nodal_core::store::Store, Fault> {
    cli.registry().map_err(|error| refused(&error))
}

/// The registry when this machine has one, as the list opens it.
fn store_if_present(cli: &Cli) -> Result<Option<nodal_core::store::Store>, Fault> {
    cli.registry_if_present().map_err(|error| refused(&error))
}

/// A rendering that may have failed, as the answer or the refusal.
fn render(answer: nodal_core::Result<String>) -> Result<String, Fault> {
    answer.map_err(|error| refused(&error))
}

/// A refusal, in the words the command line prints for it.
fn refused(error: &nodal_core::Error) -> Fault {
    Fault::Refused(error.to_string())
}
