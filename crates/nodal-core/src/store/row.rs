//! Reading model values out of columns, and writing them back in.
//!
//! Every repository file needs the same handful of conversions — a validated newtype
//! from text, an instant from a count of seconds, an enum from its serde name, a map
//! from JSON — so they are declared once here. Each one names the table and column it
//! read, because the failure it reports is a corrupt registry, and the first question
//! is always which row.

use std::path::{Path, PathBuf};

use rusqlite::types::FromSql;
use rusqlite::{Connection, Row};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::model::Timestamp;
use crate::{Error, Result};

/// Turn a failed statement into an [`Error::Store`] naming the file it ran against.
pub(crate) fn store_error(conn: &Connection) -> impl FnOnce(rusqlite::Error) -> Error {
    let path = conn.path().map_or_else(PathBuf::new, PathBuf::from);
    move |source| classify(path, source)
}

/// A unique-constraint failure is an answer a caller acts on — the branch is taken, the
/// lease is held — so it gets its own variant instead of an opaque database error.
fn classify(path: PathBuf, source: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(failure, _) = &source
        && failure.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return Error::StoreConflict { path, source: Box::new(source) };
    }
    Error::Store { path, source: Box::new(source) }
}

/// A column that could not be read as the type the model requires.
fn bad(
    table: &'static str,
    column: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> Error {
    Error::StoreRow { table, column, source: Box::new(source) }
}

/// Read a column of a type SQLite maps directly.
pub(crate) fn plain<T: FromSql>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<T> {
    row.get(column).map_err(|source| bad(table, column, source))
}

/// Read a validated newtype from its text form.
pub(crate) fn scalar<T>(row: &Row<'_>, table: &'static str, column: &'static str) -> Result<T>
where
    T: std::str::FromStr<Err = Error>,
{
    let text: String = plain(row, table, column)?;
    text.parse().map_err(|source: Error| bad(table, column, source))
}

/// Read a nullable validated newtype.
pub(crate) fn scalar_opt<T>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>>
where
    T: std::str::FromStr<Err = Error>,
{
    let text: Option<String> = plain(row, table, column)?;
    text.map(|text| text.parse().map_err(|source: Error| bad(table, column, source))).transpose()
}

/// Read an instant from its count of seconds since the Unix epoch.
pub(crate) fn stamp(row: &Row<'_>, table: &'static str, column: &'static str) -> Result<Timestamp> {
    let seconds: i64 = plain(row, table, column)?;
    Timestamp::from_unix_seconds(seconds).map_err(|source| bad(table, column, source))
}

/// Read a nullable instant.
pub(crate) fn stamp_opt(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<Timestamp>> {
    let seconds: Option<i64> = plain(row, table, column)?;
    seconds
        .map(|seconds| Timestamp::from_unix_seconds(seconds).map_err(|e| bad(table, column, e)))
        .transpose()
}

/// Read a whole number that the model holds in a narrower type.
pub(crate) fn number<T>(row: &Row<'_>, table: &'static str, column: &'static str) -> Result<T>
where
    T: TryFrom<i64>,
    <T as TryFrom<i64>>::Error: std::error::Error + Send + Sync + 'static,
{
    let value: i64 = plain(row, table, column)?;
    T::try_from(value).map_err(|source| bad(table, column, source))
}

/// Read a nullable whole number.
pub(crate) fn number_opt<T>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>>
where
    T: TryFrom<i64>,
    <T as TryFrom<i64>>::Error: std::error::Error + Send + Sync + 'static,
{
    let value: Option<i64> = plain(row, table, column)?;
    value.map(|value| T::try_from(value).map_err(|e| bad(table, column, e))).transpose()
}

/// Read a value whose stored form is JSON: the maps the store never queries into.
pub(crate) fn json<T: DeserializeOwned>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<T> {
    let text: String = plain(row, table, column)?;
    serde_json::from_str(&text).map_err(|source| bad(table, column, source))
}

/// Read a nullable value whose stored form is JSON.
pub(crate) fn json_opt<T: DeserializeOwned>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>> {
    let text: Option<String> = plain(row, table, column)?;
    text.map(|text| serde_json::from_str(&text).map_err(|source| bad(table, column, source)))
        .transpose()
}

/// Read an enum from the name `serde` gives it, which is the name in the schema.
pub(crate) fn name<T: DeserializeOwned>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<T> {
    let text: String = plain(row, table, column)?;
    serde_json::from_value(serde_json::Value::String(text))
        .map_err(|source| bad(table, column, source))
}

/// Read a nullable enum from the name `serde` gives it.
pub(crate) fn name_opt<T: DeserializeOwned>(
    row: &Row<'_>,
    table: &'static str,
    column: &'static str,
) -> Result<Option<T>> {
    let text: Option<String> = plain(row, table, column)?;
    text.map(|text| {
        serde_json::from_value(serde_json::Value::String(text))
            .map_err(|source| bad(table, column, source))
    })
    .transpose()
}

/// Read a path. Paths are stored as text, so one that is not UTF-8 never got in.
pub(crate) fn path(row: &Row<'_>, table: &'static str, column: &'static str) -> Result<PathBuf> {
    let text: String = plain(row, table, column)?;
    Ok(PathBuf::from(text))
}

/// The text form of an enum, as `serde` names it.
pub(crate) fn name_of<T: Serialize>(value: &T, kind: &'static str) -> Result<String> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(Error::StoreEncode { kind }),
    }
}

/// The JSON form of a value the store keeps as one column.
pub(crate) fn json_of<T: Serialize>(value: &T, kind: &'static str) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::StoreEncode { kind })
}

/// The text form of a path, refusing one that could not be read back.
pub(crate) fn path_of(value: &Path) -> Result<&str> {
    value.to_str().ok_or_else(|| Error::InvalidValue {
        kind: "utf-8 path",
        value: value.to_string_lossy().into_owned(),
    })
}

/// Run a query that returns at most one row, and decode it. A query that matched
/// nothing is an answer, not a failure.
///
/// This and [`many`] are what keep each repository file to one statement and one
/// decoder per function: the plumbing of preparing, mapping and classifying failures
/// lives here once.
pub(crate) fn one<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    decode: impl FnOnce(&Row<'_>) -> Result<T>,
) -> Result<Option<T>> {
    match conn.query_row(sql, params, |row| Ok(decode(row))) {
        Ok(decoded) => decoded.map(Some),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(source) => Err(store_error(conn)(source)),
    }
}

/// Run a query and decode every row it returns.
pub(crate) fn many<T>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    decode: impl Fn(&Row<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let mut statement = conn.prepare(sql).map_err(store_error(conn))?;
    let rows = statement.query_map(params, |row| Ok(decode(row))).map_err(store_error(conn))?;
    let mut collected = Vec::new();
    for row in rows {
        collected.push(row.map_err(store_error(conn))??);
    }
    Ok(collected)
}

/// Run a statement that writes, and report how many rows it changed.
pub(crate) fn write(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
    conn.execute(sql, params).map_err(store_error(conn))
}
