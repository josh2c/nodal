//! The resources a unit needs beside its files, and who holds each of them.
//!
//! A unit is a branch with a home directory, and the moment two of them run at once
//! they compete for things a host has only one of: a port, a database, a container
//! name. This module hands those out and takes them back, and it answers the question
//! a person asks when two units fight over one of them — who holds it.
//!
//! [`ports`] is the allocator: a project gets a block of ports, an environment gets
//! ports from that block, and reclaim gives them back. [`listeners`] reads what is
//! actually bound on this host, so a port an environment was granted and a port an
//! environment uses are two answers, not one assumption. [`docker`] is the one place
//! that spawns the `docker` binary, so a machine without a daemon is a reason a caller
//! can print rather than a failure it has to handle.

pub mod docker;
pub mod listeners;
pub mod ports;
