//! The grid: every shape of the size this run was asked for, both answerers, one report.
//!
//! One test and not one per shape. A shape is not a property a reviewer cites; it is a draw
//! from a space, and what a reader needs is the whole of what the run asked and every
//! difference it found. So the run prints a report and fails on the differences that are
//! defects, and the report is printed once: by the panic where the run failed, and on
//! standard output where it did not.
//!
//! ```text
//! adversarial (sample): 60 shapes, 0 false-safe, 4 over-refusals
//! ```
//!
//! ## Which size, and where
//!
//! | size | how it is asked for | where it runs |
//! |---|---|---|
//! | sample | nothing, or `NODAL_ADVERSARIAL=sample` | `acceptance (adversarial)`, both hosts, blocks a pull request |
//! | full | `NODAL_ADVERSARIAL=full` | the nightly workflow, and a person's own machine |
//!
//! ## What fails it
//!
//! One difference: Nodal says safe and the oracle says a member of the contracted loss set
//! has no proven copy. Everything else is printed and counted.
//! `crate::compare` states why at length.
//!
//! ## How long it takes
//!
//! Every shape is a machine of its own: a fixture project, a bare remote, a state directory
//! and a home that `nodal new` clones. Nothing is shared between two shapes, because a
//! store one shape built is a store the next shape's verdict would rest on. So the run
//! costs one create per shape, and it is spread over the cores the host has.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use nodal_adversarial::compare::{Asked, Found};
use nodal_adversarial::{build, compare, grid, shape::Shape};

/// One shape, asked of both answerers.
fn ask(shape: Shape) -> Asked {
    let built = build::build(shape);
    let skipped = built.skipped.clone();
    let check = built.check();
    let oracle = built.oracle();
    let difference = compare::compare(shape, &built, &check, &oracle);
    Asked { name: shape.name(), difference, skipped, held: compare::was_held(&oracle) }
}

/// Every shape of the size this run was asked for, over the cores the host has.
///
/// The answers are ordered by shape name before the report is made, so the report is the
/// same text however the cores finished.
fn over(shapes: &[Shape]) -> Found {
    let next = AtomicUsize::new(0);
    let taken: Mutex<Vec<Asked>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, Into::into);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(shape) = shapes.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let answered = ask(*shape);
                    taken.lock().unwrap().push(answered);
                }
            });
        }
    });
    let mut each = taken.into_inner().unwrap();
    each.sort_by(|left, right| left.name.cmp(&right.name));
    let mut found = Found::default();
    for answered in each {
        found.take(answered);
    }
    found
}

/// The invariant. On every shape the grid was asked for, the predicate never calls a home
/// safe to remove that the oracle says holds work with no proven copy.
#[test]
fn no_shape_is_called_safe_while_the_oracle_says_work_goes_with_it() {
    let (shapes, size) = grid::asked();
    let found = over(&shapes);
    let report = found.report(size);
    assert_eq!(found.asked, shapes.len(), "a shape was not asked: {report}");
    assert!(found.failures().is_empty(), "{report}");
    println!("{report}");
}
