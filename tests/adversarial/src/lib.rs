//! The proof protocol as a test: compose a shape, build it, and ask two independent
//! answerers the same question.
//!
//! Every false-safe reading this project has found was found by a person, after the code
//! shipped. Seven of them were found in one week of reading (`FS-1` to `FS-15`), and each
//! one was a *shape*: a combination of a witness topology, a ref, a remote observation, a
//! working tree and something running. None of them needed a strange machine. Each one
//! was a combination nobody had composed.
//!
//! So the combinations are composed here, and the answer is checked against an answerer
//! that shares no code with the one under test.
//!
//! ## The two answerers
//!
//! | answerer | what it is | where |
//! |---|---|---|
//! | the predicate | `nodal reclaim --check --json`, the binary a person types | [`check`] |
//! | the oracle | `git` plumbing, `/proc` and `lsof`, and the contract read as rules | [`oracle`] |
//!
//! The oracle is not a second implementation of Nodal. It reads
//! `docs/research/boundary-2026-09-22/safety-contract-draft.md` §1 to §3 as a procedure
//! over Git's own records: which refs a home holds, which store outside it holds every
//! object those refs reach, which dated `FETCH_HEAD` line saw the work, and what the
//! working tree says. It shares no function with the crate it checks, so a wrong rule in
//! one of them cannot be wrong in the same direction in the other.
//!
//! ## What fails, and what is only recorded
//!
//! One direction is a defect and one is a cost.
//!
//! - **Nodal says safe and the oracle says a member of the loss set has no proven copy.**
//!   That is work lost, and it fails the test.
//!   ([`compare::Disagreement::SafeAndLost`])
//! - **Nodal refuses and the oracle says every member has a copy.** That is a reclaim a
//!   person has to do by hand. It is recorded and printed, and it does not fail.
//!   ([`compare::Disagreement::RefusedAndHeld`])
//!
//! Occupancy sits outside both. A process holding a home is not a member of the loss set;
//! it is a reason to wait. So an occupancy disagreement is recorded in the grid and never
//! fails it, and the two shapes that made occupancy a safety question — a process this
//! account may not read, and a process writing from a directory elsewhere — are named
//! cases in `tests/shapes.rs` that assert the refusal directly.
//!
//! ## The five axes
//!
//! [`shape::Shape`] is one value from each. The axes are the five kinds of evidence the
//! predicate reads, and every known defect was a value of one of them meeting a value of
//! another:
//!
//! | axis | values | the shape it was found by |
//! |---|---|---|
//! | [`shape::Witness`] | ten topologies of the store outside the home | FS-14, the blobless clone |
//! | [`shape::Refs`] | six places a home can hold work | FS-2, the side branch |
//! | [`shape::Observed`] | six ways a remote was or was not seen | FS-1, the prune-less fetch |
//! | [`shape::Tree`] | four states of the working tree | the ignored-only home |
//! | [`shape::Occupant`] | five things that can be running | FS-6 and FS-8 |
//!
//! ## The three sizes
//!
//! | size | shapes | where it runs | blocks a pull request |
//! |---|---|---|---|
//! | the named shapes | `tests/shapes.rs` | every run, and in `cargo test --workspace` | yes |
//! | the sample | [`grid::sample`], a fixed seed | `acceptance (adversarial)`, both hosts | yes |
//! | the full grid | [`grid::full`] | `NODAL_ADVERSARIAL=full`, and the nightly workflow | no |
//!
//! The sample is deterministic and covers every value of every axis at least once, and
//! `grid::tests` asserts both. A sample that changed between runs would turn a defect into
//! a flake, and a defect that appears one run in ten is a defect nobody fixes.
//!
//! ## Reproduction
//!
//! A grid that says "shape 2041 disagreed" is a grid nobody can act on. So the builder
//! records the commands it ran, in order, as it runs them ([`build::Built::script`]), and
//! a disagreement prints them. What the test output holds is a shell script that makes the
//! shape again.

#![allow(
    clippy::expect_used,
    reason = "a shape that cannot be built fails the shape it was built for"
)]

pub mod build;
pub mod check;
pub mod compare;
pub mod grid;
pub mod oracle;
pub mod shape;

pub use build::Built;
pub use check::Check;
pub use compare::Disagreement;
pub use shape::Shape;
