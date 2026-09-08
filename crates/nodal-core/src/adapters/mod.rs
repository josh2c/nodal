//! The coding agents Nodal integrates with, and what it may write for each.
//!
//! An adapter is the one place that knows another tool's file format. Everything it
//! writes is outside Nodal's own state directory and inside somebody's repository, so
//! two rules hold for every adapter here and are tested for each:
//!
//! - **nothing of the person's is clobbered.** A file that is already there is read,
//!   added to, and written back with every other key it held; a hook somebody else
//!   installed is left exactly as it is.
//! - **removal is the inverse of installation.** What `nodal init` put in,
//!   `nodal uninstall` takes out, and a file that holds anything else comes back byte
//!   for byte the file it was ([`settings`]).
//!
//! [`claude_code`] is the only adapter so far. It is the larger half of the subject: an
//! agent that hands unit creation to Nodal rather than one that reads a file Nodal
//! wrote.

pub mod claude_code;
pub mod settings;
