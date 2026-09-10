//! Substrate: the warm bases a unit home is a copy-on-write clone of.
//!
//! A unit home has to be cheap to make, and a clone is only cheap if the thing being
//! cloned is already a working copy — dependencies installed, build output where the
//! tools expect it. That thing is a base, and this module is how one comes to exist.
//!
//! Three claims hold everywhere in here.
//!
//! *Nobody builds a base on purpose.* [`bases::ensure`] is called by the first
//! `nodal new` that wants one and builds it if it is not there, reporting each step as
//! it goes ([`progress`]). `nodal base build` is the same call made early.
//!
//! *The first base is a clone.* Of the project's remote, or of the checkout itself
//! when the project names no remote — a clone either way, never a copy. A checkout
//! carries uncommitted files, another tool's `.git` state, and a `node_modules`
//! installed for whatever branch it was last on; a clone leaves all of it behind,
//! because Git carries objects and builds the working tree from them. Every later base
//! is a copy of the nearest base already built here, which is what makes the second one
//! fast.
//!
//! *A base is a root, not a home.* Homes are cloned **from** bases, so a base is never
//! part of what a unit's reclaim removes. Where the two sit is
//! [`crate::workspace::home`]'s to say, and it puts them under different segments of
//! the project's directory, which is what makes that structural rather than a rule
//! somebody has to remember.

pub mod bases;
pub mod build;
pub mod lru;
pub mod pin;
pub mod progress;

pub use crate::substrate::bases::{Outcome, Request, ensure, evict, gc, list, pins, resolve};
pub use crate::substrate::build::{BaseBuild, Origin, Params};
pub use crate::substrate::lru::DEFAULT_KEEP;
pub use crate::substrate::pin::Install;
pub use crate::substrate::progress::{Collector, Reporter, Silent, Stderr, sink};
