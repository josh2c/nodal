//! Install and remove the shell integration, and say how to upgrade this binary.
//!
//! Three things live here, and they are the same subject read three ways: what Nodal
//! puts on a machine outside its own state directory, how it takes that back, and what
//! a person types to get a newer Nodal.
//!
//! - [`rc`] is the managed block: the lines Nodal writes into a start-up file, and the
//!   rule that takes them out again. The rule is exact. A file that had the block
//!   appended and then removed is byte-for-byte the file it was before, because the
//!   block always starts with one newline and removal always takes that newline with
//!   it. `crates/nodal-cli/tests/uninstall.rs` asserts the round trip on real files.
//! - [`shims`] is the shell script itself, written once into the state directory. The
//!   start-up file sources that file; it does not evaluate the output of a command. A
//!   prompt hook runs in every shell a person opens, so the line that loads it is the
//!   most security-sensitive line Nodal writes anywhere.
//! - [`channel`] answers `nodal upgrade`. Nodal has no self-updater and makes no
//!   network call of its own. It reads where its own binary is, names the
//!   channel that put it there, and prints the one command that upgrades it. It
//!   fetches nothing.
//!
//! Nothing here is a lifecycle operation with an undo, because nothing here writes into
//! a unit. The one destructive path — removing the state directory — asks
//! [`crate::lifecycle::uniqueness`] about every home first, like every other
//! destructive path in Nodal ([`plan`]).

pub mod channel;
pub mod plan;
pub mod rc;
pub mod shims;

pub use crate::setup::channel::Channel;
