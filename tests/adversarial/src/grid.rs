//! The three sizes of the grid, and the rule that keeps the smallest one honest.
//!
//! The full grid is the product of the five axes. It is too long for a pull request and it
//! is the right size for a nightly run, so a pull request gets a sample of it.
//!
//! A sample has to hold two properties or it is worth nothing:
//!
//! 1. **It is the same sample every run.** A grid that drew at random would turn a defect
//!    into a flake: the shape that fails appears one run in a hundred, the job is restarted,
//!    the run is green, and nobody ever sees it again. So the sample is a function of a
//!    constant ([`SEED`]) and of nothing else — not the clock, not the host, not the order
//!    the tests run in.
//! 2. **It holds every value of every axis.** Ten witness topologies and a sample that
//!    happened to miss the blobless one is a sample that would have missed FS-14. The draw
//!    alone does not promise that, so the draw is followed by a repair: every value no drawn
//!    shape holds gets one shape of its own, drawn from the same stream.
//!
//! Both are asserted in [`tests`], over the sample the pull-request job really runs.
//!
//! ## Which size runs
//!
//! `NODAL_ADVERSARIAL` selects it, and [`asked`] reads it once:
//!
//! | value | what runs |
//! |---|---|
//! | unset | the sample |
//! | `sample` | the sample |
//! | `full` | every shape the axes make |
//!
//! Any other value is a mistake a person made, and it stops the run rather than quietly
//! choosing for them.

use crate::shape::{Axis as _, Observed, Occupant, Refs, Shape, Tree, Witness};

/// The constant the sample is drawn from.
///
/// Its value means nothing. What matters is that it never changes: the day it changes, the
/// sample is a different sixty shapes, and a defect the old sample held is released.
pub const SEED: u64 = 0x_0110_1071_0000_0007;

/// How many shapes are drawn before the repair adds the values the draw missed.
///
/// Sixty is a reading and not a rule. It is the number at which the draw covers all
/// thirty-one axis values on this seed with the repair adding nothing, and it is under the
/// two minutes the pull-request job is meant to take. The invariant is the coverage, which
/// [`sample`] holds at any size.
pub const SAMPLE: usize = 60;

/// The variable that selects a size.
pub const VARIABLE: &str = "NODAL_ADVERSARIAL";

/// Every shape the five axes make, in index order.
#[must_use]
pub fn full() -> Vec<Shape> {
    (0..Shape::COUNT).map(at).collect()
}

/// The shapes a run was asked for, and the name of the size, for the line that reports it.
///
/// # Panics
///
/// If `NODAL_ADVERSARIAL` holds a word this function does not know. A misspelled `full` that
/// quietly ran the sample would report a nightly run that never happened.
#[must_use]
pub fn asked() -> (Vec<Shape>, &'static str) {
    match std::env::var(VARIABLE).unwrap_or_default().as_str() {
        "" | "sample" => (sample(SEED, SAMPLE), "sample"),
        "full" => (full(), "full"),
        other => panic!("{VARIABLE}={other} is not a size; it is one of `sample` or `full`"),
    }
}

/// The shape at one index of the full grid.
///
/// The index is taken apart one axis at a time, least significant axis first, so index 0 is
/// the first value of every axis and the mapping never moves when a value is appended to an
/// axis.
#[must_use]
pub fn at(index: usize) -> Shape {
    let mut left = index % Shape::COUNT;
    Shape {
        occupant: pick(&mut left, Occupant::ALL),
        tree: pick(&mut left, Tree::ALL),
        observed: pick(&mut left, Observed::ALL),
        refs: pick(&mut left, Refs::ALL),
        witness: pick(&mut left, Witness::ALL),
    }
}

/// Take one axis off an index, and leave the rest of the index behind.
fn pick<A: Copy>(left: &mut usize, values: &[A]) -> A {
    let taken = values[*left % values.len()];
    *left /= values.len();
    taken
}

/// A deterministic sample of the full grid that holds every value of every axis.
///
/// `wanted` shapes are drawn, and then the repair adds one shape for each axis value the
/// draw missed, so the answer may be a little longer than `wanted` and is never shorter than
/// the longest axis.
#[must_use]
pub fn sample(seed: u64, wanted: usize) -> Vec<Shape> {
    let mut stream = Stream::from(seed);
    let mut drawn: Vec<Shape> = Vec::with_capacity(wanted);
    while drawn.len() < wanted.min(Shape::COUNT) {
        let shape = at(stream.next_index());
        if !drawn.contains(&shape) {
            drawn.push(shape);
        }
    }
    repair(&mut drawn, &mut stream);
    drawn
}

