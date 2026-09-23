//! What `nodal reclaim --check` answers with: what a reclaim would take away.
//!
//! One value, two renderings, and neither of them does any part of a reclaim. The
//! command runs no hook, sends no signal, touches no container, releases no port, takes
//! no snapshot, moves nothing to the trash, writes no registry row and reaches no
//! remote. It reads, and it prints what it read.
//!
//! The verdict is the line to read first, and it is not a second opinion. It is the
//! [`Verdict`] the kernel answers over the same reading a `nodal reclaim` refuses with
//! ([`crate::lifecycle::kernel::judge`]), and this view renders it. A preflight that said
//! safe where the reclaim refuses would be worse than no preflight at all, because a
//! person would stop checking.
//!
//! What the answer says that a refusal cannot:
//!
//! | line | the question it answers |
//! |---|---|
//! | `commits` | what is only here, what this disk has twice, what a reading proves the remote has, and what nothing checked |
//! | `content` | which of the refused commits a remote tip already holds the tree of, under another identifier |
//! | `files` | what a person wrote that no commit holds |
//! | `state` | what a tool writes again, and what it does not |
//! | `runtime` | what the reclaim would stop, and what would make it refuse |
//! | `trash` | where the home would go, and that nothing went there |
//!
//! A count is exact and a sample is a sample. A person deciding what to do next is not
//! helped by forty file names, and is misled by a list that looks complete and is not,
//! so every group prints its count and then as much of itself as fits.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::doctor::size::Bytes;
use crate::lifecycle::assess::{
    Assessment, CommitGroup, Copies, PathGroup, Reason, Runtime, SameContent,
};
use crate::lifecycle::kernel::Verdict;
use crate::lifecycle::uniqueness::Witness;
use crate::model::Timestamp;
use crate::model::reading::{Answered, Reading, Refs, Store};
use crate::output::Render;
use crate::output::human::{self, Block, Doc, Field, JOIN, NONE};
use crate::runtime::attribute::{Note, Source, Standing};

/// What a verdict rests on, as the lines both the preflight and the reclaim print.
///
/// One renderer for one record, because `nodal reclaim --check` and `nodal reclaim` are
/// the preflight and the operation it is a preflight for, and a person comparing the two
/// must not have to work out whether two differently worded paragraphs are saying the
/// same thing.
pub(crate) fn rests_on(record: &Reading, runtime: Option<&Runtime>) -> String {
    let mut lines = vec![stores_line(&record.stores), refs_line(&record.refs)];
    lines.extend(runtime.map(table_line));
    for gap in &record.not_checked {
        lines.push(format!("not checked — {}: {}", gap.what, gap.why));
    }
    lines.join("\n")
}

/// Which object stores were asked for a second copy, and what each said.
fn stores_line(stores: &[Store]) -> String {
    if stores.is_empty() {
        return String::from("stores: none on this machine to ask");
    }
    let answered = stores.iter().filter(|store| store.answered == Answered::Yes).count();
    let shown: Vec<String> = stores
        .iter()
        .take(NAMED)
        .map(|store| {
            // The word is about the reading and never about the holding. A store that
            // answered may well hold nothing, and this line says it was read, so that a
            // reader does not take being named here for being offered as a copy.
            let what = match store.answered {
                Answered::Yes => String::from("read"),
                Answered::No => {
                    format!("would not answer ({})", store.why.as_deref().unwrap_or(""))
                }
                Answered::NotAsked => {
                    format!("not asked ({})", store.why.as_deref().unwrap_or(""))
                }
            };
            format!("{} {what}", store.path.display())
        })
        .collect();
    format!(
        "stores asked for a second copy: {answered} of {} read — {}",
        stores.len(),
        named(&shown, stores.len())
    )
}

/// Which refs of the home the assessed commits were taken from, and which were not.
fn refs_line(walked: &Refs) -> String {
    format!(
        "refs: walked {} ({} commit{}); not walked {}",
        if walked.walked.is_empty() { String::from("none") } else { walked.walked.join(JOIN) },
        walked.commits,
        if walked.commits == 1 { "" } else { "s" },
        if walked.not_walked.is_empty() {
            String::from("nothing")
        } else {
            walked.not_walked.join(JOIN)
        }
    )
}

