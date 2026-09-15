//! JSON-RPC 2.0 over lines of text: the whole of what the tool surface speaks.
//!
//! Hand-written on `serde_json`, and small enough to read in one sitting, because the
//! alternative is a runtime. Every message is one line of JSON on standard input and
//! every answer is one line on standard output. Nothing here is asynchronous: a request
//! is read, answered and written before the next line is read, which is what a stdio
//! transport with one client is.
//!
//! A message with no `id` is a notification. It is acted on and never answered, which is
//! the rule that keeps this loop correct without any bookkeeping of its own.

use serde::Serialize;
use serde_json::Value;

/// The only version of the protocol this speaks.
pub const VERSION: &str = "2.0";

/// A request or a notification, after the line has been checked.
#[derive(Debug)]
pub struct Message {
    /// Which call it is.
    pub method: String,
    /// What to answer it under. Absent makes it a notification, and an explicit `null`
    /// does not: the two are different messages and only one of them is answered.
    pub id: Option<Value>,
    /// The call's arguments, `null` when it carries none.
    pub params: Value,
}

impl Message {
    /// Read one line as a request, or say what is wrong with it.
    ///
    /// The checks are the protocol's own, and each one answers with the identifier the
    /// line carried where it carried one, because a client matches answers by
    /// identifier and an answer under `null` is an answer it cannot match.
    ///
    /// A batch is refused rather than half-supported. The 2025 protocol removed
    /// batching, and a server that accepted an array would have to decide what a partial
    /// failure means.
    ///
    /// # Errors
    ///
    /// The identifier to answer under, and the failure to answer with.
    pub fn read(line: &str) -> Result<Self, (Value, Failure)> {
        let value: Value = serde_json::from_str(line).map_err(|why| {
            (Value::Null, Failure::new(PARSE_ERROR, format!("the line is not JSON: {why}")))
        })?;
        if value.is_array() {
            return Err((
                Value::Null,
                Failure::new(
                    INVALID_REQUEST,
                    "a batch is not supported; send one request per line",
                ),
            ));
        }
        let Some(object) = value.as_object() else {
            return Err((Value::Null, Failure::new(INVALID_REQUEST, "a request is a JSON object")));
        };
        let id = match object.get("id") {
            None => None,
            Some(id) if id.is_string() || id.is_number() || id.is_null() => Some(id.clone()),
            Some(id) => {
                return Err((
                    Value::Null,
                    Failure::new(
                        INVALID_REQUEST,
                        format!("an id is a string, a number or null; this one is {id}"),
                    ),
                ));
            }
        };
        let answering = id.clone().unwrap_or(Value::Null);
        if object.get("jsonrpc").and_then(Value::as_str) != Some(VERSION) {
            return Err((
                answering,
                Failure::new(INVALID_REQUEST, format!("jsonrpc must be {VERSION:?}")),
            ));
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return Err((
                answering,
                Failure::new(INVALID_REQUEST, "a request names a method, as a string"),
            ));
        };
        Ok(Self {
            method: method.to_owned(),
            id,
            params: object.get("params").cloned().unwrap_or(Value::Null),
        })
    }
}

/// What went wrong, in the shape JSON-RPC states.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    /// The class of fault.
    pub code: i32,
    /// The reason, in the words the command line prints for the same refusal.
    pub message: String,
}

/// The line could not be read as JSON.
pub const PARSE_ERROR: i32 = -32_700;
/// The line was JSON and was not a request this server can act on.
pub const INVALID_REQUEST: i32 = -32_600;
/// No such method.
pub const METHOD_NOT_FOUND: i32 = -32_601;
/// The parameters were not what the method takes.
pub const INVALID_PARAMS: i32 = -32_602;

impl Failure {
    /// A failure of a class, with the reason a person would be told.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// One answer: a result or a failure, never both.
#[derive(Debug, Serialize)]
pub struct Answer {
    /// The protocol version, which is always [`VERSION`].
    pub jsonrpc: &'static str,
    /// The identifier of the request being answered.
    pub id: Value,
    /// What the call produced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Why it produced nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
}

impl Answer {
    /// The answer to a request that worked.
    #[must_use]
    pub fn result(id: Value, result: Value) -> Self {
        Self { jsonrpc: VERSION, id, result: Some(result), error: None }
    }

    /// The answer to a request that did not.
    #[must_use]
    pub fn failed(id: Value, error: Failure) -> Self {
        Self { jsonrpc: VERSION, id, result: None, error: Some(error) }
    }

    /// The answer as the one line that goes back.
    ///
    /// A value this small always encodes, and a rendering that could not would leave the
    /// client waiting, so the fallback is a failure with no result rather than nothing.
    #[must_use]
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            String::from(
                r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"the answer could not be encoded"}}"#,
            )
        })
    }
}
