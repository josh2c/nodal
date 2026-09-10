//! What `nodal reclaim` and `nodal gc` answer with.
//!
//! Both are write commands, and both still answer with a value rather than with printed
//! text, for the reason every read command does: `--json` and the human form have to be
//! two renderings of one answer. A script that reclaims a hundred units reads the same
//! fields a person reads.
//!
//! The last field of a reclaim is the one to read first. [`Reclaimed::leftovers`] is
//! what the verification found still attached to the unit after everything was torn
//! down, and it is a list rather than a failure: an operation that cannot honestly say
//! "nothing left" says what is left, and a person decides. Reporting a leftover is the
//! whole reason the verification exists — an operation that could only either succeed
//! or fail would have to choose between lying and refusing.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::lifecycle::hooks::Ran;
use crate::lifecycle::uniqueness::Finding;
use crate::model::{Slug, Timestamp, Trashed};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, JOIN, NONE, Table};
use crate::runtime::attribute::Note;
use crate::runtime::stop::Stopped;
use crate::services::ports::Released;

/// What a reclaim did on the remote: the refs of Nodal's own it deleted there.
///
/// The branch is not in this value and cannot be. A reclaim deletes what Nodal wrote
/// under `refs/nodal/<id>/` and leaves the person's branch where their colleagues can
/// still read it, so an empty [`Pruned::refs`] with an empty [`Pruned::notes`] is the
/// ordinary answer for a unit the remote held nothing of Nodal's for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pruned {
    /// The remote that was asked, by name, or nothing when none was reached.
    pub remote: Option<String>,
    /// The refs that were deleted there, in full.
    pub refs: Vec<String>,
    /// What could not be done out there. A note is never a failure.
    pub notes: Vec<String>,
}

impl Pruned {
    /// A remote that was asked, and what came of it.
    #[must_use]
    pub fn on(remote: &str, refs: Vec<String>, notes: Vec<String>) -> Self {
        Self { remote: Some(remote.to_owned()), refs, notes }
    }

    /// No remote reached, and the sentence saying why.
    #[must_use]
    pub fn nothing(why: impl Into<String>) -> Self {
        Self { remote: None, refs: Vec::new(), notes: vec![why.into()] }
    }

    /// The line the report prints for it.
    fn cell(&self) -> String {
        let mut lines = Vec::new();
        match (&self.remote, self.refs.len()) {
            (None, _) => {}
            (Some(remote), 0) => lines.push(format!("{remote} held no ref of nodal's")),
            (Some(remote), _) => {
                lines.push(format!("deleted {} on {remote}", self.refs.join(JOIN)));
            }
        }
        lines.push(String::from("the branch was left"));
        lines.extend(self.notes.clone());
        lines.join("\n")
    }
}

/// One thing that is still attached to a unit after it was reclaimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftover {
    /// What kind of thing it is: `port`, `lease`, `session`, `tether`, `process`,
    /// `container`, `listener`, `directory` or `row`.
    pub kind: String,
    /// Which one, in the words the report prints.
    pub detail: String,
}

impl Leftover {
    /// A leftover of a kind, naming the one thing it is about.
    pub fn new(kind: &str, detail: impl Into<String>) -> Self {
        Self { kind: kind.to_owned(), detail: detail.into() }
    }
}

/// What one reclaim did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reclaimed {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit's handle.
    pub slug: String,
    /// What the uniqueness check found. Empty is the ordinary case; anything here was
    /// overridden by `--force` and is printed so the record says what was accepted.
    pub findings: Vec<Finding>,
    /// The ref a forced reclaim committed the work to, before anything was removed.
    pub snapshot: Option<String>,
    /// What was stopped.
    pub stopped: Stopped,
    /// The containers that were removed, by name.
    pub containers: Vec<String>,
    /// The ports and leases that were given back.
    pub released: Released,
    /// Where the home went, when it was a home Nodal made.
    pub trashed: Option<Trashed>,
    /// What was deleted on the remote, when a remote was reached at all.
    pub pruned: Option<Pruned>,
    /// The directory that was left exactly as it is, when the unit was adopted in
    /// place. A root is unregistered, never trashed.
    pub root: Option<PathBuf>,
    /// The hooks that ran, in the order they ran.
    pub hooks: Vec<Ran>,
    /// The signals that could not be read, and why. A note is not a failure; it is the
    /// difference between "nothing is left" and "I could not look".
    pub notes: Vec<Note>,
    /// What the verification still found. Empty means nothing it could read is left.
    pub leftovers: Vec<Leftover>,
}

