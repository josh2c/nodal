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

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The only version of the protocol this speaks.
pub const VERSION: &str = "2.0";

/// A request or a notification, as it arrives.
#[derive(Debug, Deserialize)]
pub struct Message {
    /// Which call it is.
    pub method: String,
    /// What to answer it under. Absent makes it a notification.
    #[serde(default)]
    pub id: Option<Value>,
    /// The call's arguments, `null` when it carries none.
    #[serde(default)]
    pub params: Value,
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
/// No such method.
pub const METHOD_NOT_FOUND: i32 = -32_601;
/// The parameters were not what the method takes.
pub const INVALID_PARAMS: i32 = -32_602;
/// The call was understood and the work refused or failed.
pub const INTERNAL_ERROR: i32 = -32_603;

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
