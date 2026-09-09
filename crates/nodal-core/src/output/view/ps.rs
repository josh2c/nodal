//! What is running on this machine, and whose it is: what `nodal ps` answers with.

use serde::{Deserialize, Serialize};

use crate::model::{HostName, Timestamp};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Table};
use crate::output::notice::{self, Notice};
use crate::runtime::attribute::{Attributed, Note};

/// The columns of the attribution table, in the order they are printed.
const COLUMNS: [&str; 7] = ["unit", "kind", "what", "pid", "port", "confidence", "by"];

/// Everything attributed to a unit on this host, with the confidence of each row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ps {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The host it was taken on. Attribution reads this machine and no other.
    pub host: HostName,
    /// The rows, ordered by unit, then kind, then name.
    pub rows: Vec<Attributed>,
    /// What a signal could not do. A note is not a failure: it is the difference
    /// between "nothing is running" and "I could not see".
    pub notes: Vec<Note>,
}

impl Render for Ps {
    const KIND: &'static str = "ps";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        if self.rows.is_empty() {
            doc.push(Block::line(format!("{}: nothing attributed to a unit", self.host)));
        } else {
            doc.push(Block::table(table(&self.rows)));
        }
        for line in notice::collapse(&notices(&self.notes), "signals") {
            doc.push(Block::line(line));
        }
        doc
    }
}

/// The notes as notices, so that one cause is one line however many signals it stopped.
///
/// A host with no `/proc` stops both process signals for one reason, and the reason
/// printed twice reads as two faults. Collapsed, it is one line that still names both:
/// which signals went quiet is the fact a person needs, and a count would lose it.
fn notices(notes: &[Note]) -> Vec<Notice> {
    notes.iter().map(|note| Notice::about(note.signal.label(), &note.why)).collect()
}

/// The attribution table.
fn table(rows: &[Attributed]) -> Table {
    let mut table = Table::new(&COLUMNS);
    for row in rows {
        table.push(vec![
            row.slug.to_string(),
            row.kind.label().to_owned(),
            human::or_none(row.what.clone()),
            number(row.pid),
            number(row.port),
            row.confidence.label().to_owned(),
            row.signal.label().to_owned(),
        ]);
    }
    table
}

/// A number, or the placeholder when the row has none: a container has no port of its
/// own to show here, and a listener has no process this reading can name.
fn number<T: ToString>(value: Option<T>) -> String {
    value.map_or_else(|| String::from(human::NONE), |value| value.to_string())
}
