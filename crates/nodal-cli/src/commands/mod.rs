//! One file per command: read the arguments, call `nodal-core`, print the result.
//!
//! No command holds behaviour of its own. Anything a second command would need lives in
//! the library, so that `--json` output and the human form are two renderings of one
//! value rather than two code paths.

pub mod env;
pub mod init;
pub mod new;
