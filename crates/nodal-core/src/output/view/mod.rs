//! The read types: what a read command answers with.
//!
//! One file per command family. Each type is plain data with a `serde` shape and a
//! [`Render`](crate::output::Render) implementation, and holds no IO: a producer builds
//! one and hands it to the output layer.
//!
//! They live here, beside the renderers, rather than in `model/`, because they are not
//! records. A row of `nodal ls` is a join across three tables plus facts computed at the
//! moment they matter, and `model/` is the shape of what the registry stores.

pub mod base;
pub mod created;
pub mod doctor;
pub mod done;
pub mod env;
pub mod event;
pub mod explain;
pub mod init;
pub mod merge;
pub mod ps;
pub mod reclaim;
pub mod setup;
pub mod status;
pub mod unit;
pub mod verdict;

pub use crate::output::view::base::{BaseBuild, BaseList, BaseRow, BaseSweep};
pub use crate::output::view::created::{Arrival, Created};
pub use crate::output::view::doctor::{Checkout, Doctor, Finding, Kind, Note};
pub use crate::output::view::done::Done;
pub use crate::output::view::env::{EnvReport, VarLine};
pub use crate::output::view::event::EventLog;
pub use crate::output::view::explain::{Exclusion, Explained, Invalidation, Origin, PortLine};
pub use crate::output::view::init::InitReport;
pub use crate::output::view::merge::{Conflict, Merged, StageLine};
pub use crate::output::view::ps::Ps;
pub use crate::output::view::reclaim::{Idle, Leftover, Pruned, Reclaimed, Retired, Swept};
pub use crate::output::view::setup::{Installed, Uninstall, Upgrade};
pub use crate::output::view::status::{SharedResource, Status};
pub use crate::output::view::unit::{
    EnvLine, Freshness, Remote, Running, ToolSessions, UnitDetail, UnitList, UnitRow, WorkTree,
};
pub use crate::output::view::verdict::{Behind, RowKind, Verdict, WorktreeRow};
