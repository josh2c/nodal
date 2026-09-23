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

/// The invariant the whole surface rests on: a tool answers with what the command line
/// writes for the same question. One value, two callers, no second shape.
///
/// **What is compared is the parsed document, not the bytes.** Every answer carries the
/// instant it was taken and the ages measured from it, so two readings of one machine
/// are never byte-identical; `volatile` removes those and the comparison is of
/// everything else. The write verbs are compared the same way with the fields that name
/// one unit removed as well, because two units are not one unit — what is asserted there
/// is that the two routes produce the same document about the work they did. `read_at` is
/// one of those instants: a verdict's evidence record dates the reading it was made from,
/// and two readings of one machine are taken at two instants by construction.
#[test]
fn every_reading_tool_answers_with_the_command_lines_own_json() {
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
        let output = workspace.nodal(&args);
        assert!(output.status.success(), "nodal {}", args.join(" "));
        let printed = nodal_safety::text::answer(&output);
        assert_eq!(
            volatile(&told),
            volatile(&printed),
            "the tool and `nodal {}` disagree",
            args.join(" ")
        );
    }
}

/// The same invariant for the three tools that write. Each one is run both ways and the
/// two documents are compared with the clock and the identity of the unit removed.
#[test]
fn every_writing_tool_answers_with_the_command_lines_own_json() {
    let workspace = workspace();

    // `new`: one unit each way, with the same objective, so everything but which unit it
    // is has to match.
    let made = ask(
        workspace.command(&["mcp"]),
        &[call(1, "new", &json!({ "objective": "import the ledger", "name": "by-tool" }))],
    );
    let by_hand =
        stdout(&workspace.nodal(&["new", "--name", "by-hand", "import the ledger", "--json"]));
    assert_eq!(anonymous(&text(&made[0])), anonymous(&by_hand), "`new` disagrees");

    // `handoff` and `done`: the same unit both ways, so only the clock differs.
    let stated = ask(
        workspace.command(&["mcp"]),
        &[
            call(1, "handoff", &json!({ "unit": "by-tool", "text": "the parser still fails" })),
            call(2, "done", &json!({ "unit": "by-tool" })),
        ],
    );
    let said = stdout(&workspace.nodal(&[
        "handoff",
        "--unit",
        "by-tool",
        "the parser still fails",
        "--json",
    ]));
    assert_eq!(anonymous(&text(&stated[0])), anonymous(&said), "`handoff` disagrees");

    let pushed = stdout(&workspace.nodal(&["done", "by-tool", "--json"]));
    assert_eq!(anonymous(&text(&stated[1])), anonymous(&pushed), "`done` disagrees");
}

/// The same answer with the fields that differ between two readings of one machine taken
/// out: the instant it was taken, and the ages measured from it.
/// What a second reading of one machine cannot be expected to repeat.
///
/// Two kinds, and both are readings rather than answers. The instants — when the answer
/// was taken, and when a verdict's evidence was read — move by construction. So do the
/// counts of the process table a verdict rests on: a machine starts and ends processes
/// between two commands, and `seen`, `read` and `withheld` are how many there were at the
/// moment each command looked. A comparison of those would be asserting that nothing
/// happened on the host, which is not the claim.
const VOLATILE: &[&str] = &["now", "read_at", "seen", "read", "withheld"];

fn volatile(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).expect("a tool answers with JSON");
    strip(&mut value, VOLATILE);
    value
}

/// The same, with the identity of the unit taken out as well: which unit it is, where it
/// lives and what it was given, none of which two units share.
fn anonymous(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).expect("a tool answers with JSON");
    strip(&mut value, VOLATILE);
    strip(
        &mut value,
        &[
            "id",
            "slug",
            "unit",
            "branch",
            "home",
            "ports",
            "created_at",
            "ts",
            "taken_at",
            "refreshed_at",
            "expires_at",
            "last_active",
            "compare",
            "pushed",
            "refs",
            "events",
            "base",
            "commit",
            "environment",
        ],
    );
    value
}

