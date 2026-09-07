//! The two rows the port allocator keeps: a project's block, and a port held in it.

use std::ops::RangeInclusive;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::environment::PortName;
use crate::model::ids::{EnvId, ProjectId};

/// The range of ports one project hands out from.
///
/// A project is given a block the first time it needs a port, and keeps it. Blocks are
/// equal in length and do not overlap, so a port number says which project holds it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct PortBlock {
    /// The project the block belongs to.
    pub project_id: ProjectId,
    /// The lowest port in the block.
    pub first: u16,
    /// The highest port in the block.
    pub last: u16,
}

impl PortBlock {
    /// Whether `port` is in this block.
    #[must_use]
    pub const fn contains(&self, port: u16) -> bool {
        self.first <= port && port <= self.last
    }

    /// Every port in the block, lowest first.
    #[must_use]
    pub fn ports(self) -> RangeInclusive<u16> {
        self.first..=self.last
    }
}

/// One port an environment holds, under the name the recipe gave it.
///
/// The port is the identity of the row. Two environments cannot hold one port, and an
/// environment holds one port per name.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct PortAllocation {
    /// The port itself.
    pub port: u16,
    /// The project whose block the port came from.
    pub project_id: ProjectId,
    /// The environment that holds it.
    pub environment_id: EnvId,
    /// What the recipe calls this port, for example `app`.
    pub name: PortName,
}
