//! The machine's state, as `nodal status` prints it and `--watch` streams it.
//!
//! This is the read type the NDJSON stream carries, so its `serde` shape is what a
//! consumer of `status --watch` parses, one document per line.

use serde::{Deserialize, Serialize};

use crate::model::{HostName, ProjectName, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc};
use crate::output::view::unit::{self, UnitRow};

/// Something the units of a project have in common: the shared service stack, the
/// package manager's store, the warm bases, the trash waiting to be collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedResource {
    /// What it is called, for example `supabase stack`.
    pub name: String,
    /// What is worth saying about it, for example `10 containers`.
    pub detail: Option<String>,
    /// What it occupies, when it has been measured.
    pub disk_bytes: Option<u64>,
}

/// Every unit of a project on this host, and what they share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The instant the answer was taken. Every relative time is measured from it, and a
    /// consumer of the stream reads it as the frame's timestamp.
    pub now: Timestamp,
    /// The project.
    pub project: ProjectName,
    /// The host the answer was taken on.
    pub host: HostName,
    /// The units, in the order the producer chose.
    pub units: Vec<UnitRow>,
    /// What those units share.
    pub shared: Vec<SharedResource>,
}

impl Render for Status {
    const KIND: &'static str = "status";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        if self.units.is_empty() {
            doc.push(Block::line(format!("{}: no units yet", self.project)));
        } else {
            doc.push(Block::table(unit::table(&self.units, self.now)));
        }
        if !self.shared.is_empty() {
            doc.push(Block::line(format!("shared: {}", shared_line(&self.shared))));
        }
        doc
    }
}

/// The shared resources on one line, as `supabase stack (10 containers) · base 1.2 GB`.
fn shared_line(shared: &[SharedResource]) -> String {
    let named: Vec<String> = shared.iter().map(one).collect();
    human::join(&named)
}

/// One shared resource: its name, what is worth saying about it, and its size.
fn one(resource: &SharedResource) -> String {
    let detail = resource.detail.as_ref().map(|detail| format!(" ({detail})")).unwrap_or_default();
    let size =
        resource.disk_bytes.map(|bytes| format!(" {}", human::bytes(bytes))).unwrap_or_default();
    format!("{}{detail}{size}", resource.name)
}