/// Remove these keys wherever they sit.
fn strip(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for held in map.values_mut() {
                strip(held, keys);
            }
        }
        Value::Array(items) => {
            for held in items.iter_mut() {
                strip(held, keys);
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
    // The refusal is the tool's own answer, marked as failed, so the agent that asked for
    // the verb reads where it lives. It is not a protocol error, which several clients
    // never show a model.
    for (answer, verb) in answers[1..].iter().zip(withheld) {
        assert_eq!(answer["result"]["isError"], true, "{answer:?}");
        assert!(answer["error"].is_null(), "a withheld verb answered with a protocol error");
        let said = text(answer);
        assert!(said.contains(verb), "the refusal does not name the verb: {said}");
        assert!(
            said.contains(&format!("`nodal {verb}`")),
            "it does not say where to run it: {said}"
        );
    }
}

/// The two kinds of "no", which are not one kind.
///
/// A refusal of the **work** — no such unit — is the tool's own answer: a result marked
/// as failed, carrying the sentence the command line prints. A model has to read it to
/// act on it, and several clients never show a protocol error to a model at all.
///
/// A fault in the **message** — an argument the tool does not take, one of the wrong
/// type, a required one missing, a method this server does not answer — is a JSON-RPC
/// error, because nothing a model could say would make that message right.
#[test]
fn a_refusal_of_the_work_is_an_answer_and_a_bad_message_is_an_error() {
    let workspace = workspace();
    let answers = ask(
        workspace.command(&["mcp"]),
        &[
            call(1, "show", &json!({ "unit": "no-such-unit" })),
            call(2, "handoff", &json!({})),
            call(3, "show", &json!({ "unit": "x", "nonsense": "y" })),
            call(4, "new", &json!({ "objective": "x", "carry": "true" })),
            request(5, "tools/call", &json!({ "name": "not-a-tool" })),
            request(6, "no/such/method", &json!({})),
        ],
    );

    // The work said no.
    assert_eq!(answers[0]["result"]["isError"], true, "{:?}", answers[0]);
    assert!(text(&answers[0]).contains("no-such-unit"), "{}", text(&answers[0]));
    assert!(answers[0]["error"].is_null(), "a refusal was sent as a protocol error");

    // The message was wrong.
    for (answer, says) in [
        (&answers[1], "text is required"),
        (&answers[2], "no argument called \"nonsense\""),
        (&answers[3], "carry must be a boolean"),
        (&answers[4], "not-a-tool"),
    ] {
        assert_eq!(answer["error"]["code"], -32_602, "{answer:?}");
        assert!(
            answer["error"]["message"].as_str().unwrap().contains(says),
            "the error does not name what is wrong: {answer:?}"
        );
    }
    assert_eq!(answers[5]["error"]["code"], -32_601, "an unknown method: {:?}", answers[5]);
}

/// What the wire refuses before any tool runs: a line that is not a request, a batch, a
/// message without the version or the method, and an identifier of a kind that cannot be
/// answered under.
#[test]
fn a_line_that_is_not_a_request_is_refused_as_one() {
    let workspace = workspace();
    let mut command = workspace.command(&["mcp"]);
    let mut server = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the server starts");
    {
        let mut stdin = server.stdin.take().expect("the server reads");
        for line in [
            "not json at all",
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
            r#"{"id":2,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":3}"#,
            r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ] {
            writeln!(stdin, "{line}").unwrap();
        }
    }
    let stdout = server.stdout.take().expect("the server writes");
    let answers: Vec<Value> = BufReader::new(stdout)
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).expect("every answer is one JSON line"))
        .collect();
    let _ = server.wait().unwrap();

    assert_eq!(answers.len(), 5, "a notification was answered: {answers:?}");
    assert_eq!(answers[0]["error"]["code"], -32_700, "{:?}", answers[0]);
    for answer in &answers[1..] {
        assert_eq!(answer["error"]["code"], -32_600, "{answer:?}");
    }
    // The identifier is echoed wherever the line carried one that can be answered under,
    // because a client matches answers by identifier.
    assert_eq!(answers[2]["id"], 2, "{:?}", answers[2]);
    assert_eq!(answers[3]["id"], 3, "{:?}", answers[3]);
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
