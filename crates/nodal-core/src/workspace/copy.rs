//! The backend that works everywhere: copy the bytes.
//!
//! Nothing is shared, so a home costs its own disk and takes as long as the bytes take.
//! It is here for two reasons. A filesystem that cannot share blocks still has to give
//! a person a working unit, and every other backend needs one honest thing to be
//! measured against.

use std::fs::Metadata;
use std::path::Path;

use super::tree::{Ops, Put, materialize};
use super::{Excludes, Materializer, Report};
use crate::error::{Error, Result};

/// Copies the bytes of every file. Works on any filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopyFallback;

impl Materializer for CopyFallback {
    fn name(&self) -> &'static str {
        "copy"
    }

    fn supports(&self, _path: &Path) -> bool {
        true
    }

    fn clone_tree(&self, source: &Path, destination: &Path, exclude: &Excludes) -> Result<Report> {
        materialize(source, destination, exclude, Ops { file: put })
    }
}

/// Copy the bytes of one file, and report how many there were.
///
/// # Errors
/// [`Error::Io`] naming the file that could not be read or written.
pub(super) fn put(source: &Path, destination: &Path, _metadata: &Metadata) -> Result<Put> {
    let bytes = std::fs::copy(source, destination).map_err(Error::io(destination))?;
    Ok(Put { bytes, shared: false })
}
