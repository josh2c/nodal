//! The test kit of the workspace, and the safety suite that is built on it.
//!
//! Two things live here.
//!
//! The kit is what every suite in the workspace needs before it can assert anything:
//!
//! | module | what it is |
//! |---|---|
//! | [`runner`] | a command for the `nodal` binary, with a state directory of its own |
//! | [`mod@git`] | one `git` call in a directory, with the machine's configuration shut out |
//! | [`text`] | the two streams of a finished command, as text |
//! | [`project`] | a project to make units in, and the state directory they go in |
//! | [`state`] | what a state directory holds, read back after a command has run |
//! | [`rows`] | the registry rows a test writes by hand |
//! | [`activation`] | one activated home: the three files a shell reads |
//! | [`checkout`] | a checkout Nodal holds nothing about, and the readings a verdict takes of one |
//! | [`process`] | a process a test starts, and the two shapes it starts one in |
//!
//! Each of those was written out again in every file that wanted it. The binary runner
//! stood in nineteen test files and the `git` runner in sixteen, so a fix to one of them
//! reached one suite. They are one implementation now, and each module says what its
//! callers had in common.
//!
//! The safety suite is the rest of this crate, and the paragraphs below are about it.
//!
//! ## The safety suite: what makes one unit of a project independent of every other one
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
//! | reclaim scope | a reclaim signals a process that carries no unit identifier | `tests/reclaim_scope.rs` |
//! | hook process ownership | a recipe hook leaves a process running that nothing on the machine can name or stop | `tests/hook_processes.rs` |
//! | trash prune | the trash loses a path that holds work, or keeps a build a tool writes again | `tests/trash_prune.rs` |
//! | a session's home | a file nodal wrote for an agent shows as the agent's work, and is merged as it | `tests/hook_home.rs` |
//! | hook scope | an install nobody asked for leaves a file in a repository, or the provider ends a session in a project that is not Nodal's | `tests/claude_scope.rs` |
//! | doctor reads only | a read command changes the machine it is reporting on | `tests/doctor_writes_nothing.rs` |
//! | stand-in values | a create calls a unit ready with a generated value its own generate step needs | `tests/stand_in_values.rs` |
//! | a kept clone | a failed install throws away the clone it was installing into | `tests/base_retry.rs` |
//! | a reason with every error | a tool fails and the message does not say what it wrote | `tests/base_retry.rs` |
//! | a pin acted on | a host installs at a version the project did not ask for | `tests/base_pin.rs` |
//! | one clone at any worker count | the threads a copy runs on change the tree it makes | `tests/clone_identity.rs` |
//! | one registry per host | a second account cannot open the list, or two clones of one remote make two projects | `tests/shared_host.rs` |
//! | secrets stay their owner's | a second account entering a home reads the first's credentials | `tests/shared_host.rs` |
//! | approvals stay their owner's | one person's reading of a hook decides what runs as another | `tests/shared_host.rs` |
//! | one writer per home | two actors write one home and neither is told the other is there | `tests/one_writer.rs` |
//! | an advisory lock | a held unit stops an editor, a `git` call or a read command | `tests/one_writer.rs` |
//!
//! ## How the properties are asserted
//!
//! Through the binary, in a temporary machine of its own ([`Machine`]). A safety
//! property is a claim about what a person gets when they type `nodal new`, so the
//! suite types it. Nothing here reads or writes the state directory, the per-machine
//! secrets file or the hook approvals of whoever is running the tests.
//!
//! Three properties are also asserted below the binary, because the binary cannot state
//! them. Concurrency needs many callers at one instant, so the port suite opens one
//! registry connection per thread, as `crates/nodal-core/tests/ports.rs` does; "nothing
//! changed" needs the bytes on both sides, so [`Snapshot`] holds them; and a worker
//! count is not something `nodal new` takes, so the clone-identity suite calls the
//! copier with one.
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
    reason = "a fixture that cannot be built fails the test it was built for"
)]

pub mod activation;
pub mod checkout;
pub mod git;
pub mod machine;
pub mod platform;
pub mod process;
pub mod project;
pub mod rows;
pub mod runner;
pub mod state;
pub mod text;
pub mod tree;

pub use git::{git, git_ok, try_git};
pub use machine::Machine;
pub use project::Workspace;
pub use state::InState;
pub use text::{answer, json, stderr, stdout};
pub use tree::Snapshot;
