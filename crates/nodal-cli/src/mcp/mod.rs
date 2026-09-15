//! `nodal mcp`: the tool surface every agent can reach, without a hook of its own.
//!
//! An agent speaks to Nodal through one server rather than through one adapter per
//! vendor. The transport is standard input and standard output, one JSON-RPC message per
//! line ([`protocol`]), and the loop is synchronous: read a line, answer it, write the
//! answer, read the next. There is no runtime, no task and no thread.
//!
//! Standard output carries nothing but answers. Everything a command would have said to
//! a person — progress, notes about what an interrupted run was resolved to — goes to
//! standard error, where a client's log takes it.
//!
//! Each call opens the registry the way the matching command does and closes it again,
//! so a tool call is a `nodal` invocation in every respect that matters: the same
//! reading, the same writes, the same refusals, and the interrupted-operation preamble
//! before it.
//!
//! What the tools are, and what they are not, is [`tools`].

pub mod protocol;
pub mod tools;

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::mcp::protocol::{Answer, Failure, INVALID_PARAMS, METHOD_NOT_FOUND, Message};
use crate::mcp::tools::Fault;

/// The version of the model context protocol this server answers with.
const PROTOCOL: &str = "2025-06-18";

/// What the server calls itself to a client.
const SERVER: &str = "nodal";

/// Answer requests on `input` until it ends.
///
/// # Errors
///
/// [`nodal_core::Error::Io`] when a line could not be read, or an answer could not be
/// written. A refusal inside a tool is an answer and never an error here.
pub fn serve(cli: &Cli, input: &mut dyn BufRead, output: &mut dyn Write) -> nodal_core::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = input.read_line(&mut line).map_err(nodal_core::Error::io("<stdin>"))?;
        if read == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = answer(cli, &line) {
            writeln!(output, "{}", answer.line()).map_err(nodal_core::Error::io("<stdout>"))?;
            output.flush().map_err(nodal_core::Error::io("<stdout>"))?;
        }
    }
}

/// The answer to one line, or nothing for a notification.
fn answer(cli: &Cli, line: &str) -> Option<Answer> {
    let message = match Message::read(line) {
        Ok(message) => message,
        Err((id, failure)) => return Some(Answer::failed(id, failure)),
    };
    // A message with no identifier is a notification: it is never answered, and the one
    // that matters here (`notifications/initialized`) asks for nothing.
    let id = message.id.clone()?;
    Some(match dispatch(cli, &message) {
        Ok(result) => Answer::result(id, result),
        Err(failure) => Answer::failed(id, failure),
    })
}

/// What one method answers with.
fn dispatch(cli: &Cli, message: &Message) -> Result<Value, Failure> {
    match message.method.as_str() {
        "initialize" => Ok(initialize()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(listing()),
        "tools/call" => call(cli, &message.params),
        other => {
            Err(Failure::new(METHOD_NOT_FOUND, format!("nodal mcp does not answer {other:?}")))
        }
    }
}

/// What the server says it is and what it can do.
fn initialize() -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
    })
}

/// The tool listing as a document, for the copy `ci/schema-diff.sh` keeps.
///
/// The same value `tools/list` answers with, rendered the way every other committed
/// schema is: one document, ending in one newline.
#[must_use]
pub fn tool_listing() -> String {
    format!("{:#}\n", listing())
}

/// Every tool, as a client lists them.
fn listing() -> Value {
    let tools: Vec<Value> = tools::all()
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": (tool.schema)(),
            })
        })
        .collect();
    json!({ "tools": tools })
}

/// Run one tool and answer with what its `--json` writes.
///
/// The result is one text block and it is the command's own JSON. A client that wants
/// the value parses it; a model reads it as it stands.
fn call(cli: &Cli, params: &Value) -> Result<Value, Failure> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(Failure::new(INVALID_PARAMS, "the call names no tool"));
    };
    let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    if let Some((verb, why)) = tools::WITHHELD.iter().find(|(verb, _)| *verb == name) {
        return Err(Failure::new(
            INVALID_PARAMS,
            format!("nodal mcp does not offer {verb}, because {why}; a person runs `nodal {verb}`"),
        ));
    }
    let tools = tools::all();
    // A tool that is not there is a parameter that is wrong, not a method that is
    // missing: the method — `tools/call` — is one this server answers.
    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
        return Err(Failure::new(INVALID_PARAMS, format!("nodal mcp has no tool called {name:?}")));
    };
    tools::check(&(tool.schema)(), &arguments)?;
    match (tool.call)(cli, &arguments) {
        Ok(answer) => {
            Ok(json!({ "content": [{ "type": "text", "text": answer }], "isError": false }))
        }
        // A refusal is the tool's answer and not the protocol's. A model has to read it
        // to act on it, and several clients never show a protocol error to the model, so
        // the sentence the command line prints comes back as the content of a result
        // that says it failed.
        Err(Fault::Refused(why)) => {
            Ok(json!({ "content": [{ "type": "text", "text": why }], "isError": true }))
        }
        Err(Fault::Protocol(failure)) => Err(failure),
    }
}