impl Render for Reclaimed {
    const KIND: &'static str = "reclaimed unit";

    fn doc(&self) -> Doc {
        let mut fields = vec![
            Field::new("unit", self.slug.clone()),
            Field::new("check", self.check_cell()),
            Field::new("stop", self.stop_cell()),
            Field::new(self.home_label(), self.home_cell()),
        ];
        if !self.hooks.is_empty() {
            fields.push(Field::new("hooks", self.hooks_cell()));
        }
        if let Some(pruned) = &self.pruned {
            fields.push(Field::new("remote", pruned.cell()));
        }
        fields.push(Field::new("verify", self.verify_cell()));
        let mut doc = Doc::from_iter([Block::fields(fields)]);
        for note in &self.notes {
            doc.push(Block::line(format!("{}: {}", note.signal.label(), note.why)));
        }
        doc
    }
}

impl Reclaimed {
    /// What the uniqueness check said, and what a `--force` accepted losing.
    fn check_cell(&self) -> String {
        if self.findings.is_empty() {
            return String::from("nothing that is only here");
        }
        let mut lines: Vec<String> = self
            .findings
            .iter()
            .map(|finding| format!("forced past {}", finding.describe()))
            .collect();
        if let Some(reference) = &self.snapshot {
            lines.push(format!("work committed to {reference}"));
        }
        lines.join("\n")
    }

    /// The tethers, processes, containers and ports that were given up.
    ///
    /// A tether is counted apart from a process because it is not one: it is a whole
    /// process group, and "1 tether" means a server and everything it started.
    fn stop_cell(&self) -> String {
        let parts = [
            plural(self.stopped.groups(), "tether", "tethers"),
            plural(self.stopped.processes(), "process", "processes"),
            plural(self.containers.len(), "container removed", "containers removed"),
            plural(self.released.allocated.len() + self.released.fixed.len(), "port", "ports"),
        ];
        let mut cell = parts.join(JOIN);
        if !self.stopped.spared.is_empty() {
            cell.push('\n');
            cell.push_str("the process that asked was left running");
        }
        cell
    }

    /// Which word the home line is labelled with.
    fn home_label(&self) -> &'static str {
        if self.root.is_some() { "root" } else { "trash" }
    }

    /// Where the home is now, and until when.
    fn home_cell(&self) -> String {
        if let Some(root) = &self.root {
            return format!("{} left in place; the unit is no longer registered", root.display());
        }
        let Some(entry) = &self.trashed else { return String::from(NONE) };
        format!(
            "home moved to {}; gc removes it {}",
            entry.path.display(),
            human::until(self.now, entry.expires_at)
        )
    }

    /// Which hooks ran, in order.
    fn hooks_cell(&self) -> String {
        human::join(&self.hooks.iter().map(|ran| ran.phase.to_string()).collect::<Vec<String>>())
    }

    /// What the verification found, which is the line to read.
    ///
    /// "Nothing left by id" is a claim about the whole machine, so it is made only when
    /// every signal answered. A host that could not read one of them is told that
    /// instead, and the note underneath says which.
    fn verify_cell(&self) -> String {
        if self.leftovers.is_empty() && self.notes.is_empty() {
            return String::from("nothing left by id");
        }
        if self.leftovers.is_empty() {
            return String::from("nothing found by id; a signal could not be read");
        }
        self.leftovers
            .iter()
            .map(|left| format!("{}: {}", left.kind, left.detail))
            .collect::<Vec<String>>()
            .join("\n")
    }
}

/// A merged unit whose home `gc` gave back, once its retention had run out.
///
/// Reclaiming is what it does, not removing: the home goes to the trash by the ordinary
/// path, keeps a retention of its own, and a later sweep is what finally takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    /// The unit's handle.
    pub slug: Slug,
    /// Where the home went, or nothing for a checkout that was adopted in place and is
    /// therefore left exactly where it is.
    pub trashed: Option<PathBuf>,
}

/// A live unit nothing has touched for a while.
///
/// Reported and never acted on. Runtime that belongs to a unit somebody is still using
/// is that person's, however long the clock says it has been; what `gc` stops is
/// runtime whose unit has already been reclaimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idle {
    /// The unit's handle.
    pub slug: Slug,
    /// Its home, which is still exactly where it was.
    pub home: PathBuf,
    /// The last instant anything is recorded as having happened in it.
    pub since: Timestamp,
}