/// How much of the process table the reading got, and what occupancy meant on this host.
///
/// Off the runtime itself, which is the value the kernel judged
/// ([`crate::lifecycle::kernel::Evidence::runtime`]), so the line and the answer cannot
/// describe two different readings.
fn table_line(runtime: &Runtime) -> String {
    let occupancy = if runtime.occupancy.is_empty() {
        String::from("nothing was read")
    } else {
        format!("occupancy is {}", runtime.occupancy.join(JOIN))
    };
    format!(
        "process table: {}{} — {} read, {} withheld; {occupancy}",
        runtime.reach().label(),
        runtime.at.as_deref().map(|at| format!(" {at}")).unwrap_or_default(),
        runtime.read,
        runtime.withheld,
    )
}

/// How many names a line prints before it says how many more there are.
const NAMED: usize = 6;

/// What one reclaim would do, without doing any of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preflight {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit's handle.
    pub slug: String,
    /// Whether a reclaim run now would go ahead rather than refuse.
    ///
    /// The kernel's verdict over [`Preflight::assessment`], which is the verdict the
    /// operation itself acts on. It is written out here because it is the field a script
    /// gates on, and a script must not have to re-derive a verdict.
    pub safe_to_reclaim: bool,
    /// Where the home would go, for a home Nodal made. `None` for a checkout adopted in
    /// place, which a reclaim unregisters and leaves exactly where it is.
    ///
    /// Nothing is moved there by this command.
    pub trash: Option<PathBuf>,
    /// The reading itself.
    #[serde(flatten)]
    pub assessment: Assessment,
    /// What the same reading says when every unit of the set goes at once.
    ///
    /// `None` for a check of one unit, which has no set to be part of and nothing joint
    /// to say about itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub together: Option<Together>,
}

/// What a reclaim of a whole set would find about one of its units.
///
/// Beside the per-unit answer and never instead of it. Each unit's own verdict is what a
/// reclaim of that unit alone would do, and it stays exactly what it was; this is what the
/// same reading says when every home in the set goes at once.
///
/// The reasons come from one [`Verdict`], which is the kernel's answer over the same
/// reading with the set handed to it ([`crate::lifecycle::kernel::judge`]). There is no
/// second rule here: the reasons are a rendering of the verdict.
///
/// The verdict itself is read off those reasons rather than stored beside them, for the
/// reason [`SameContent`] reads its disposition off its own type: a stored verdict is one
/// that can drift from what it is a verdict of. [`Serialize`] writes it out, so a script
/// gates on `safe` without re-deriving it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Together {
    /// Every reason, ranked, with the per-unit ones first and the joint ones added.
    pub reasons: Vec<Reason>,
}

impl Together {
    /// What a reclaim of the whole set would find about this unit, from the joint verdict.
    #[must_use]
    pub fn of(verdict: &Verdict) -> Self {
        Self { reasons: verdict.reasons() }
    }

    /// Whether a reclaim of the whole set would go ahead over this unit.
    ///
    /// The same predicate [`Verdict::safe`] answers, over the reasons that verdict printed,
    /// so the two agree wherever the reasons do.
    #[must_use]
    pub fn safe(&self) -> bool {
        !self.reasons.iter().any(|reason| reason.needs.refuses())
    }
}

impl Serialize for Together {
    /// The reasons, with the verdict read off them by [`Together::safe`].
    fn serialize<S: serde::Serializer>(&self, out: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;

        let mut joint = out.serialize_struct("Together", 2)?;
        joint.serialize_field("safe", &self.safe())?;
        joint.serialize_field("reasons", &self.reasons)?;
        joint.end()
    }
}

/// What a reclaim of several units would do, without doing any of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preflights {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// One answer per unit, in the order they were named.
    pub units: Vec<Preflight>,
    /// Whether a reclaim of every one of them would go ahead.
    ///
    /// Not the conjunction of the per-unit verdicts. A pair of units that each hold the
    /// other's only second copy are each safe alone and are not safe together.
    pub safe_together: bool,
}

impl Preflights {
    /// The answer over a set of units already read and already joined.
    #[must_use]
    pub fn new(now: Timestamp, units: Vec<Preflight>) -> Self {
        let safe_together = units.iter().all(|unit| match &unit.together {
            Some(together) => together.safe(),
            None => unit.safe_to_reclaim,
        });
        Self { now, units, safe_together }
    }
}

impl Render for Preflights {
    const KIND: &'static str = "reclaim preflight";

    fn doc(&self) -> Doc {
        let mut doc = Doc::new();
        for unit in &self.units {
            for block in unit.doc().blocks() {
                doc.push(block.clone());
            }
            doc.push(Block::blank());
        }
        doc.push(Block::fields(vec![Field::new("together", self.verdict_cell())]));
        doc
    }
}

