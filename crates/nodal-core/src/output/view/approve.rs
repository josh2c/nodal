//! What `nodal approve` did: the hook commands this machine now accepts.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Hooks, Phase};
use crate::output::Render;
use crate::output::human::{Block, Doc, Table};

/// One hook command, as the person who is approving it reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approved {
    /// Which hook it is.
    pub phase: Phase,
    /// Its exact text, which is what the approval pins.
    pub command: String,
}

/// The approval record after `nodal approve` wrote it.
///
/// The commands are on the report because approving is the one moment a person decides
/// whether somebody else's command may run on their account. A count would tell them
/// that they decided; the text tells them what they decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    /// The project whose recipe declares them, which is what the record is keyed by.
    pub project: PathBuf,
    /// The file the record was written to.
    pub record: PathBuf,
    /// Every command this machine now accepts for that project, in phase order.
    pub commands: Vec<Approved>,
}

impl Approval {
    /// The report for a project whose declared hooks have just been approved.
    #[must_use]
    pub fn of(project: PathBuf, record: PathBuf, hooks: &Hooks) -> Self {
        let commands = crate::model::recipe::PHASES
            .iter()
            .filter_map(|phase| {
                phase
                    .command(hooks)
                    .map(|command| Approved { phase: *phase, command: command.as_str().to_owned() })
            })
            .collect();
        Self { project, record, commands }
    }
}

impl Render for Approval {
    const KIND: &'static str = "approval";

    fn doc(&self) -> Doc {
        if self.commands.is_empty() {
            return Doc::from_iter([Block::line(format!(
                "{} declares no hook, so nothing is approved",
                self.project.display()
            ))]);
        }
        let mut table = Table::new(&["hook", "command"]);
        for approved in &self.commands {
            table.push(vec![approved.phase.key().to_owned(), approved.command.clone()]);
        }
        Doc::from_iter([
            Block::line(format!("approved for {}", self.project.display())),
            Block::table(table),
            Block::line(format!("recorded in {}", self.record.display())),
        ])
    }
}
