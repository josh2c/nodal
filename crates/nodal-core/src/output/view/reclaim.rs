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

use crate::git::Oid;
use crate::lifecycle::hooks::Ran;
use crate::lifecycle::uniqueness::Finding;
use crate::lifecycle::uniqueness::Witness;
use crate::model::{Outside, Slug, Timestamp, Trashed};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, JOIN, NONE, Table};
use crate::runtime::attribute::{Note, Source};
use crate::runtime::stop::Stopped;
use crate::services::ports::Released;
use crate::workspace::prune;

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

/// The paths of a set of removals, in the order the prune found them.
fn named_paths(removals: &[prune::Removal]) -> Vec<String> {
    removals.iter().map(|removal| removal.path.display().to_string()).collect()
}

/// What a reclaim does not write, said in the report rather than left to be found.
///
/// The label is six characters, as every other label of this report is, so that adding
/// the line moved no other line of it.
///
/// Every other command that touches a unit compiles the memory of every unit of the
/// project. A reclaim does not, because a reclaim of one unit writes into that unit's
/// home and into the registry, and into nothing else.
const MEMORIES: &str = "this reclaim wrote into no other unit's home. The next nodal command that reads \
     them writes their WORKUNIT.md again";

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
    /// The ref the run committed the whole home to before its first step, when the
    /// home had a commit to build on. Every reclaim takes it, forced or not, and it is
    /// where the home as it was can be read back from the trashed repository.
    #[serde(default)]
    pub record: Option<String>,
    /// What was stopped.
    pub stopped: Stopped,
    /// The containers that were removed, by name.
    pub containers: Vec<String>,
    /// The ports and leases that were given back.
    pub released: Released,
    /// Where the home went, when it was a home Nodal made.
    pub trashed: Option<Trashed>,
    /// What the trashed copy lost on the way in: the build output and the installed
    /// dependencies, and the local state the trash kept instead.
    #[serde(default)]
    pub trimmed: prune::Report,
    /// What was deleted on the remote, when a remote was reached at all.
    pub pruned: Option<Pruned>,
    /// The directory that was left exactly as it is, when the unit was adopted in
    /// place. A root is unregistered, never trashed.
    pub root: Option<PathBuf>,
    /// What `nodal reclaim --prune` would take out of that directory, for the run that
    /// did not ask for it. Empty for a home Nodal made, and for the run that pruned:
    /// [`Reclaimed::trimmed`] then says what went.
    ///
    /// This is the warning the person reads before they consent to anything. The two
    /// fields hold the same type because they are the same classification, read once
    /// and acted on once.
    #[serde(default)]
    pub prunable: Vec<prune::Removal>,
    /// The hooks that ran, in the order they ran.
    pub hooks: Vec<Ran>,
    /// The signals that could not be read, and why. A note is not a failure; it is the
    /// difference between "nothing is left" and "I could not look".
    pub notes: Vec<Note>,
    /// What the verification still found. Empty means nothing it could read is left.
    pub leftovers: Vec<Leftover>,
    /// The path of a done adopted worktree, when reclaim may print
    /// `git worktree remove` for it. Never set for a home Nodal made, or when the
    /// uniqueness check found anything unique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_remove: Option<PathBuf>,
}

impl Render for Reclaimed {
    const KIND: &'static str = "reclaimed unit";

    fn doc(&self) -> Doc {
        let mut fields =
            vec![Field::new("unit", self.slug.clone()), Field::new("check", self.check_cell())];
        if let Some(reference) = &self.record {
            fields.push(Field::new(
                "record",
                format!("the home before this reclaim is on {reference}"),
            ));
        }
        fields.extend([
            Field::new("stop", self.stop_cell()),
            Field::new(self.home_label(), self.home_cell()),
        ]);
        if !self.trimmed.kept.is_empty() {
            fields.push(Field::new(self.kept_label(), self.kept_cell()));
        }
        if !self.hooks.is_empty() {
            fields.push(Field::new("hooks", self.hooks_cell()));
        }
        if let Some(pruned) = &self.pruned {
            fields.push(Field::new("remote", pruned.cell()));
        }
        fields.push(Field::new("verify", self.verify_cell()));
        fields.push(Field::new("memory", MEMORIES));
        if let Some(path) = &self.worktree_remove {
            fields.push(Field::new("remove", crate::output::view::adopt::removal_command(path)));
        }
        let mut doc = Doc::from_iter([Block::fields(fields)]);
        for line in super::ps::note_lines(&self.notes) {
            doc.push(Block::line(line));
        }
        doc
    }
}

