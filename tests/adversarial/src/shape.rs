//! The five axes, and one value from each.
//!
//! Each axis is an enum and not a string, so a shape cannot name a value the builder does
//! not build, and the grid cannot lose a value by spelling it wrong. `ALL` on each axis is
//! the whole of it: [`grid`](crate::grid) multiplies those lists, so a value added to an
//! enum is in the full grid and in the coverage rule the same day it is added.
//!
//! The names are what a disagreement prints and what a person greps for, so each value
//! answers [`Axis::label`] with the words the contract uses for it.

/// One value of one axis, as the grid and the reports name it.
///
/// The three members are what every axis owes a reader: the whole of the axis, so the
/// grid can multiply it; a label, so a shape has a name; and the axis's own name, so a
/// coverage report says which axis a missing value belongs to.
pub trait Axis: Copy + Eq + 'static {
    /// Every value of this axis, in a fixed order.
    const ALL: &'static [Self];

    /// What this axis is called.
    const AXIS: &'static str;

    /// What this value is called.
    fn label(self) -> &'static str;
}

/// The topology of the store outside the home that is offered as the copy.
///
/// Four of these hold the commit and are not a copy of the work, and that is the whole of
/// FS-14: a store that answers every reachability question and holds none of the content.
/// The contract calls a store that exists *eligible* and a store that passes all four
/// checks *proven*, and these are the topologies that come apart between the two words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Witness {
    /// An ordinary clone of the home, with every object.
    FullClone,
    /// A bare clone of the home. No working tree, every object.
    Bare,
    /// A linked worktree of a clone of the home. A second repository at a second path.
    WorktreeElsewhere,
    /// A linked worktree of the home's own repository. The removal takes its objects.
    WorktreeOfHome,
    /// A shallow clone whose cut is above the work, so the work is not in it.
    ShallowAbove,
    /// A shallow clone whose cut is below the work, so the work is in it and its history
    /// is not.
    ShallowBelow,
    /// A `--filter=blob:none` clone. Every commit, no content.
    BloblessPartial,
    /// A clone that borrows its objects from a repository outside the home.
    AlternatesOutside,
    /// A clone that borrows its objects from the home itself.
    AlternatesIntoHome,
    /// Nothing beside the checkout at all.
    Nothing,
}

impl Axis for Witness {
    const ALL: &'static [Self] = &[
        Self::FullClone,
        Self::Bare,
        Self::WorktreeElsewhere,
        Self::WorktreeOfHome,
        Self::ShallowAbove,
        Self::ShallowBelow,
        Self::BloblessPartial,
        Self::AlternatesOutside,
        Self::AlternatesIntoHome,
        Self::Nothing,
    ];
    const AXIS: &'static str = "witness";

    fn label(self) -> &'static str {
        match self {
            Self::FullClone => "full-clone",
            Self::Bare => "bare",
            Self::WorktreeElsewhere => "worktree-elsewhere",
            Self::WorktreeOfHome => "worktree-of-home",
            Self::ShallowAbove => "shallow-above",
            Self::ShallowBelow => "shallow-below",
            Self::BloblessPartial => "blobless-partial",
            Self::AlternatesOutside => "alternates-outside",
            Self::AlternatesIntoHome => "alternates-into-home",
            Self::Nothing => "no-witness",
        }
    }
}

/// Where in the home the work is held.
///
/// The reading that walked `HEAD` alone reported `commits: []` over five of these six and
/// called the home safe. Each value writes the same content; only the ref differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refs {
    /// A branch the home is not checked out on.
    Branch,
    /// A tag, and no branch on the commit.
    Tag,
    /// `refs/stash`, over a tracked file.
    Stash,
    /// A record under `refs/nodal/`, holding the working tree, as `done` writes one.
    Wip,
    /// A commit made with `HEAD` detached.
    DetachedHead,
    /// A note on a commit every store already has.
    Notes,
}

impl Axis for Refs {
    const ALL: &'static [Self] =
        &[Self::Branch, Self::Tag, Self::Stash, Self::Wip, Self::DetachedHead, Self::Notes];
    const AXIS: &'static str = "refs";

    fn label(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Tag => "tag",
            Self::Stash => "stash",
            Self::Wip => "wip",
            Self::DetachedHead => "detached-head",
            Self::Notes => "notes",
        }
    }
}

/// What the witness saw of the remote, and when.
///
/// The contract never establishes that a remote is correct now. It establishes that at an
/// instant the remote reported a state, and that the instant is after the last local
/// change. These six are the orderings that instant can stand in, and two of them are
/// FS-1's two orderings of a dropped branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    /// The work was pushed, and the witness fetched after it.
    FetchedAfter,
    /// The witness fetched, and the work was pushed after that.
    FetchedBefore,
    /// The work was never offered to the remote, and the witness has no record of a fetch.
    /// The one value of this axis in which nothing outside the home has the work.
    NeverPushed,
    /// The witness fetched, and the remote then dropped the branch. `FETCH_HEAD` still
    /// names it, dated after the push. DL-073 rules this safe, with the instant printed.
    DroppedAfterFetch,
    /// The remote dropped the branch, and the witness then pruned. Nothing names it.
    Pruned,
    /// The branch was force-pushed over, so the sha the witness saw no longer reaches the
    /// work.
    ForcePushed,
}

