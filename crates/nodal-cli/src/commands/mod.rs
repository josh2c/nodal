//! One file per command: read the arguments, call `nodal-core`, print the result.
//!
//! No command holds behaviour of its own. Anything a second command would need lives in
//! the library, so that `--json` output and the human form are two renderings of one
//! value rather than two code paths.

pub mod base;
pub mod cd;
pub mod doctor;
pub mod done;
pub mod env;
pub mod gc;
pub mod init;
pub mod ls;
pub mod merge;
pub mod new;
pub mod ps;
pub mod reclaim;
pub mod run;
pub mod shell;
pub mod shell_init;