impl Reclaimed {
    /// What the uniqueness check said, what it rested on, and what a `--force` accepted
    /// losing.
    ///
    /// A verdict of "nothing that is only here" is a statement about other repositories,
    /// and the line names them. A person reading it can see what the removal depends on,
    /// and so can the sweep that removes the directory later: the same names are on the
    /// trash row, and `nodal gc` prints them again if the copy has gone by then.
    fn check_cell(&self) -> String {
        if self.findings.is_empty() {
            let mut lines = vec![String::from("nothing that is only here")];
            lines.extend(self.rested_lines());
            return lines.join("\n");
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

    /// Where the commits this reclaim did not refuse over also live, one line each.
    ///
    /// Nothing at all for a home that held no commit of its own, because there is then
    /// no second copy for the verdict to have rested on and a line saying so would be
    /// about a question nobody asked.
    fn rested_lines(&self) -> Vec<String> {
        let entry = self.trashed.as_ref();
        entry
            .map(|entry| entry.rested.copies())
            .unwrap_or_default()
            .iter()
            .map(|copy| format!("{} of them are also in {}", copy.commits, copy.describe()))
            .collect()
    }

    /// The tethers, processes, containers and ports that were given up.
    ///
    /// A tether is counted apart from a process because it is not one: it is a whole
    /// process group, and "1 tether" means a server and everything it started.
    fn stop_cell(&self) -> String {
        let parts = [
            plural(self.stopped.groups(), "tether", "tethers"),
            super::check::counted(
                &self.notes,
                Source::Environment,
                self.stopped.processes(),
                "process",
                "processes",
            ),
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

    /// Which word the kept-state line is labelled with.
    ///
    /// The same split as [`Reclaimed::home_label`], and for the same reason. A home
    /// Nodal made is in the trash and a person goes there to get an `.env.local` back.
    /// A checkout adopted in place was never moved, so the same file is where they left
    /// it, and a line that sent them to a trash would send them nowhere.
    fn kept_label(&self) -> &'static str {
        if self.root.is_some() {
            "local state left where it is"
        } else {
            "local state kept in the trash"
        }
    }

    /// Where the home is now, and until when.
    fn home_cell(&self) -> String {
        if let Some(root) = &self.root {
            let mut lines =
                vec![format!("{} left in place; the unit is no longer registered", root.display())];
            lines.push(self.root_prune_line());
            lines.extend(self.trimmed.notes.clone());
            return lines.join("\n");
        }
        let Some(entry) = &self.trashed else { return String::from(NONE) };
        let mut lines = vec![format!(
            "home moved to {}; gc removes it {}",
            entry.path.display(),
            human::until(self.now, entry.expires_at)
        )];
        lines.push(self.dropped_line());
        lines.extend(self.trimmed.notes.clone());
        lines.join("\n")
    }

    /// What the prune took out of the checkout, or what `--prune` would take.
    ///
    /// A checkout adopted in place is the person's own directory and a reclaim never
    /// moves it, so its build output is reachable by nothing else. The line says the
    /// bytes either way: after `--prune` it is a record of what went, and without it, it
    /// is the warning that the flag answers. A checkout holding no regenerable state
    /// says so, because a missing line and "there was none" are different claims.
    fn root_prune_line(&self) -> String {
        if !self.trimmed.changed_nothing() {
            return format!(
                "removed {} of build output and dependencies: {}",
                human::bytes(self.trimmed.bytes),
                human::join(&named_paths(&self.trimmed.removed))
            );
        }
        if self.prunable.is_empty() {
            return String::from("it holds no build output an ignore rule covers");
        }
        let bytes: u64 = self.prunable.iter().map(|removal| removal.bytes).sum();
        format!(
            "it holds {} of build output and dependencies that stay: {}. `nodal reclaim {} \
             --prune` removes them",
            human::bytes(bytes),
            human::join(&named_paths(&self.prunable)),
            self.slug,
        )
    }

    /// What the trash did not have to keep, and what it holds instead.
    ///
    /// The contract in one line: the trash holds the home without its build output and
    /// its dependencies. A home that held none of it says so, because "nothing was
    /// dropped" and "the line is missing" are different claims and a person reading a
    /// thirteen gigabyte trash needs the first.
    fn dropped_line(&self) -> String {
        if self.trimmed.changed_nothing() {
            return String::from("the trash holds the whole home; it held no build output");
        }
        format!(
            "dropped {} of build output and dependencies: {}",
            human::bytes(self.trimmed.bytes),
            human::join(&named_paths(&self.trimmed.removed))
        )
    }

    /// The ignored state the prune did not take, which is what a person goes back for.
    ///
    /// Named one by one while there are few enough to read ([`prune::NAMED`]), and
    /// counted with a total after that. Either way the answer is the same claim: this
    /// is what no commit holds and no prune removed.
    fn kept_cell(&self) -> String {
        let kept = &self.trimmed.kept;
        if kept.len() > prune::NAMED {
            return format!(
                "{} paths holding {}; nodal show names them",
                kept.len(),
                human::bytes(self.trimmed.kept_bytes)
            );
        }
        let names: Vec<String> =
            kept.iter().map(|entry| entry.path.display().to_string()).collect();
        human::join(&names)
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

/// One expired home `nodal gc` did not remove.
///
/// The row stays with the directory, so the entry is still in the trash, still expired,
/// and read again by the next sweep. A copy somebody restores is therefore all it takes
/// for the home to go on the sweep after that.
///
/// A home that could not be read at all is not one of these. That is a directory nobody
/// could remove and nobody could ask about, which is what [`Leftover`] already carries,
/// and a second list for it would be a second word for one fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldBack {
    /// The entry that stays.
    pub entry: Trashed,
    /// How many commits in the home no ref outside it reaches. Exact.
    pub count: usize,
    /// The first ten of them, newest first. A sample; the count is the fact.
    pub sample: Vec<Oid>,
    /// What this machine could say about the remote while it read them.
    ///
    /// It decides the words. A reading nothing could check has not earned "only here",
    /// and the line says what it could not do instead ([`Witness::because`]).
    pub witness: Witness,
    /// The copies the reclaim rested on that no longer reach these commits.
    ///
    /// Empty where the reclaim recorded none, and empty where every copy it recorded
    /// still holds them, which is why the line names these rather than the whole row.
    pub gone: Vec<Outside>,
}

impl HeldBack {
    /// What the sweep says about this home: one line per commit, then the reason the
    /// remote question is open, where it is open.
    ///
    /// Each commit line names the commit and the copy the reclaim rested on that has
    /// since gone, because those two together are what a person acts on: the commit says
    /// what would go, and the repository and ref say where to look for what held it. The
    /// last line says what this machine could not read, and it is one line for the home
    /// rather than one for each commit, because it is one fact about the home.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let clause = self.clause();
        let mut lines: Vec<String> = self
            .sample
            .iter()
            .map(|oid| {
                format!("kept: {} {clause}", oid.as_str().chars().take(8).collect::<String>())
            })
            .collect();
        let more = self.count.saturating_sub(self.sample.len());
        if more > 0 {
            lines.push(format!(
                "kept: and {}",
                plural(more, "more commit only in this home", "more commits only in this home")
            ));
        }
        lines.extend(self.witness.because().map(|why| format!("kept: {why}")));
        lines
    }

    /// The half of the line that is the same for every commit it names.
    ///
    /// The clause is two statements, and each says only what the reading earned.
    ///
    /// The first is what the reading proved about the commit. A home nothing here could
    /// check about the remote may hold the only copy and may not, and calling that "only
    /// here" would be a claim this machine did not make. That is the split
    /// [`crate::lifecycle::assess`] itself draws between a commit only here and a commit
    /// not checked, and the words follow it.
    ///
    /// The second is what became of the copies the reclaim rested on: none was recorded,
    /// or a recorded one has gone. A row whose recorded copies this reading could neither
    /// credit nor call gone gets nothing after the first statement, because there is
    /// nothing after it that is true.
    fn clause(&self) -> String {
        let proved = if matches!(self.witness, Witness::Unchecked) {
            "could not be checked"
        } else {
            "is only here"
        };
        if self.entry.rested.copies().is_empty() {
            return format!("{proved}; this home's reclaim recorded no copy outside it");
        }
        let gone: Vec<String> = self.gone.iter().map(Outside::describe).collect();
        if gone.is_empty() {
            return String::from(proved);
        }
        format!("{proved}; the copy in {} is gone", gone.join(", "))
    }
}

/// What one `nodal gc` removed, and what it would not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swept {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The trash entries that were removed, oldest first.
    pub removed: Vec<Trashed>,
    /// The entries whose retention has not run out, and are therefore still there.
    pub kept: Vec<Trashed>,
    /// The expired entries this sweep did not remove, because removing one would have
    /// taken the last copy of a commit with it, or because the home could not be read.
    #[serde(default)]
    pub held: Vec<HeldBack>,
    /// What the removed directories occupied, when it was measured.
    pub freed_bytes: Option<u64>,
    /// Runtime that was stopped because it belonged to a home that is not there.
    pub stopped: Stopped,
    /// Containers removed for the same reason.
    pub containers: Vec<String>,
    /// Leases that had lapsed and were given back.
    pub leases: Vec<String>,
    /// The pre-operation snapshot refs that were removed, because their runs are over
    /// and their retention has run out.
    pub records: Vec<String>,
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
            Field::new("kept", self.kept_cell()),
            Field::new("merged", self.retired_cell()),
            Field::new("runtime", self.runtime_cell()),
            Field::new(
                "records",
                plural(self.records.len(), "snapshot record removed", "snapshot records removed"),
            ),
        ];
        if self.idle_asked {
            fields.push(Field::new("idle", self.idle_cell()));
        }
        let mut blocks = vec![Block::fields(fields)];
        if !self.removed.is_empty() {
            blocks.push(Block::table(self.removed_table()));
        }
        blocks.extend(self.held.iter().flat_map(HeldBack::lines).map(Block::line));
        if !self.idle.is_empty() {
            blocks.push(Block::table(self.idle_table()));
        }
        if !self.leftovers.is_empty() {
            blocks.push(Block::table(self.leftovers_table()));
        }
        blocks.extend(super::ps::note_lines(&self.notes).into_iter().map(Block::line));
        Doc::from_iter(blocks)
    }
}