/// What one `nodal gc` removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swept {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The trash entries that were removed, oldest first.
    pub removed: Vec<Trashed>,
    /// The entries whose retention has not run out, and are therefore still there.
    pub kept: Vec<Trashed>,
    /// What the removed directories occupied, when it was measured.
    pub freed_bytes: Option<u64>,
    /// Runtime that was stopped because it belonged to a home that is not there.
    pub stopped: Stopped,
    /// Containers removed for the same reason.
    pub containers: Vec<String>,
    /// Leases that had lapsed and were given back.
    pub leases: Vec<String>,
    /// The merged units whose homes were reclaimed because their retention had run out.
    pub retired: Vec<Retired>,
    /// The live units nothing has touched for longer than the threshold that was asked
    /// for. Reported only; nothing of theirs was stopped. Empty when no threshold was
    /// given, which is not the same as "none", and the report says which it is.
    pub idle: Vec<Idle>,
    /// Whether an idle threshold was asked for at all.
    pub idle_asked: bool,
    /// The signals that could not be read, and why.
    pub notes: Vec<Note>,
    /// What could not be removed, and why. Never a failure, for the reason
    /// [`Reclaimed::leftovers`] is not one.
    pub leftovers: Vec<Leftover>,
}

impl Render for Swept {
    const KIND: &'static str = "garbage collection";

    fn doc(&self) -> Doc {
        let mut fields = vec![
            Field::new("freed", self.freed_cell()),
            Field::new("kept", plural(self.kept.len(), "home in trash", "homes in trash")),
            Field::new("merged", self.retired_cell()),
            Field::new("runtime", self.runtime_cell()),
        ];
        if self.idle_asked {
            fields.push(Field::new("idle", self.idle_cell()));
        }
        let mut blocks = vec![Block::fields(fields)];
        if !self.removed.is_empty() {
            blocks.push(Block::table(self.removed_table()));
        }
        if !self.idle.is_empty() {
            blocks.push(Block::table(self.idle_table()));
        }
        if !self.leftovers.is_empty() {
            blocks.push(Block::table(self.leftovers_table()));
        }
        for note in &self.notes {
            blocks.push(Block::line(format!("{}: {}", note.signal.label(), note.why)));
        }
        Doc::from_iter(blocks)
    }
}

impl Swept {
    /// How much went, and how many homes it was.
    fn freed_cell(&self) -> String {
        let homes = plural(self.removed.len(), "home", "homes");
        match self.freed_bytes {
            Some(bytes) => format!("{} ({homes})", human::bytes(bytes)),
            None => homes,
        }
    }

    /// The runtime that was stopped because its home is gone.
    fn runtime_cell(&self) -> String {
        [
            plural(self.stopped.groups(), "tether", "tethers"),
            plural(self.stopped.processes(), "process", "processes"),
            plural(self.containers.len(), "container", "containers"),
            plural(self.leases.len(), "lapsed lease", "lapsed leases"),
        ]
        .join(JOIN)
    }

    /// The merged units whose homes were given back this sweep.
    fn retired_cell(&self) -> String {
        let count = plural(self.retired.len(), "unit reclaimed", "units reclaimed");
        if self.retired.is_empty() {
            return count;
        }
        let named = self.retired.iter().map(|unit| unit.slug.to_string()).collect::<Vec<String>>();
        format!("{count}: {}", human::join(&named))
    }

    /// How many live units have gone quiet, and the line that says nothing was done
    /// about them.
    fn idle_cell(&self) -> String {
        let count = plural(self.idle.len(), "live unit", "live units");
        format!("{count} past the threshold; reported only, nothing was stopped")
    }

    /// One row per idle unit: which, for how long, and where it still is.
    fn idle_table(&self) -> Table {
        let mut table = Table::new(&["idle", "quiet for", "home"]);
        for unit in &self.idle {
            table.push(vec![
                unit.slug.to_string(),
                human::since(self.now, unit.since),
                unit.home.display().to_string(),
            ]);
        }
        table
    }

    /// One row per home removed.
    fn removed_table(&self) -> Table {
        let mut table = Table::new(&["removed", "unit", "path"]);
        for entry in &self.removed {
            table.push(vec![
                human::since(self.now, entry.trashed_at),
                entry.slug.to_string(),
                entry.path.display().to_string(),
            ]);
        }
        table
    }

