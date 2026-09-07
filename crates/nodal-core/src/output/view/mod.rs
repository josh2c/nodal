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
pub mod env;
pub mod event;
pub mod init;
pub mod ps;
pub mod status;
pub mod unit;

pub use crate::output::view::base::{BaseBuild, BaseList, BaseRow, BaseSweep};
pub use crate::output::view::created::Created;
pub use crate::output::view::env::{EnvReport, VarLine};
pub use crate::output::view::event::EventLog;
pub use crate::output::view::init::InitReport;
pub use crate::output::view::ps::Ps;
pub use crate::output::view::status::{SharedResource, Status};
pub use crate::output::view::unit::{
    EnvLine, Freshness, Running, UnitDetail, UnitList, UnitRow, WorkTree,
};
