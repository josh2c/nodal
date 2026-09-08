//! Activation and entry: how a person, a terminal and an agent get into a unit.
//!
//! A unit's home is a normal directory that carries its own environment (`crate::env`).
//! This module is everything that happens around that directory: which shell function
//! makes `nodal cd` move the shell a person is in ([`init`], [`entry`]), what
//! `nodal shell` does for a script or a second machine ([`shell`]), what `nodal run`
//! records ([`run`]), and how "who is in this unit" is answered without anybody
//! reporting it ([`sessions`]).
//!
//! Two rules shape all of it, and they are the same rule twice: Nodal does not take
//! over a person's shell, and Nodal does not ask a person anything on the way out.
//! So there is no subshell here. `nodal cd` writes a path into a file the
//! shell function opened and the shell changes its own directory. `nodal shell`
//! replaces this process with the shell rather than running one under it. A session
//! ends because a process is gone, not because something was asked.
//!
//! The same reading answers a second question. A process table that says who is attached
//! also says what is running and whose it is, which is [`attribute`] and the `nodal ps`
//! it composes into ([`ps`]).
//!
//! The two routes into an activated shell are file-based and both are written by
//! `crate::env::files`: the `.envrc` direnv reads, and `nodal env --export`, which the
//! prompt hook evaluates. They compose: the hook does nothing in a home direnv has
//! already activated, because `NODAL_ROOT` is then already the home.

pub mod actor;
pub mod attribute;
pub mod entry;
pub mod init;
pub mod ls;
pub mod processes;
pub mod ps;
pub mod run;
pub mod sessions;
pub mod shell;
pub mod shells;
pub mod show;
pub mod stop;

pub use crate::runtime::shells::Shell;
