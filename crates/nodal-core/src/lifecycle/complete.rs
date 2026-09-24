//! Whether a store outside a home holds the work, and not only the commit.
//!
//! The second-copy reading asks whether a ref of another repository reaches a commit.
//! That establishes one thing: the commit object is in that store, and a ref keeps it
//! there. It does not establish that the store holds the commit's tree or its blobs, and
//! the work is in the trees and the blobs. The two are the same thing only in a
//! repository that holds all of its objects, and nothing asked.
//!
//! A blobless partial clone (`git clone --filter=blob:none`) answers the reachability
//! question perfectly and holds none of the content. That was reproduced end to end: the
//! verdict said safe, it named the partial clone as the proof, and after the home went
//! and the remote went, that directory answered `fatal: bad object <sha>:unique.txt`. The
//! only surviving copy of the file on the whole disk was the trash, which a sweep removes
//! on a timer.
//!
//! So a store is read before it may vouch for anything, and there are two readings.
//!
//! # The cheap reading: what a store says about itself
//!
//! A store that cannot produce content advertises the reason in its own configuration and
//! in two files, and reading all of it costs about a millisecond:
//!
//! | reading | what it means |
//! |---|---|
//! | its common git directory is the home's | it is a worktree of the home; its objects go with the home |
//! | `objects/info/alternates` | it borrows its objects from a store this removal may take, or that a `gc` elsewhere may empty |
//! | a promisor remote, or `extensions.partialClone` | it is a partial clone: it fetches the objects it lacks on demand, and nothing here may fetch |
//! | a `shallow` file | its history stops at a depth, so a tip promises nothing behind it |
//!
//! The witness path made the shallow reading already and the store path did not, and the
//! difference between the two was an oversight rather than a decision.
//!
//! # The dear reading: whether the objects are there
//!
//! A store may pass all four and still be missing an object, so the commits it claims are
//! walked for the objects they introduce. The walk is bounded to what the home's own
//! commits add — their trees and blobs down to the parents they share with the rest of
//! the history — so it costs what the change costs rather than what the repository costs.
//! `git rev-list --objects --missing=print` prints one line per object it cannot find,
//! and one such line is enough to refuse.
//!
//! # What a failure is worth
//!
//! Not a loss, and not a copy: an unread reading. The store holds the commits as far as
//! this machine can say, so calling them only here would be a claim this machine did not
//! earn; and it cannot produce the work, so calling them a second copy would be the false
//! safe this module exists to remove. They are not checked, and the row names the
//! directory and the property that failed, because a person who is refused has to know
//! what to do next.
//!
//! Nothing here writes, and nothing here reaches a network. A partial store fetches a
//! missing object on demand unless it is told not to, and [`crate::git::cmd`] tells every
//! invocation not to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{Git, Oid, layout};
use crate::paths;

/// Where a store keeps the record that it borrows its objects from somewhere else.
const ALTERNATES: &str = "objects/info/alternates";

/// Where a store keeps the record that its history stops at a depth.
const SHALLOW: &str = "shallow";

/// Why one store may not stand as a second copy of the commits it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Lacking {
    /// It is another checkout of the home's own repository, so its objects are the
    /// home's objects and the removal takes them both.
    SameRepository,
    /// It borrows its objects from another store rather than holding them.
    Borrowed,
    /// It is a partial clone: it holds the commits and fetches their content on demand.
    Partial,
    /// Its history stops at a depth, so a tip in it promises nothing behind that tip.
    Shallow,
    /// It passed every other reading and does not hold some of the objects.
    Missing {
        /// How many objects the walk could not find. Exact.
        objects: usize,
    },
    /// The reading itself could not be made.
    Unreadable,
}

impl Lacking {
    /// What a report says about a store this is true of, after the store is named.
    #[must_use]
    pub const fn because(&self) -> &'static str {
        match self {
            Self::SameRepository => {
                "holds them and is a worktree of this home, so its objects go with the home"
            }
            Self::Borrowed => {
                "holds them and borrows its objects from another store, which this removal \
                 or a collection elsewhere may take"
            }
            Self::Partial => {
                "holds them and is a partial clone, so it holds the commits and not the work"
            }
            Self::Shallow => "holds them and is shallow, so it promises no history behind a tip",
            Self::Missing { .. } => "holds them and does not hold every object behind them",
            Self::Unreadable => "holds them and could not be read",
        }
    }
}

/// One store that holds commits of a home and may not stand as a copy of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incomplete {
    /// Where the store is.
    pub store: PathBuf,
    /// Which reading it failed.
    pub lacking: Lacking,
}

impl Incomplete {
    /// The clause a refusal adds about this store, naming it and saying what failed.
    #[must_use]
    pub fn because(&self) -> String {
        format!("{} {}", self.store.display(), self.lacking.because())
    }
}

/// What a store says about itself, or nothing when it says nothing against itself.
///
/// Four readings and at most three processes, and every one of them is of a file this
/// store wrote about itself. `home` is the directory the removal would take, which is the
/// one thing a store cannot be another copy of.
///
/// A store whose git directory cannot be found is left to the reading that follows: this
/// says what a store admits, and a directory that admits nothing may still fail to
/// produce an object ([`missing`]).
#[must_use]
pub fn admits(store: &Path, home: &Path) -> Option<Lacking> {
    let common = layout::common_dir(store)?;
    if layout::common_dir(home).is_some_and(|theirs| paths::resolve(&common) == paths::resolve(&theirs))
    {
        return Some(Lacking::SameRepository);
    }
    if common.join(ALTERNATES).exists() {
        return Some(Lacking::Borrowed);
    }
    if common.join(SHALLOW).exists() {
        return Some(Lacking::Shallow);
    }
    Git::at(store).partial().unwrap_or(false).then_some(Lacking::Partial)
}

/// How many of the objects behind `commits` this store does not hold, and `None` when the
/// walk could not be made.
///
/// `boundary` is what the walk stops at: the parents of `commits` that are not themselves
/// in `commits`. What is left is the objects the home's own commits introduce, which is
/// the work, and the walk costs what that change costs rather than what the repository
/// costs.
///
/// A walk that would not run answers `None`, which its caller reads as a store that could
/// not be checked. It is never read as a store that holds everything.
#[must_use]
pub fn missing(store: &Path, commits: &[Oid], boundary: &[Oid]) -> Option<usize> {
    Git::at(store).missing_objects(commits, boundary).ok()
}
