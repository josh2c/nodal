//! The ledger: what has happened in a unit, as `nodal explain` prints it.

use serde::{Deserialize, Serialize};

use crate::model::{Epistemic, Event, EventKind, Slug, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Table};

/// Events, in the order the producer chose, with the unit they belong to when they all
/// belong to one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLog {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit, when the log is of one unit rather than of a project.
    pub unit: Option<Slug>,
    /// The events themselves, most recent last.
    pub events: Vec<Event>,
}

impl Render for EventLog {
    const KIND: &'static str = "event log";

    fn doc(&self) -> Doc {
        if self.events.is_empty() {
            return Doc::from_iter([Block::line("nothing recorded yet")]);
        }
        Doc::from_iter([Block::table(table(&self.events, self.now))])
    }
}

/// The columns of the event table.
const COLUMNS: [&str; 5] = ["when", "kind", "actor", "how", "what"];

/// The event table, shared by the log and by a unit's detail view.
pub(crate) fn table(events: &[Event], now: Timestamp) -> Table {
    let mut table = Table::new(&COLUMNS);
    for event in events {
        table.push(vec![
            human::since(now, event.ts),
            kind_label(event.kind).to_owned(),
            event.actor.name.to_string(),
            epistemic_label(event.epistemic).to_owned(),
            event.body.lines().next().unwrap_or("").to_owned(),
        ]);
    }
    table
}

/// The word an event kind carries in output, in one place.
fn kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Attached => "attached",
        EventKind::Detached => "detached",
        EventKind::Command => "command",
        EventKind::Commit => "commit",
        EventKind::TestResult => "test",
        EventKind::Failure => "failure",
        EventKind::FileTouched => "touched",
        EventKind::Finding => "finding",
        EventKind::Decision => "decision",
        EventKind::Question => "question",
        EventKind::Handoff => "handoff",
        EventKind::Sync => "sync",
        EventKind::Note => "note",
    }
}

/// Whether Nodal saw the event or was told about it. The distinction is on every row
/// because an agent's claim and a recorded command are not the same kind of fact.
fn epistemic_label(epistemic: Epistemic) -> &'static str {
    match epistemic {
        Epistemic::Observed => "saw",
        Epistemic::Stated => "said",
    }
}