/// Add a shape for every axis value the draw did not hold.
///
/// The added shape is drawn from the same stream and then has the one value forced, so the
/// other four axes are as varied as the draw is and the addition is still a function of the
/// seed.
fn repair(drawn: &mut Vec<Shape>, stream: &mut Stream) {
    for value in Witness::ALL {
        add(drawn, stream, |shape| shape.witness == *value, |shape| shape.witness = *value);
    }
    for value in Refs::ALL {
        add(drawn, stream, |shape| shape.refs == *value, |shape| shape.refs = *value);
    }
    for value in Observed::ALL {
        add(drawn, stream, |shape| shape.observed == *value, |shape| shape.observed = *value);
    }
    for value in Tree::ALL {
        add(drawn, stream, |shape| shape.tree == *value, |shape| shape.tree = *value);
    }
    for value in Occupant::ALL {
        add(drawn, stream, |shape| shape.occupant == *value, |shape| shape.occupant = *value);
    }
}

/// Draw one shape and force one axis on to it, unless something drawn already holds it.
fn add(
    drawn: &mut Vec<Shape>,
    stream: &mut Stream,
    holds: impl Fn(&Shape) -> bool,
    force: impl Fn(&mut Shape),
) {
    if drawn.iter().any(holds) {
        return;
    }
    let mut shape = at(stream.next_index());
    force(&mut shape);
    drawn.push(shape);
}

/// The draw itself: `splitmix64`, which is eleven lines and has no state but its seed.
///
/// A generator from a crate would do as well. This one is here because the sample must be
/// the same sequence on both hosts and in every release of every dependency, for as long as
/// the seed does not change, and eleven lines in the repository say that without a version
/// to pin.
struct Stream {
    /// The state, which is the seed advanced once per draw.
    state: u64,
}

impl From<u64> for Stream {
    fn from(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl Stream {
    /// The next index into the full grid.
    fn next_index(&mut self) -> usize {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut word = self.state;
        word = (word ^ (word >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        word = (word ^ (word >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        word ^= word >> 31;
        usize::try_from(word % Shape::COUNT as u64).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{SAMPLE, SEED, at, full, sample};
    use crate::shape::{Axis as _, Observed, Occupant, Refs, Shape, Tree, Witness};

    /// The sample is a function of the seed. Two calls are the same list, in the same order.
    ///
    /// This is the property that keeps a failing shape failing. Without it a red run is
    /// restarted green and the defect is released.
    #[test]
    fn the_sample_is_the_same_list_every_time() {
        assert_eq!(sample(SEED, SAMPLE), sample(SEED, SAMPLE));
    }

    /// Every value of every axis is in the sample the pull-request job runs.
    ///
    /// A sample that missed the blobless clone would have missed FS-14, and a sample that
    /// missed the prune-less fetch would have missed FS-1.
    #[test]
    fn the_sample_holds_every_value_of_every_axis() {
        let drawn = sample(SEED, SAMPLE);
        let missing = |axis: &str, held: BTreeSet<&str>, all: Vec<&str>| {
            let absent: Vec<&str> = all.into_iter().filter(|value| !held.contains(value)).collect();
            assert!(absent.is_empty(), "the sample holds no {axis} {absent:?}");
        };
        missing(
            Witness::AXIS,
            drawn.iter().map(|shape| shape.witness.label()).collect(),
            Witness::ALL.iter().map(|value| value.label()).collect(),
        );
        missing(
            Refs::AXIS,
            drawn.iter().map(|shape| shape.refs.label()).collect(),
            Refs::ALL.iter().map(|value| value.label()).collect(),
        );
        missing(
            Observed::AXIS,
            drawn.iter().map(|shape| shape.observed.label()).collect(),
            Observed::ALL.iter().map(|value| value.label()).collect(),
        );
        missing(
            Tree::AXIS,
            drawn.iter().map(|shape| shape.tree.label()).collect(),
            Tree::ALL.iter().map(|value| value.label()).collect(),
        );
        missing(
            Occupant::AXIS,
            drawn.iter().map(|shape| shape.occupant.label()).collect(),
            Occupant::ALL.iter().map(|value| value.label()).collect(),
        );
    }

    /// No shape is drawn twice. A sample of sixty that held one shape three times would run
    /// fifty-eight.
    #[test]
    fn the_sample_holds_no_shape_twice() {
        let drawn = sample(SEED, SAMPLE);
        let distinct: BTreeSet<String> = drawn.iter().map(|shape| shape.name()).collect();
        assert_eq!(distinct.len(), drawn.len(), "a shape is in the sample twice");
    }

    /// The index mapping is one to one over the whole grid, so the full grid is every shape
    /// once and the draw cannot reach a shape twice under two indices.
    #[test]
    fn the_full_grid_is_every_shape_exactly_once() {
        let every = full();
        assert_eq!(every.len(), Shape::COUNT);
        let distinct: BTreeSet<String> = every.iter().map(|shape| shape.name()).collect();
        assert_eq!(distinct.len(), Shape::COUNT, "two indices name one shape");
    }

    /// An index past the end of the grid wraps rather than panicking, because the draw takes
    /// a word from the stream and a word is bigger than the grid.
    #[test]
    fn an_index_past_the_end_names_a_shape() {
        assert_eq!(at(Shape::COUNT), at(0));
    }
}