impl Swept {
    /// How many homes are still in the trash, and how many of them are past their
    /// retention and stayed anyway.
    ///
    /// The second number is the one worth reading. A home past its retention that is
    /// still there is one this sweep refused to remove, and the lines under the table
    /// say about which commit.
    fn kept_cell(&self) -> String {
        let kept = plural(self.kept.len() + self.held.len(), "home in trash", "homes in trash");
        if self.held.is_empty() {
            return kept;
        }
        format!("{kept}{JOIN}{} past its retention and kept", self.held.len())
    }

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

    use std::path::PathBuf;

    use super::{Leftover, Pruned, Reclaimed, plural};
    use crate::model::{Outside, Rested, Timestamp};
    use crate::output::Render;

    fn reclaimed() -> Reclaimed {
        Reclaimed {
            prunable: Vec::new(),
            now: Timestamp::parse("2026-09-07T09:00:00Z").unwrap(),
            slug: String::from("worker-import"),
            findings: Vec::new(),
            snapshot: None,
            record: None,
            stopped: crate::runtime::stop::Stopped::default(),
            containers: Vec::new(),
            released: crate::services::ports::Released::default(),
            trashed: None,
            trimmed: crate::workspace::prune::Report::default(),
            pruned: None,
            root: None,
            hooks: Vec::new(),
            notes: Vec::new(),
            leftovers: Vec::new(),
            worktree_remove: None,
        }
    }

