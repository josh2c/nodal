//! The JSON renderer.
//!
//! Two forms of the same value: pretty for `--json`, where a person is often the one
//! reading it, and compact for a stream, where one document must occupy one line. Both
//! end in a newline, so output is line-oriented either way.

use crate::output::Render;

/// The value as pretty JSON, with a trailing newline.
///
/// # Errors
///
/// [`Error::Render`](crate::Error::Render) if the value cannot be encoded.
pub fn pretty<T: Render>(value: &T) -> crate::Result<String> {
    let mut text = serde_json::to_string_pretty(value).map_err(encoding::<T>)?;
    text.push('\n');
    Ok(text)
}

/// The value as one line of JSON, with a trailing newline. This is the NDJSON form.
///
/// # Errors
///
/// [`Error::Render`](crate::Error::Render) if the value cannot be encoded.
pub fn compact<T: Render>(value: &T) -> crate::Result<String> {
    let mut text = serde_json::to_string(value).map_err(encoding::<T>)?;
    text.push('\n');
    Ok(text)
}

/// Name the read type in the error, so a failure says which document was being written.
fn encoding<T: Render>(source: serde_json::Error) -> crate::Error {
    crate::Error::Render { kind: T::KIND, source }
}