impl Axis for Observed {
    const ALL: &'static [Self] = &[
        Self::FetchedAfter,
        Self::FetchedBefore,
        Self::NeverPushed,
        Self::DroppedAfterFetch,
        Self::Pruned,
        Self::ForcePushed,
    ];
    const AXIS: &'static str = "observed";

    fn label(self) -> &'static str {
        match self {
            Self::FetchedAfter => "fetched-after",
            Self::FetchedBefore => "fetched-before",
            Self::NeverPushed => "never-pushed",
            Self::DroppedAfterFetch => "dropped-after-fetch",
            Self::Pruned => "pruned",
            Self::ForcePushed => "force-pushed",
        }
    }
}

/// What the working tree holds that no commit does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tree {
    /// Nothing. Every path is as some commit has it.
    Clean,
    /// A tracked file with a change in it.
    TrackedModified,
    /// A file no commit tracks and no ignore rule covers.
    Untracked,
    /// A file an ignore rule covers, and nothing else. Not in the guarantee, and the
    /// trash keeps it.
    IgnoredOnly,
}

impl Axis for Tree {
    const ALL: &'static [Self] =
        &[Self::Clean, Self::TrackedModified, Self::Untracked, Self::IgnoredOnly];
    const AXIS: &'static str = "tree";

    fn label(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::TrackedModified => "tracked-modified",
            Self::Untracked => "untracked",
            Self::IgnoredOnly => "ignored-only",
        }
    }
}

/// What is running against the home.
///
/// Occupancy is not a member of the loss set, so no value here can make the grid fail. It
/// is on the grid because the two shapes that were missed — a process the account may not
/// read and a process writing from a directory elsewhere — were missed in combination with
/// the other four axes, not on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occupant {
    /// Nothing at all.
    Nothing,
    /// A process of this unit, carrying its identifier.
    OwnProcess,
    /// A process of nothing, standing in the home.
    UnrelatedCwd,
    /// A process whose directory is elsewhere, holding a descriptor open for writing on a
    /// file inside the home.
    WriteDescriptor,
    /// A process of this account that this account may not read. Skipped, with a note,
    /// where the host will not make one.
    Unreadable,
}

impl Axis for Occupant {
    const ALL: &'static [Self] = &[
        Self::Nothing,
        Self::OwnProcess,
        Self::UnrelatedCwd,
        Self::WriteDescriptor,
        Self::Unreadable,
    ];
    const AXIS: &'static str = "occupant";

    fn label(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::OwnProcess => "own-process",
            Self::UnrelatedCwd => "unrelated-cwd",
            Self::WriteDescriptor => "write-descriptor",
            Self::Unreadable => "unreadable",
        }
    }
}

/// One value from each axis: what the generator builds and what a report names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// The topology of the store offered as the copy.
    pub witness: Witness,
    /// Where the home holds its work.
    pub refs: Refs,
    /// What the witness saw of the remote, and when.
    pub observed: Observed,
    /// What the working tree holds.
    pub tree: Tree,
    /// What is running against the home.
    pub occupant: Occupant,
}

impl Shape {
    /// How many shapes the five axes make between them.
    pub const COUNT: usize = Witness::ALL.len()
        * Refs::ALL.len()
        * Observed::ALL.len()
        * Tree::ALL.len()
        * Occupant::ALL.len();

    /// The shape's name: the five labels, in axis order, joined by a slash.
    ///
    /// It is the name a test failure prints and the name a person greps the nightly log
    /// for, so it is the same string every run and it holds no address and no instant.
    #[must_use]
    pub fn name(self) -> String {
        [
            self.witness.label(),
            self.refs.label(),
            self.observed.label(),
            self.tree.label(),
            self.occupant.label(),
        ]
        .join("/")
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::{Axis, Observed, Occupant, Refs, Shape, Tree, Witness};

    /// Every label of every axis is its own. Two values with one label would make a shape
    /// name ambiguous, and the grid's coverage rule counts by label.
    #[test]
    fn no_two_values_of_one_axis_share_a_label() {
        fn distinct<A: Axis>() {
            let mut labels: Vec<&str> = A::ALL.iter().map(|value| value.label()).collect();
            let all = labels.len();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(labels.len(), all, "{} has two values with one label", A::AXIS);
        }
        distinct::<Witness>();
        distinct::<Refs>();
        distinct::<Observed>();
        distinct::<Tree>();
        distinct::<Occupant>();
    }

    /// The count is the product of the axes. It is asserted because the nightly workflow's
    /// runtime is a function of it, and an axis value added without a reading of the cost
    /// is how a nightly job becomes one nobody waits for.
    #[test]
    fn the_grid_is_the_product_of_the_five_axes() {
        assert_eq!(Shape::COUNT, 10 * 6 * 6 * 4 * 5);
    }
}