impl Preflights {
    /// The joint verdict, and what it rests on.
    fn verdict_cell(&self) -> String {
        if self.safe_together {
            return format!(
                "safe — a reclaim of all {} would go ahead; no copy any of them relies on \
                 lives only in another of them",
                self.units.len()
            );
        }
        format!(
            "refuse — a reclaim of all {} would stop; per-unit safety is not joint safety",
            self.units.len()
        )
    }
}

impl Preflight {
    /// The answer for one unit, from one reading of its home.
    #[must_use]
    pub fn new(
        now: Timestamp,
        slug: String,
        trash: Option<PathBuf>,
        assessment: Assessment,
    ) -> Self {
        let verdict = assessment.verdict(Vec::new(), now);
        Self { now, slug, safe_to_reclaim: verdict.safe(), trash, assessment, together: None }
    }

    /// Record what a reclaim of the whole set would find about this unit.
    ///
    /// The per-unit verdict above is untouched, for the reason [`Together`] exists: both
    /// answers are true and a person needs the one that matches what they are about to do.
    pub fn together(&mut self, verdict: &Verdict) {
        self.together = Some(Together::of(verdict));
    }
}

impl Render for Preflight {
    const KIND: &'static str = "reclaim preflight";

    fn doc(&self) -> Doc {
        let mut fields = vec![
            Field::new("unit", self.slug.clone()),
            Field::new("home", self.assessment.home.display().to_string()),
            Field::new("verdict", self.verdict_cell()),
            Field::new("because", self.because_cell()),
            Field::new("commits", self.commits_cell()),
            Field::new("content", self.content_cell()),
            Field::new("files", self.paths_cell(false)),
            Field::new("state", self.paths_cell(true)),
            Field::new("runtime", self.runtime_cell()),
            Field::new("trash", self.trash_cell()),
            Field::new("with the rest", self.together_cell()),
            Field::new("rests on", self.evidence_cell()),
        ];
        fields.retain(|field| !field.value.is_empty());
        let mut doc = Doc::from_iter([Block::fields(fields)]);
        for note in &self.assessment.notes {
            doc.push(Block::line(note.clone()));
        }
        if let Some(runtime) = &self.assessment.runtime {
            for line in super::ps::note_lines(&runtime.notes) {
                doc.push(Block::line(line));
            }
        }
        doc.push(Block::line(String::from("this command changed nothing")));
        doc
    }
}

impl Preflight {
    /// Safe or refuse, in the words of the operation it is about.
    fn verdict_cell(&self) -> String {
        if self.safe_to_reclaim {
            String::from("safe — a reclaim would go ahead")
        } else {
            String::from("refuse — a reclaim would stop and change nothing")
        }
    }

    /// Every reason, ranked, most actionable first.
    fn because_cell(&self) -> String {
        if self.assessment.reasons.is_empty() {
            // A home that moves has no reason only when the table was read and nothing
            // stands in it. A home that does not move says that instead, because its table
            // may not have been read at all.
            return String::from(if self.assessment.moves {
                "nothing that is only here, and nothing standing in the home"
            } else {
                "nothing that is only here, and the home is not moved"
            });
        }
        self.assessment
            .reasons
            .iter()
            .map(|reason| format!("{}: {}", reason.needs.label(), reason.detail))
            .collect::<Vec<String>>()
            .join("\n")
    }

    /// One line per commit disposition, with what it means for the work.
    fn commits_cell(&self) -> String {
        if self.assessment.commits.is_empty() {
            return String::from("none this checkout does not already hold");
        }
        self.assessment.commits.iter().map(commit_line).collect::<Vec<String>>().join("\n")
    }

    /// One line per refused commit whose tree a remote tip already holds.
    ///
    /// Empty for every home this is not true of, and the field is then dropped. It never
    /// says safe: the tree is one object and the commit is still only here, which is
    /// what the line states and what the verdict above it goes on saying.
    fn content_cell(&self) -> String {
        self.assessment.content.iter().map(SameContent::line).collect::<Vec<String>>().join("\n")
    }

    /// The working tree, or the ignored state, whichever was asked for.
    ///
    /// Two lines of one table, split because they answer different questions. What a
    /// person wrote is work; what a tool wrote is weight.
    fn paths_cell(&self, ignored: bool) -> String {
        let lines: Vec<String> = self
            .assessment
            .paths
            .iter()
            .filter(|group| group.bytes.is_some() == ignored)
            .map(path_line)
            .collect();
        lines.join("\n")
    }

