//! The safety suite: what makes one unit of a project independent of every other one.
//!
//! Nodal's promise is that two working copies of one project cannot interfere. Every
//! other property is a convenience; this one is the product. A person runs two agents
//! against one repository at once because Nodal says the second cannot reach the first,
//! and the day that stops being true is the day the tool takes work away instead of
//! giving it.
//!
//! A promise of that kind is not held by a review. It is held by a test that fails, so
//! each row of the interference table below is one named test in this crate, on the
//! fixture project, run by its own CI job on both platforms Nodal ships for. A reviewer
//! asked "what stops a write in one unit from reaching another" has a test name to
//! answer with.
//!
//! | property | what a break would look like | test |
//! |---|---|---|
//! | source isolation | a file written in one unit appears in another, or in the person's checkout | `tests/source_isolation.rs` |
//! | git isolation | a branch, a stash or a ref made in one unit is visible in another | `tests/git_isolation.rs` |
//! | port isolation | two units are granted one port and race for the socket | `tests/port_isolation.rs` |
//! | base immutability | a unit writes into the tree every other unit is cloned from | `tests/base_immutability.rs` |
//! | tracked excludes | a copy leaves out a path the project tracks and is dirty at birth | `tests/tracked_exclude.rs` |
//! | reclaim refusal | a reclaim removes a home holding work that exists nowhere else | `tests/reclaim_refusal.rs` |
//! | doctor reads only | a read command changes the machine it is reporting on | `tests/doctor_writes_nothing.rs` |
//!
//! ## How the properties are asserted
//!
//! Through the binary, in a temporary machine of its own ([`Machine`]). A safety
//! property is a claim about what a person gets when they type `nodal new`, so the
//! suite types it. Nothing here reads or writes the state directory, the per-machine
//! secrets file or the hook approvals of whoever is running the tests.
//!
//! Two properties are also asserted below the binary, because the binary cannot state
//! them. Concurrency needs many callers at one instant, so the port suite opens one
//! registry connection per thread, as `crates/nodal-core/tests/ports.rs` does; and
//! "nothing changed" needs the bytes on both sides, so [`Snapshot`] holds them.
//!
//! ## The linked temporary directory
//!
//! `ci/acceptance-safety.sh` runs the whole suite a second time with `TMPDIR` reached
//! through a symbolic link. macOS names `/var/folders` and means `/private/var/folders`,
//! so every path in a test arrives under two names there, and a comparison between the
//! two is false unless something resolved them. Linux has no such link, so the condition
//! is made rather than waited for, and both hosts check it.
//!
//! ## What is not here
//!
//! Isolation of the database and of the per-unit services. Those are the second part of
//! the suite: they need a container daemon and a Postgres instance, which is a different
//! CI job with different prerequisites.

#![allow(
    clippy::expect_used,
    reason = "a machine that cannot be built fails the property it was built for"
)]

pub mod machine;
pub mod platform;
pub mod tree;

pub use machine::{Machine, git, stderr, stdout, try_git};
pub use tree::Snapshot;
