//! One file per command: read the arguments, call `nodal-core`, print the result.
//!
//! No command holds behaviour of its own. Anything a second command would need lives in
//! the library, so that `--json` output and the human form are two renderings of one
//! value rather than two code paths.

pub mod adopt;
pub mod approve;
pub mod base;
pub mod cd;
pub mod claude_code;
pub mod context;
pub mod doctor;
pub mod done;
pub mod env;
pub mod explain;
pub mod gc;
pub mod handoff;
pub mod init;
pub mod ls;
pub mod mcp;
pub mod merge;
pub mod new;
pub mod ps;
pub mod reclaim;
pub mod run;
pub mod shell;
pub mod shell_init;
pub mod show;
pub mod uninstall;
pub mod upgrade;

/// Write a rendered answer to standard output.
///
/// Every command renders its value once ([`nodal_core::output::render`]) and hands the
/// text here, so the bytes a person sees and the bytes a tool reads are the same bytes.
///
/// # Errors
///
/// [`nodal_core::Error::Io`] when standard output refused them.
pub fn emit(text: &str) -> nodal_core::Result<()> {
    use std::io::Write as _;

    std::io::stdout().write_all(text.as_bytes()).map_err(nodal_core::Error::io("<stdout>"))
}