    /// What would be stopped, and what would stop the reclaim.
    fn runtime_cell(&self) -> String {
        let Some(runtime) = &self.assessment.runtime else {
            return String::new();
        };
        let mut lines = vec![owned_line(runtime)];
        if runtime.bystanders.is_empty() {
            lines.push(String::from(if unread(&runtime.notes, Source::Environment) {
                "nothing was found standing in the home, and the process table could not be read"
            } else {
                "nothing else is standing in the home"
            }));
        } else {
            lines.push(format!(
                "standing in the home, never signalled: {}{}",
                {
                    let standing: Vec<String> =
                        runtime.bystanders.iter().map(Standing::label).collect();
                    let total = standing.len();
                    named(&standing, total)
                },
                if self.assessment.moves {
                    "; a reclaim would refuse to move the home"
                } else {
                    "; the home is not moved, so it stops nothing"
                }
            ));
        }
        lines.join("\n")
    }

    /// What this verdict rests on: the positive record, printed whether it is safe or
    /// not.
    ///
    /// **This is the half a refusal never needed and a safe verdict always did.** A
    /// refusal names what it refused over, so it is its own evidence. A safe verdict used
    /// to print the absence of objections and nothing else, so a home with nothing in it
    /// and a home whose work fell outside what the predicate walks printed the same three
    /// empty lists. Nothing here changes the verdict above it; it says what the verdict
    /// was made of, so that a person or a test can disagree with it.
    fn evidence_cell(&self) -> String {
        rests_on(&self.assessment.reading, self.assessment.runtime.as_ref())
    }

    /// What a reclaim of the whole set would find about this unit, when one was asked.
    ///
    /// Empty for a check of one unit, and the field is then dropped. Where the two
    /// verdicts agree the line says so in one clause, because the interesting case is the
    /// one where they do not.
    fn together_cell(&self) -> String {
        let Some(together) = &self.together else { return String::new() };
        if together.safe() {
            return String::from("safe with the other units named here too");
        }
        let joined: Vec<String> = together
            .reasons
            .iter()
            .map(|reason| format!("{}: {}", reason.needs.label(), reason.detail))
            .collect();
        format!("refuse with the other units named here — {}", joined.join(JOIN))
    }

    /// Where the home would go, and that it did not go there.
    fn trash_cell(&self) -> String {
        match &self.trash {
            Some(path) => format!("would move to {}; nothing was moved", path.display()),
            None => String::from("the home is a checkout adopted in place; a reclaim leaves it"),
        }
    }
}

/// One commit disposition as a line: how many, which, and what it means.
fn commit_line(group: &CommitGroup) -> String {
    let sample = named(
        &group
            .sample
            .iter()
            .map(|oid| oid.as_str().chars().take(8).collect())
            .collect::<Vec<String>>(),
        group.count,
    );
    // The clause belongs to a row a reclaim would keep: it says the two ways a remote
    // goes unproved, which is not a question a row that survives the reclaim leaves
    // open. It is the same clause a refusal prints, from the same value, so a person who
    // reads the preflight and then the refusal is given one account of one reading. The
    // sentence and the clause are read off one branch, so the two cannot disagree.
    //
    // The clause goes on a line of its own under the row. With the sample and the
    // sentence it ran past two hundred columns in one cell, and a cell that carries an
    // instruction puts the instruction where a person can read it, as `runtime` does.
    let (means, because) = if group.copies.survives() {
        ("removing this home does not lose it", None)
    } else {
        ("a reclaim keeps this home", group.copies.witness().and_then(Witness::because))
    };
    let row = format!(
        "{} ({}): {sample} — {means}{}",
        group.copies.label(),
        group.count,
        holder(&group.copies)
    );
    match because {
        Some(clause) => format!("{row}\n{clause}"),
        None => row,
    }
}

/// Which repository holds the second copy, for the one disposition that rests on one.
///
/// The store was always in the reading and only `--json` printed it. It is the whole of
/// what "removing this home does not lose it" rests on, and it is what a joint reading
/// then discounts when that repository goes too, so a person reading the line has to be
/// able to see which directory is being relied on.
fn holder(copies: &Copies) -> String {
    match copies {
        Copies::SecondLocalCopy { held_by } => format!(", held by {}", held_by.display()),
        _ => String::new(),
    }
}

