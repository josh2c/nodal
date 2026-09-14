//! Acceptance for `nodal mcp`: the tool surface every agent reaches, on the wire.
//!
//! Every claim here is made by writing JSON-RPC lines into the server's standard input
//! and reading what comes back, because that is what an agent does. Nothing reaches
//! inside the binary.
//!
//! Four claims:
//!
//! 1. The handshake. `initialize` says what the server is, `tools/list` names the tools,
//!    `ping` answers, and a notification is never answered.
//! 2. **A tool result is the command's own `--json`, to the byte.** The same unit is
//!    asked for through the tool and through the command line, and the two texts are
//!    compared. A second representation of a unit would show up here as a diff.
//! 3. The withheld verbs are not in the listing, and calling one by name is refused with
//!    the reason and the command a person runs instead.
//! 4. A refusal is a JSON-RPC error carrying the sentence the command line prints.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use nodal_safety::Workspace;
use nodal_safety::text::stdout;
use serde_json::{Value, json};

mod state;

/// A project this suite's commands work in, with its own state directory.
fn workspace() -> Workspace {
    Workspace::new(state::BINARY)
}

/// Ask the server a list of requests and answer with what came back, in order.
///
/// One process for the whole conversation, which is what a client does: the server is
/// started once and answers until its standard input ends.
fn ask(command: Command, requests: &[Value]) -> Vec<Value> {
    let mut command = command;
    let mut server = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    // The handle is taken and dropped, which closes the pipe. A server whose standard
    // input is still open is a server waiting for the next line, which is what it should
    // do and what would hang a test that has asked everything it means to ask.
    {
        let mut stdin = server.stdin.take().expect("the server reads");
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let stdout = server.stdout.take().expect("the server writes");
    let answers: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).expect("every answer is one JSON line"))
        .collect();
    let _ = server.wait().unwrap();
    answers
}

/// One request.
fn request(id: u32, method: &str, params: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

/// One tool call.
fn call(id: u32, name: &str, arguments: &Value) -> Value {
    request(id, "tools/call", &json!({ "name": name, "arguments": arguments }))
}

/// The text a tool answered with.
fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"].as_str().expect("a text block").to_owned()
}

#[test]
fn the_handshake_names_the_server_and_its_tools() {
    let workspace = workspace();
    let answers = ask(
        workspace.command(&["mcp"]),
        &[
            request(1, "initialize", &json!({})),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            request(2, "tools/list", &json!({})),
            request(3, "ping", &json!({})),
        ],
    );

    assert_eq!(answers.len(), 3, "a notification was answered: {answers:?}");
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "nodal");
    assert!(answers[0]["result"]["protocolVersion"].is_string());
    assert_eq!(answers[0]["result"]["capabilities"]["tools"], json!({}));

    let named: Vec<String> = answers[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(named, ["ls", "show", "check", "new", "handoff", "done"]);
    for tool in answers[1]["result"]["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
        assert!(tool["description"].as_str().is_some_and(|said| !said.is_empty()), "{tool}");
    }
    assert_eq!(answers[2]["result"], json!({}), "ping answers with nothing");
}

/// The invariant the whole surface rests on: a tool answers with the bytes the command
/// line writes for the same question. One value, two callers, no second shape.
#[test]
fn every_tool_answers_with_the_command_lines_own_json() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));

    let answers = ask(
        workspace.command(&["mcp"]),
        &[
            call(1, "ls", &json!({})),
            call(2, "show", &json!({ "unit": "worker-import" })),
            call(3, "check", &json!({ "unit": "worker-import" })),
        ],
    );

    for (answer, args) in answers.iter().zip([
        vec!["ls", "--json"],
        vec!["show", "worker-import", "--json"],
        vec!["reclaim", "worker-import", "--check", "--json"],
    ]) {
        let told = text(answer);
        let printed = stdout(&workspace.nodal(&args));
        let (told, printed) = (volatile(&told), volatile(&printed));
        assert_eq!(told, printed, "the tool and `nodal {}` disagree", args.join(" "));
    }
}

/// The same answer with the fields that differ between two readings of one machine taken
/// out: the instant it was taken, and the ages measured from it.
fn volatile(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).expect("a tool answers with JSON");
    strip(&mut value);
    value
}

