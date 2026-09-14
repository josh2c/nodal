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
use crate::mcp::protocol::{Failure, INTERNAL_ERROR, INVALID_PARAMS};

/// One tool, as `tools/list` states it and `tools/call` runs it.
pub struct Tool {
    /// What an agent calls it.
    pub name: &'static str,
    /// What it does, in one sentence.
    pub description: &'static str,
    /// The arguments it takes, as JSON Schema.
    pub schema: fn() -> Value,
    /// The work. The string is the JSON the matching `--json` writes.
    pub call: fn(&Cli, &Value) -> Result<String, Failure>,
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
                let command = Show { unit: text(arguments, "unit")?, json: true };
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
                    unit: text(arguments, "unit")?,
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
                    name: text(arguments, "name")?,
                    from: text(arguments, "from")?,
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
                    unit: text(arguments, "unit")?,
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
                    unit: text(arguments, "unit")?,
                    remote: text(arguments, "remote")?,
                    wip: flag(arguments, "wip"),
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
fn done_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "unit": { "type": "string", "description": "The unit's handle. Defaults to the unit of the working directory." },
            "remote": { "type": "string", "description": "The remote to push to. `origin` when it is not said." },
            "wip": { "type": "boolean", "description": "Send the work-in-progress snapshot as well as the branch." },
        },
        "additionalProperties": false,
    })
}

/// One string argument, when it was given.
fn text(arguments: &Value, name: &str) -> Result<Option<String>, Failure> {
    match arguments.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(given)) => Ok(Some(given.clone())),
        Some(_) => Err(Failure::new(INVALID_PARAMS, format!("{name} must be a string"))),
    }
}

/// One string argument the tool cannot work without.
fn required(arguments: &Value, name: &str) -> Result<String, Failure> {
    text(arguments, name)?
        .ok_or_else(|| Failure::new(INVALID_PARAMS, format!("{name} is required")))
}

/// One boolean argument, false where it was not given.
fn flag(arguments: &Value, name: &str) -> bool {
    arguments.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// The registry, made where this machine has none, as every writing command opens it.
fn store(cli: &Cli) -> Result<nodal_core::store::Store, Failure> {
    cli.registry().map_err(|error| refused(&error))
}

/// The registry when this machine has one, as the list opens it.
fn store_if_present(cli: &Cli) -> Result<Option<nodal_core::store::Store>, Failure> {
    cli.registry_if_present().map_err(|error| refused(&error))
}

/// A rendering that may have failed, as the answer or the refusal.
fn render(answer: nodal_core::Result<String>) -> Result<String, Failure> {
    answer.map_err(|error| refused(&error))
}

/// A refusal, in the words the command line prints for it.
fn refused(error: &nodal_core::Error) -> Failure {
    Failure::new(INTERNAL_ERROR, error.to_string())
}