/// One path disposition as a line: how many, which, what it holds, and why.
fn path_line(group: &PathGroup) -> String {
    let names: Vec<String> = group.sample.iter().map(|path| path.display().to_string()).collect();
    let weight = group.bytes.as_ref().map_or_else(String::new, weight_of);
    format!(
        "{} ({}): {}{weight} — {}",
        group.held.label(),
        group.count,
        named(&names, group.count),
        group.held.why(group.fate)
    )
}

/// What a group holds, and the one thing that figure is not.
fn weight_of(bytes: &Bytes) -> String {
    let floor = if bytes.complete { "" } else { " at least" };
    format!(",{floor} {} apparent", human::bytes(bytes.apparent))
}

/// What a reclaim would stop, counted by the record that names it.
///
/// A count only where the signal answered. "0 processes by id" and "I could not read the
/// process table" are different claims, and printing the first for the second is the one
/// thing a preflight must not do.
fn owned_line(runtime: &Runtime) -> String {
    let parts = [
        plural(runtime.groups.len(), "recorded group", "recorded groups"),
        counted(
            &runtime.notes,
            Source::Environment,
            runtime.processes.len(),
            "process by id",
            "processes by id",
        ),
        counted(
            &runtime.notes,
            Source::Docker,
            runtime.containers.len(),
            "container",
            "containers",
        ),
    ];
    format!("would stop: {}", parts.join(JOIN))
}

/// The count a signal produced, or the fact that the signal could not be read.
///
/// Shared with the reclaim report, which counts what it stopped from the same notes.
pub(super) fn counted(
    notes: &[Note],
    signal: Source,
    count: usize,
    one: &str,
    many: &str,
) -> String {
    if unread(notes, signal) {
        return format!("{many} could not be read");
    }
    plural(count, one, many)
}

/// Whether one signal went unread.
fn unread(notes: &[Note], signal: Source) -> bool {
    notes.iter().any(|note| note.unread(signal))
}

/// The first [`NAMED`] of these, and how many of `total` were not named.
///
/// `total` is the group's exact count and `items` is the sample the reading kept, which
/// is already shorter. Both truncations have to be counted once and in one place: a line
/// that took its tail from the sample and its tail from the count printed each of them,
/// and told a person there were "4 more and 3 more".
fn named(items: &[String], total: usize) -> String {
    if items.is_empty() {
        return String::from(NONE);
    }
    let shown = items.len().min(NAMED);
    let more = total.saturating_sub(shown);
    if more == 0 {
        return items[..shown].join(", ");
    }
    format!("{}, and {more} more", items[..shown].join(", "))
}

