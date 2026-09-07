//! Core library for Nodal.
//!
//! Every behaviour lives here; the `nodal` binary only parses arguments and prints
//! (`docs/code-structure.md`). This file re-exports module facades and holds no logic.

pub mod error;
pub mod logging;

pub use crate::error::{Error, Result};

/// Version of this library, as published in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