/// Remove every `now` and every instant derived from the clock, wherever it sits.
fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("now");
            for held in map.values_mut() {
                strip(held);
            }
        }
        Value::Array(items) => {
            for held in items.iter_mut() {
                strip(held);
            }
        }
        _ => {}
    }
}

/// The verbs that remove or rewrite something a person has are a person's. They are not
/// in the listing, and asking for one by name is refused with the reason.
#[test]
fn the_withheld_verbs_are_not_offered_and_are_refused_by_name() {
    let workspace = workspace();
    let withheld = ["reclaim", "merge", "gc", "uninstall", "base"];
    let mut requests = vec![request(1, "tools/list", &json!({}))];
    for (index, verb) in withheld.iter().enumerate() {
        requests.push(call(u32::try_from(index).unwrap() + 2, verb, &json!({})));
    }

    let answers = ask(workspace.command(&["mcp"]), &requests);

    let listed = answers[0]["result"]["tools"].as_array().unwrap();
    for verb in withheld {
        assert!(
            !listed.iter().any(|tool| tool["name"] == verb),
            "{verb} is in the listing: {listed:?}"
        );
    }
    for (answer, verb) in answers[1..].iter().zip(withheld) {
        let said = answer["error"]["message"].as_str().expect("a refusal with a reason");
        assert!(said.contains(verb), "the refusal does not name the verb: {said}");
        assert!(
            said.contains(&format!("`nodal {verb}`")),
            "it does not say where to run it: {said}"
        );
        assert!(answer["result"].is_null(), "a withheld verb answered with a result: {answer}");
    }
}

/// A refusal is an error with the sentence the command line prints, and a method the
/// server does not answer is an error and never a silence.
#[test]
fn a_refusal_carries_the_reason_the_command_line_prints() {
    let workspace = workspace();
    let answers = ask(
        workspace.command(&["mcp"]),
        &[
            call(1, "show", &json!({ "unit": "no-such-unit" })),
            call(2, "handoff", &json!({})),
            request(3, "tools/call", &json!({ "name": "not-a-tool" })),
            request(4, "no/such/method", &json!({})),
        ],
    );

    assert!(
        answers[0]["error"]["message"].as_str().unwrap().contains("no-such-unit"),
        "{:?}",
        answers[0]
    );
    assert!(
        answers[1]["error"]["message"].as_str().unwrap().contains("text is required"),
        "{:?}",
        answers[1]
    );
    assert!(answers[2]["error"]["message"].as_str().unwrap().contains("not-a-tool"));
    assert!(answers[3]["error"]["message"].as_str().unwrap().contains("no/such/method"));
}

/// The writing tools write: a handoff is on the unit's log afterwards, and a unit an
/// agent makes has a home, its own ports and its own environment.
#[test]
fn a_unit_an_agent_makes_is_a_unit_with_its_own_ports_and_environment() {
    let workspace = workspace();

    let answers = ask(
        workspace.command(&["mcp"]),
        &[
            call(1, "new", &json!({ "objective": "fix the importer", "name": "importer" })),
            call(2, "handoff", &json!({ "unit": "importer", "text": "the parser still fails" })),
        ],
    );

    let made: Value = serde_json::from_str(&text(&answers[0])).unwrap();
    let environment = &made["unit"]["environment"];
    assert_eq!(made["unit"]["slug"], "importer");
    assert!(environment["home"].as_str().is_some_and(|home| !home.is_empty()), "{made}");
    assert!(environment["ports"].as_object().is_some_and(|ports| !ports.is_empty()), "{made}");
    assert!(
        std::path::Path::new(environment["home"].as_str().unwrap()).join(".envrc").is_file(),
        "the home carries no environment: {made}"
    );

    let stated: Value = serde_json::from_str(&text(&answers[1])).unwrap();
    assert_eq!(stated["events"][0]["kind"], "handoff");
    assert_eq!(stated["events"][0]["epistemic"], "stated");
    let shown = stdout(&workspace.nodal(&["show", "importer", "--json"]));
    assert!(shown.contains("the parser still fails"), "the log does not carry it: {shown}");
}