    /// One row per thing that would not go.
    fn leftovers_table(&self) -> Table {
        let mut table = Table::new(&["left", "detail"]);
        for left in &self.leftovers {
            table.push(vec![left.kind.clone(), left.detail.clone()]);
        }
        table
    }
}

/// A count with the word that goes with it, so a report never says `1 processes`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{Leftover, Pruned, Reclaimed, plural};
    use crate::model::Timestamp;
    use crate::output::Render;

    fn reclaimed() -> Reclaimed {
        Reclaimed {
            now: Timestamp::parse("2026-09-07T09:00:00Z").unwrap(),
            slug: String::from("worker-import"),
            findings: Vec::new(),
            snapshot: None,
            stopped: crate::runtime::stop::Stopped::default(),
            containers: Vec::new(),
            released: crate::services::ports::Released::default(),
            trashed: None,
            pruned: None,
            root: None,
            hooks: Vec::new(),
            notes: Vec::new(),
            leftovers: Vec::new(),
        }
    }

    /// The line says which refs went and, in the same breath, that the branch did not.
    /// A person reading a reclaim has to be able to tell their colleagues that the
    /// branch they were sent is still there.
    #[test]
    fn the_remote_line_names_what_was_deleted_and_says_the_branch_was_left() {
        let mut report = reclaimed();
        report.pruned =
            Some(Pruned::on("origin", vec![String::from("refs/nodal/01J/wip")], Vec::new()));
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("refs/nodal/01J/wip"), "{lines}");
        assert!(lines.contains("the branch was left"), "{lines}");
    }

    /// A remote that could not be reached is a note under the line, and the reclaim it
    /// belongs to still reports as a reclaim.
    #[test]
    fn a_remote_that_refused_is_a_note_and_not_a_failure() {
        let mut report = reclaimed();
        report.pruned =
            Some(Pruned::on("origin", Vec::new(), vec![String::from("origin is unreachable")]));
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("origin is unreachable"), "{lines}");
        assert!(lines.contains("nothing left by id"), "{lines}");
    }

    #[test]
    fn one_of_a_thing_is_never_reported_in_the_plural() {
        assert_eq!(plural(1, "process", "processes"), "1 process");
        assert_eq!(plural(0, "process", "processes"), "0 processes");
    }

    #[test]
    fn a_reclaim_that_left_nothing_says_so_in_one_line() {
        let lines = reclaimed().doc().lines().join("\n");
        assert!(lines.contains("nothing left by id"), "{lines}");
        assert!(lines.contains("nothing that is only here"), "{lines}");
    }

    #[test]
    fn a_reclaim_that_stopped_a_tether_counts_it_apart_from_a_process() {
        let mut report = reclaimed();
        report.stopped.asked = vec![
            crate::runtime::stop::Target::Group(900),
            crate::runtime::stop::Target::Process(7),
        ];
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("1 tether"), "{lines}");
        assert!(lines.contains("1 process"), "{lines}");
    }

    #[test]
    fn a_reclaim_run_from_inside_the_home_says_what_it_would_not_stop() {
        let mut report = reclaimed();
        report.stopped.spared = vec![crate::runtime::stop::Target::Process(42)];
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("the process that asked was left running"), "{lines}");
        assert!(lines.contains("nothing left by id"), "and it is not a leftover: {lines}");
    }

    #[test]
    fn a_reclaim_that_left_something_names_it_instead_of_claiming_clean() {
        let mut report = reclaimed();
        report.leftovers.push(Leftover::new("port", "20001 (app)"));
        let lines = report.doc().lines().join("\n");
        assert!(!lines.contains("nothing left"), "{lines}");
        assert!(lines.contains("port: 20001 (app)"), "{lines}");
    }

    #[test]
    fn a_host_that_could_not_look_is_never_told_that_nothing_is_left() {
        let mut report = reclaimed();
        report.notes.push(crate::runtime::attribute::Note::new(
            crate::runtime::attribute::Source::Environment,
            "a process scan reads /proc, which macos does not have",
        ));
        let lines = report.doc().lines().join("\n");
        assert!(!lines.contains("nothing left by id"), "{lines}");
        assert!(lines.contains("nothing found by id"), "{lines}");
        assert!(lines.contains("env: a process scan reads /proc"), "{lines}");
    }

    #[test]
    fn an_adopted_root_is_reported_as_left_in_place() {
        let mut report = reclaimed();
        report.root = Some("/home/u/code/project".into());
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("left in place"), "{lines}");
        assert!(!lines.contains("moved to"), "{lines}");
    }
}
