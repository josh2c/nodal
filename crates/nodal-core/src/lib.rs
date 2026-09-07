//! Core library for Nodal.
//!
//! Every behaviour lives here; the `nodal` binary only parses arguments and prints
//! (`docs/code-structure.md`). This file re-exports module facades and holds no logic.

pub mod env;
pub mod error;
pub mod fingerprint;
pub mod git;
pub mod lifecycle;
pub mod logging;
pub mod model;
pub mod output;
pub mod recipe;
pub mod services;
pub mod store;

pub use crate::error::{Error, Result};

/// Version of this library, as published in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
