//! The output layer: one value, two renderings.
//!
//! Every read command answers with a value, never with printed text. The value
//! implements [`Render`], which gives it a JSON form (its `serde` shape) and a human
//! form (a [`human::Doc`] of blocks). `--json` and the default output are therefore two
//! renderings of one value rather than two code paths that can drift apart, and
//! `status --watch` is the JSON rendering emitted one line at a time ([`watch`]).
//!
//! The read types themselves live in [`view`]. They are shaped for reading rather than
//! for storage: a row of `nodal ls` gathers a unit, its environment and its freshness,
//! which the registry keeps in three tables. Facts that a later task supplies — disk
//! usage, running processes, staleness — are optional on the view, so a producer fills
//! in what it knows and the renderer prints a placeholder for the rest.

pub mod human;
pub mod json;
pub mod notice;
pub mod view;
pub mod watch;

use std::io::Write;

use serde::Serialize;

pub use crate::output::human::{Block, Doc, Field, Table};
pub use crate::output::notice::Notice;

/// Which rendering a command was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Aligned text for a person.
    #[default]
    Human,
    /// Pretty JSON for a tool.
    Json,
}

impl Format {
    /// The format a `--json` flag asks for.
    #[must_use]
    pub const fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

/// A value a read command can answer with.
///
/// The `serde` shape is the contract other tools read; [`Render::doc`] is what a person
/// sees. Implementing this trait is the whole of adding a read type to the CLI.
pub trait Render: Serialize {
    /// What this type is called in an error message, and in a stream's diagnostics.
    const KIND: &'static str;

    /// Top-level fields that differ between two answers that say the same thing: the
    /// instant the answer was taken, and anything else derived from the clock. A stream
    /// still writes them — they are what dates a frame — but ignores them when deciding
    /// whether an answer is news ([`watch::Stream`]). Without this, polling would emit
    /// a frame every tick and "unchanged" could never be observed.
    const VOLATILE: &'static [&'static str] = &["now"];

    /// The human form, as blocks. Layout is [`human`]'s decision, not this method's.
    fn doc(&self) -> Doc;
}

/// Render a value in the format asked for, ending in exactly one newline.
///
/// # Errors
///
/// [`Error::Render`](crate::Error::Render) if the value cannot be encoded as JSON.
pub fn render<T: Render>(value: &T, format: Format) -> crate::Result<String> {
    match format {
        Format::Human => Ok(value.doc().to_string()),
        Format::Json => json::pretty(value),
    }
}

/// Render a value and write it out.
///
/// # Errors
///
/// [`Error::Render`](crate::Error::Render) if the value cannot be encoded as JSON, and
/// [`Error::Io`](crate::Error::Io) if the destination refuses the bytes.
pub fn write<T: Render, W: Write>(value: &T, format: Format, out: &mut W) -> crate::Result<()> {
    let text = render(value, format)?;
    out.write_all(text.as_bytes()).map_err(crate::Error::io(T::KIND))
}
