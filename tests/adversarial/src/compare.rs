//! The two answers, put beside each other, and what each kind of difference is worth.
//!
//! Only one difference is a defect. Nodal says a home is safe to remove, the oracle says a
//! member of the contracted loss set has no proven copy outside it, and the difference is
//! work that disappears with the directory. That fails the grid.
//!
//! The other difference is a cost. Nodal refuses, the oracle finds a copy of everything, and
//! a person has to decide by hand. The project documents several of these on purpose — a
//! sibling three directories down, a store on a network mount, a reading whose two instants
//! fell in one second — so an over-refusal is printed and counted and never fails a run. A
//! grid that failed over them would be turned off inside a week, and a grid that is off finds
//! nothing.
//!
//! Occupancy is outside both. Something holding a home is a reason to wait, not a thing that
//! is lost, so a refusal over occupancy alone is no disagreement at all. The two shapes that
//! made occupancy a safety question are named cases in `tests/shapes.rs` instead, where the
//! refusal is asserted directly rather than inferred from the oracle.

use crate::build::Built;
use crate::check::{Check, Needs};
use crate::oracle::{Answer, Occupancy};
use crate::shape::Shape;

/// A difference between the two answers.
#[derive(Debug, Clone)]
pub enum Disagreement {
    /// Nodal would remove the home and the oracle says work goes with it. A defect.
    SafeAndLost {
        /// The shape it was found on.
        shape: Shape,
        /// What the oracle found no copy of.
        lost: String,
        /// The commands that make the shape again.
        script: String,
    },
    /// Nodal refuses and the oracle finds a copy of everything. A cost, and recorded.
    RefusedAndHeld {
        /// The shape it was found on.
        shape: Shape,
        /// The reason Nodal gave, in its own words.
        because: String,
        /// The commands that make the shape again.
        script: String,
    },
}

impl Disagreement {
    /// Whether this difference fails the run.
    #[must_use]
    pub const fn fails(&self) -> bool {
        matches!(self, Self::SafeAndLost { .. })
    }

    /// The shape it was found on.
    #[must_use]
    pub const fn shape(&self) -> Shape {
        match self {
            Self::SafeAndLost { shape, .. } | Self::RefusedAndHeld { shape, .. } => *shape,
        }
    }
}

impl std::fmt::Display for Disagreement {
    /// The whole of what a reader needs: which way it went, on what shape, and the script.
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SafeAndLost { shape, lost, script } => {
                write!(out, "FAIL {shape}\n  nodal: safe to reclaim\n  oracle: {lost}\n{script}\n")
            }
            Self::RefusedAndHeld { shape, because, script } => write!(
                out,
                "over-refusal {shape}\n  nodal: {because}\n  oracle: every member has a proven copy\n{script}\n"
            ),
        }
    }
}

/// Whether the oracle saw something holding the home.
///
/// Counted by the report and never a failure. The module note says why.
#[must_use]
pub fn was_held(oracle: &Answer) -> bool {
    matches!(oracle.occupancy, Occupancy::Held(_))
}

/// Compare the two answers about one shape.
///
/// `None` is agreement, which is what nearly every shape answers.
#[must_use]
pub fn compare(
    shape: Shape,
    built: &Built,
    check: &Check,
    oracle: &Answer,
) -> Option<Disagreement> {
    let script = built.reproduction(shape);
    if check.safe_to_reclaim && oracle.loses() {
        return Some(Disagreement::SafeAndLost { shape, lost: lost(oracle), script });
    }
    if !check.safe_to_reclaim && !oracle.loses() && !only_occupancy(check) {
        return Some(Disagreement::RefusedAndHeld { shape, because: because(check), script });
    }
    None
}

/// What the oracle found no copy of, in one line.
fn lost(oracle: &Answer) -> String {
    let mut said = Vec::new();
    if !oracle.unproved.is_empty() {
        let named: Vec<String> = oracle
            .unproved
            .iter()
            .map(|one| {
                format!("{} (reached by {})", &one.oid[..8.min(one.oid.len())], one.refs.join(", "))
            })
            .collect();
        said.push(format!("{} commits have no proven copy: {}", named.len(), named.join("; ")));
    }
    if !oracle.tree.is_empty() {
        said.push(format!("the working tree holds {}", oracle.tree.join(", ")));
    }
    said.push(format!("{} stores were asked", oracle.asked.len()));
    said.join("; ")
}

/// Why Nodal refused, in its own words.
fn because(check: &Check) -> String {
    let said: Vec<String> = check
        .reasons
        .iter()
        .map(|reason| format!("{:?}: {}", reason.needs, reason.detail))
        .collect();
    if said.is_empty() { String::from("refused with no reason printed") } else { said.join("; ") }
}

/// Whether every reason Nodal gave is about something running, which the oracle does not judge.
fn only_occupancy(check: &Check) -> bool {
    !check.reasons.is_empty()
        && check.reasons.iter().all(|reason| reason.needs == Needs::BlockingRuntime)
}

/// What one shape answered: its name, the difference if there was one, and whether the host
/// built the whole of it.
///
/// A shape's answer is three things and they travel together, so they are one type. The name
/// is carried rather than the shape, because the report is ordered by name and an order that
/// depends on which core finished first is an order that changes between runs.
#[derive(Debug)]
pub struct Asked {
    /// The shape, by name.
    pub name: String,
    /// The difference between the two answerers, if there was one.
    pub difference: Option<Disagreement>,
    /// Why the host did not build the whole shape, if it did not.
    pub skipped: Option<String>,
    /// Whether the oracle saw something holding the home.
    pub held: bool,
}

/// Every disagreement of a run, and the line the run ends with.
#[derive(Debug, Default)]
pub struct Found {
    /// Each difference, in the order the shapes ran.
    pub all: Vec<Disagreement>,
    /// The shapes a host would not build, with the reason each time.
    pub skipped: Vec<String>,
    /// How many shapes were asked.
    pub asked: usize,
    /// How many of them the oracle saw something holding.
    pub held: usize,
}

impl Found {
    /// Take what one shape answered.
    pub fn take(&mut self, asked: Asked) {
        self.asked += 1;
        self.held += usize::from(asked.held);
        if let Some(why) = asked.skipped {
            self.skipped.push(why);
        }
        if let Some(one) = asked.difference {
            self.all.push(one);
        }
    }

    /// The differences that fail the run.
    #[must_use]
    pub fn failures(&self) -> Vec<&Disagreement> {
        self.all.iter().filter(|one| one.fails()).collect()
    }

    /// The whole report: every difference, then one line of counts.
    ///
    /// Printed whether or not the run failed. A run that printed nothing when it agreed would
    /// leave nobody able to say how much it asked, and a grid nobody can size is a grid
    /// nobody trusts.
    #[must_use]
    pub fn report(&self, size: &str) -> String {
        let mut lines: Vec<String> = self.all.iter().map(ToString::to_string).collect();
        for why in distinct(&self.skipped) {
            lines.push(format!("SKIPPED: {why}"));
        }
        lines.push(format!(
            "adversarial ({size}): {} shapes, {} false-safe, {} over-refusals, {} the oracle saw held",
            self.asked,
            self.failures().len(),
            self.all.len() - self.failures().len(),
            self.held
        ));
        lines.join("\n")
    }
}

/// The reasons a host gave, each said once however many shapes it stopped.
fn distinct(said: &[String]) -> Vec<String> {
    let mut each: Vec<String> = said.to_vec();
    each.sort_unstable();
    each.dedup();
    each
}