/// A count with the word that goes with it, so a report never says `1 processes`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::PathBuf;

    use super::{Preflight, Preflights};
    use crate::doctor::size::Bytes;
    use crate::lifecycle::assess::{
        Assessment, CommitGroup, Copies, Fate, Held, PathGroup, Runtime, SameContent,
    };
    use crate::lifecycle::uniqueness::Witness;
    use crate::model::Timestamp;
    use crate::output::Render;
    use crate::runtime::attribute::{Note, Source, Standing};

    /// The instant every reading in these properties was taken. Fixed, because a property
    /// about a verdict must not depend on the clock.
    fn now() -> Timestamp {
        Timestamp::parse("2026-09-07T09:00:00Z").unwrap()
    }

    /// A preflight over one assessment, at a fixed instant.
    fn preflight(assessment: Assessment) -> Preflight {
        Preflight::new(
            now(),
            String::from("worker-import"),
            Some(PathBuf::from("/state/project/trash/E1")),
            assessment,
        )
    }

    /// A home with nothing in it that a reclaim would stop for.
    fn clear() -> Assessment {
        Assessment {
            home: PathBuf::from("/state/project/e/E1"),
            moves: true,
            ..Assessment::default()
        }
    }

    /// The value's own words say it did nothing, on the run where it found nothing and
    /// on the run where it found everything. A person who reads this and then finds
    /// their home moved has been lied to.
    #[test]
    fn every_answer_says_that_the_command_changed_nothing() {
        let safe = preflight(clear());
        assert!(safe.safe_to_reclaim);
        let lines = safe.doc().lines().join("\n");
        assert!(lines.contains("this command changed nothing"), "{lines}");
        assert!(lines.contains("nothing was moved"), "{lines}");

        let mut refusing = clear();
        refusing.paths.push(group(Held::Untracked));
        refusing.ranked(now());
        let lines = preflight(refusing).doc().lines().join("\n");
        assert!(lines.contains("this command changed nothing"), "{lines}");
        assert!(lines.contains("refuse"), "{lines}");
    }

    /// One path group of one path.
    fn group(held: Held) -> PathGroup {
        PathGroup {
            held,
            fate: Fate::Trashed,
            count: 1,
            sample: vec![PathBuf::from("scratch.md")],
            bytes: None,
        }
    }

    /// The verdict word and the exit code a script reads are one value, and the reasons
    /// under it are what the same reclaim would refuse with.
    #[test]
    fn the_verdict_says_which_way_the_reclaim_would_go() {
        assert!(preflight(clear()).doc().lines().join("\n").contains("safe — a reclaim would go"));
        let mut refusing = clear();
        refusing.paths.push(group(Held::Uncommitted));
        refusing.ranked(now());
        let report = preflight(refusing);
        assert!(!report.safe_to_reclaim);
        assert!(report.doc().lines().join("\n").contains("refuse — a reclaim would stop"));
    }

    /// Four dispositions, four lines, and each says what it means for the work rather
    /// than leaving a person to work it out from a word.
    #[test]
    fn each_commit_disposition_says_what_it_means_for_the_work() {
        let mut assessment = clear();
        let by = vec![PathBuf::from("/w/project")];
        for copies in [
            Copies::RemoteProved { witness: Witness::Checked { by: by.clone() } },
            Copies::SecondLocalCopy { held_by: PathBuf::from("/w/project") },
            Copies::OnlyHere { witness: Witness::NoRemote },
            Copies::NotChecked { witness: Witness::Unchecked },
        ] {
            assessment.commits.push(CommitGroup {
                copies,
                count: 1,
                sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
            });
        }
        let lines = preflight(assessment).doc().lines().join("\n");
        for label in ["proved on the remote", "second local copy", "only here", "not checked"] {
            assert!(lines.contains(label), "{label} is missing: {lines}");
        }
        assert!(lines.contains("removing this home does not lose it"), "{lines}");
        assert!(lines.contains("a reclaim keeps this home"), "{lines}");
    }

    /// The clause that names what a reading does not reach belongs to a row a reclaim
    /// would keep. On a proved row it tells a person the remote does not have the work
    /// the same line just said the remote has.
    #[test]
    fn a_row_that_survives_the_reclaim_does_not_say_the_reading_misses_it() {
        let by = vec![PathBuf::from("/w/project")];
        let line = |copies| {
            let mut assessment = clear();
            assessment.commits.push(CommitGroup {
                copies,
                count: 1,
                sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
            });
            preflight(assessment).doc().lines().join("\n")
        };

        let proved = line(Copies::RemoteProved { witness: Witness::Checked { by: by.clone() } });
        assert!(proved.contains("proved on the remote"), "{proved}");
        assert!(!proved.contains("does not reach them"), "{proved}");

        let only_here = line(Copies::OnlyHere { witness: Witness::Checked { by } });
        assert!(only_here.contains("does not reach them"), "{only_here}");

        let not_checked = line(Copies::NotChecked { witness: Witness::Unchecked });
        assert!(
            not_checked.contains("nothing here read the remote to check them"),
            "{not_checked}"
        );
    }

    /// The clause is a line under the row, not a tail on it: the row ends at the
    /// sentence, and the next line starts with the clause, whole, with the instruction
    /// it carries.
    #[test]
    fn the_witness_clause_is_a_line_under_the_commits_row() {
        let mut assessment = clear();
        assessment.commits.push(CommitGroup {
            copies: Copies::NotChecked { witness: Witness::Unchecked },
            count: 1,
            sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
        });
        let lines = preflight(assessment).doc().lines();
        let row = lines.iter().position(|line| line.contains("not checked (1)")).unwrap();
        assert!(lines[row].ends_with("a reclaim keeps this home"), "{}", lines[row]);
        let clause = lines[row + 1].trim_start();
        assert!(clause.starts_with("this home's own remote-tracking refs"), "{clause}");
        assert!(clause.ends_with("Fetch in the project checkout and reclaim again."), "{clause}");
    }

    /// The case a reading of five unit homes found and nothing answered. Two units each
    /// hold the
    /// other's only second copy: each is safe alone, and a reclaim of both loses the work.
    /// Both answers are printed, because both are true.
    #[test]
    fn two_units_that_hold_each_others_only_copy_are_safe_apart_and_not_together() {
        let homes = [PathBuf::from("/state/project/e/E1"), PathBuf::from("/state/project/e/E2")];
        let mut units = Vec::new();
        for (mine, theirs) in [(0, 1), (1, 0)] {
            let mut assessment = clear();
            assessment.home = homes[mine].clone();
            assessment.commits.push(CommitGroup {
                copies: Copies::SecondLocalCopy { held_by: homes[theirs].clone() },
                count: 2,
                sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
            });
            assessment.ranked(now());
            let mut one = preflight(assessment);
            assert!(one.safe_to_reclaim, "each unit is safe on its own");
            let joint = one.assessment.verdict(homes.to_vec(), now());
            one.together(&joint);
            units.push(one);
        }

        let report = Preflights::new(Timestamp::parse("2026-09-07T09:00:00Z").unwrap(), units);
        assert!(!report.safe_together, "the pair is not safe");
        let lines = report.doc().lines().join("\n");
        assert!(
            lines.contains("safe — a reclaim would go ahead"),
            "the per-unit one stays: {lines}"
        );
        assert!(lines.contains("refuse with the other units named here"), "{lines}");
        assert!(lines.contains("which the same removal takes"), "{lines}");
        assert!(
            lines.contains("refuse — a reclaim of all 2 would stop"),
            "the joint verdict is printed: {lines}"
        );

        let written = serde_json::to_value(&report).unwrap();
        assert_eq!(written["safe_together"], serde_json::json!(false));
        assert_eq!(written["units"][0]["safe_to_reclaim"], serde_json::json!(true));
        assert_eq!(written["units"][0]["together"]["safe"], serde_json::json!(false));
    }

    /// A copy held by something the reclaim does not remove counts, and the two verdicts
    /// agree.
    #[test]
    fn a_copy_outside_the_set_still_counts_when_the_set_goes() {
        let mut assessment = clear();
        assessment.commits.push(CommitGroup {
            copies: Copies::SecondLocalCopy { held_by: PathBuf::from("/w/project") },
            count: 1,
            sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
        });
        assessment.ranked(now());
        let mut one = preflight(assessment);
        let homes = [PathBuf::from("/state/project/e/E1")];
        let joint = one.assessment.verdict(homes.to_vec(), now());
        one.together(&joint);

        assert!(one.safe_to_reclaim);
        let report = Preflights::new(Timestamp::parse("2026-09-07T09:00:00Z").unwrap(), vec![one]);
        assert!(report.safe_together);
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("safe with the other units named here too"), "{lines}");
        assert!(lines.contains("safe — a reclaim of all 1 would go ahead"), "{lines}");
    }

    /// A row that says the content is elsewhere never says the commit is. The verdict
    /// above it is the verdict it would have been with no row at all.
    #[test]
    fn same_content_under_another_id_is_named_and_moves_no_verdict() {
        let mut assessment = clear();
        assessment.commits.push(CommitGroup {
            copies: Copies::OnlyHere { witness: Witness::NoRemote },
            count: 1,
            sample: vec![crate::git::Oid::parse(&"ab".repeat(20)).unwrap()],
        });
        assessment.ranked(now());
        let without = preflight(assessment.clone());

        assessment.content.push(SameContent {
            commit: crate::git::Oid::parse(&"ab".repeat(20)).unwrap(),
            reference: String::from("refs/nodal/origin/nodal/payroll"),
            tip: crate::git::Oid::parse(&"cd".repeat(20)).unwrap(),
            tree: crate::git::Oid::parse(&"ef".repeat(20)).unwrap(),
        });
        assessment.ranked(now());
        let with = preflight(assessment);

        assert_eq!(with.safe_to_reclaim, without.safe_to_reclaim, "the row is not evidence");
        assert!(!with.safe_to_reclaim);
        let lines = with.doc().lines().join("\n");
        assert!(lines.contains("same content as refs/nodal/origin/nodal/payroll"), "{lines}");
        assert!(lines.contains("under a different id"), "{lines}");
        assert!(lines.contains("a reclaim keeps this home"), "{lines}");

        let written = serde_json::to_string(&with).unwrap();
        assert!(written.contains("\"disposition\":\"reconstructable\""), "{written}");
    }

    /// The bytes are apparent and the line says so. A person clearing a disk who reads
    /// twelve gigabytes and gets back four has been given a number, not an answer.
    #[test]
    fn a_size_says_that_it_is_apparent_and_why_that_is_not_what_comes_back() {
        let mut assessment = clear();
        assessment.paths.push(PathGroup {
            held: Held::Generated,
            fate: Fate::Trashed,
            count: 1,
            sample: vec![PathBuf::from("target")],
            bytes: Some(Bytes {
                apparent: 4096,
                complete: true,
                exclusive_unknown: String::from("shared blocks"),
            }),
        });
        let report = preflight(assessment);
        assert!(report.doc().lines().join("\n").contains("apparent"), "{:?}", report.doc().lines());
        let written = serde_json::to_string(&report).unwrap();
        assert!(written.contains("exclusive_unknown"), "{written}");
        assert!(written.contains("\"disposition\":\"reconstructable\""), "{written}");
    }

    /// A host that could not look is never told that nothing is running.
    #[test]
    fn a_signal_that_could_not_be_read_is_never_printed_as_a_zero() {
        let mut assessment = clear();
        assessment.runtime = Some(Runtime {
            notes: vec![Note::new(
                Source::Environment,
                "a process scan reads /proc, which macos does not have",
            )],
            ..Runtime::default()
        });
        let lines = preflight(assessment).doc().lines().join("\n");
        assert!(lines.contains("processes by id could not be read"), "{lines}");
        assert!(!lines.contains("0 processes by id"), "{lines}");
        assert!(lines.contains("the process table could not be read"), "{lines}");
    }

    /// "Nothing standing in the home" is said only where the table was read. A home that
    /// moves over an unread table has a reason instead, and a home that does not move
    /// says that it does not.
    #[test]
    fn nothing_standing_in_the_home_is_said_only_where_the_table_was_read() {
        let unread = Runtime {
            notes: vec![Note::new(
                Source::Environment,
                "a process scan reads /proc, which macos does not have",
            )],
            ..Runtime::default()
        };
        for moves in [true, false] {
            let mut assessment = clear();
            assessment.moves = moves;
            assessment.runtime = Some(unread.clone());
            assessment.ranked(now());
            let report = preflight(assessment);
            let lines = report.doc().lines().join("\n");
            assert!(!lines.contains("nothing standing in the home"), "{lines}");
            assert_eq!(report.safe_to_reclaim, !moves, "{lines}");
        }
    }

    /// A bystander is named, and the line says what a reclaim would do about it.
    #[test]
    fn a_bystander_is_named_and_the_line_says_the_home_would_not_move() {
        let mut assessment = clear();
        assessment.runtime = Some(Runtime {
            bystanders: vec![Standing::new(4711, Some(String::from("tmux")))],
            ..Runtime::default()
        });
        assessment.ranked(now());
        let lines = preflight(assessment).doc().lines().join("\n");
        assert!(lines.contains("tmux (pid 4711)"), "{lines}");
        assert!(lines.contains("never signalled"), "{lines}");
        assert!(lines.contains("would refuse to move the home"), "{lines}");
    }

    /// A group says how many it did not name, once. Two truncations meet on this line —
    /// the sample the reading kept, and the names the line has room for — and a person
    /// told there are "4 more and 3 more" cannot tell how many there are.
    #[test]
    fn a_group_says_how_many_it_did_not_name_exactly_once() {
        let mut assessment = clear();
        assessment.paths.push(PathGroup {
            held: Held::Uncommitted,
            fate: Fate::Trashed,
            count: 13,
            sample: (0..10).map(|n| PathBuf::from(format!("f{n}.rs"))).collect(),
            bytes: None,
        });
        let line = preflight(assessment).doc().lines().join("\n");
        assert!(line.contains("uncommitted changes (13)"), "{line}");
        assert_eq!(line.matches("more").count(), 1, "{line}");
        assert!(line.contains("and 7 more"), "the count is not against the total: {line}");
    }

    /// The human form and `--json` are two renderings of one value: the verdict a person
    /// reads is the field a script reads.
    #[test]
    fn the_two_renderings_carry_one_verdict() {
        let mut assessment = clear();
        assessment.paths.push(group(Held::Untracked));
        assessment.ranked(now());
        let report = preflight(assessment);
        let written: serde_json::Value = serde_json::to_value(&report).unwrap();
        assert_eq!(written["safe_to_reclaim"], serde_json::json!(false));
        assert_eq!(written["reasons"][0]["needs"], serde_json::json!("unique_loss"));
        assert!(report.doc().lines().join("\n").contains("refuse"));
    }
}