    /// A reclaim that recorded the home names the ref, and one that could not take a
    /// record has no line to print rather than a line saying nothing.
    #[test]
    fn the_record_line_names_the_ref_the_home_was_committed_to() {
        let mut report = reclaimed();
        assert!(!report.doc().lines().join("\n").contains("record"), "no record, no line");
        report.record = Some(String::from("refs/nodal/01J/pre/01K"));
        let lines = report.doc().lines().join("\n");
        assert!(
            lines.contains("record  the home before this reclaim is on refs/nodal/01J/pre/01K"),
            "{lines}"
        );
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

    /// A reclaim that went ahead over a copy somewhere else says where that copy is.
    /// The person reads what the removal depends on, and `nodal gc` prints the same
    /// names again if the copy has gone by the time the retention runs out.
    #[test]
    fn a_verdict_that_rested_on_a_copy_names_the_repository_and_the_ref() {
        let mut report = reclaimed();
        report.trashed = Some(trashed(Rested::Safe {
            copies: vec![Outside {
                repository: PathBuf::from("/w/project"),
                references: vec![String::from("refs/remotes/origin/topic")],
                commits: 3,
            }],
        }));
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("nothing that is only here"), "{lines}");
        assert!(
            lines.contains("3 of them are also in /w/project (refs/remotes/origin/topic)"),
            "{lines}"
        );
    }

    /// A home that held no commit of its own rested on nothing, and a line saying so
    /// would answer a question nobody asked.
    #[test]
    fn a_home_with_nothing_to_hold_prints_no_line_about_where_it_is_held() {
        let mut report = reclaimed();
        report.trashed = Some(trashed(Rested::Safe { copies: Vec::new() }));
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("nothing that is only here"), "{lines}");
        assert!(!lines.contains("also in"), "{lines}");
    }

    /// A canonical identifier, for the row the properties below are about.
    const ID: &str = "01J0000000000000000000000A";

    /// One trash row, for the properties about what the check line says.
    fn trashed(rested: Rested) -> crate::model::Trashed {
        let at = Timestamp::parse("2026-09-07T09:00:00Z").unwrap();
        crate::model::Trashed {
            environment_id: crate::model::EnvId::parse(ID).unwrap(),
            unit_id: crate::model::UnitId::parse(ID).unwrap(),
            project_id: crate::model::ProjectId::parse(ID).unwrap(),
            slug: crate::model::Slug::parse("worker-import").unwrap(),
            home: PathBuf::from("/w/home"),
            path: PathBuf::from("/w/trash/home"),
            snapshot: None,
            pruned_bytes: 0,
            rested,
            trashed_at: at,
            expires_at: at,
        }
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
