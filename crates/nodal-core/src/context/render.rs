//! The Markdown a unit's memory is written as.
//!
//! Markdown because the readers are agents and people, and both read it without a
//! parser. The shape is a contract (`docs/contracts.md`): three sections in one order,
//! headings that do not move, and one fact per line.
//!
//! Two rules hold the file together.
//!
//! Every line that came from an event is written on one line. An event body is text
//! somebody or something else chose, and text with a newline in it would otherwise be
//! able to write a heading of its own into this file and state a fact Nodal never
//! computed. So a body is flattened before it is rendered, and the file's shape is
//! Nodal's alone.
//!
//! Every list states how long it really is before it prints its first line, and says
//! what it left out. A memory that quietly kept the first forty lines of a sibling that
//! changed four hundred files reads exactly like a memory of a sibling that changed
//! forty, which is worse than saying nothing.

use crate::context::ledger::{CAP, Entry, Gained, Ledger};
use crate::context::survey::{Snapshot, reference};
use crate::git::history::FileChange;
use crate::model::Event;
use crate::output::view::event::kind_label;
use crate::output::view::unit::status_label;
use crate::runtime::run::EXIT_CODE;

/// How many lines any one list in the facts section may take.
const LIST: usize = 20;

/// The reference a test event carries its failing count under.
const FAILING: &str = "failing";

/// The reference it carries its passing count under.
const PASSING: &str = "passing";

/// The whole file.
///
/// `test_command` is the command line the project's recipe declares as its test suite,
/// when it declares one. It is what lets the memory say that the last run of the tests
/// failed: Nodal records that a command ran and what it exited with, and the recipe is
/// where the project states which command that was.
///
/// Nothing here states when the file was written. The file is a function of the
/// project, so two compiles of one unchanged project are the same bytes, and a compile
/// that changed nothing leaves the file alone rather than waking every editor and
/// agent watching it. When the file was last true is a question the filesystem already
/// answers, and answers correctly for a memory nothing has recompiled in a week.
#[must_use]
pub fn memory(subject: &Snapshot, ledger: &Ledger, test_command: Option<&str>) -> String {
    let mut lines = heading(subject);
    lines.extend(facts(subject, test_command));
    lines.extend(stated(subject));
    lines.extend(project_ledger(ledger));
    lines.extend(unread(subject));
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The title, and the warning that an edit here does not survive.
fn heading(subject: &Snapshot) -> Vec<String> {
    vec![
        format!("# {}", subject.unit.slug),
        String::new(),
        String::from(
            "<!-- Written by nodal from the project itself. Every nodal command that touches \
             this unit writes this file again, so an edit here is lost. -->",
        ),
        String::new(),
    ]
}

/// What is true of the unit now.
fn facts(subject: &Snapshot, test_command: Option<&str>) -> Vec<String> {
    let mut lines = vec![String::from("## Facts"), String::new()];
    lines.push(field("objective", &objective(subject)));
    lines.push(field("state", status_label(subject.unit.status)));
    lines.push(field("branch", subject.unit.branch.as_str()));
    lines.push(field("home", &home(subject)));
    lines.extend(standing(subject));
    lines.extend(commands(subject));
    if let Some(line) = tests(subject, test_command) {
        lines.push(line);
    }
    lines.push(String::new());
    lines
}

/// What the unit was made for.
fn objective(subject: &Snapshot) -> String {
    subject.unit.objective.as_ref().map_or_else(|| String::from("none stated"), ToString::to_string)
}

/// Where the unit's home is.
fn home(subject: &Snapshot) -> String {
    subject
        .home
        .as_ref()
        .map_or_else(|| String::from("none on this machine"), |path| path.display().to_string())
}

/// Where the branch stands, and what the tree holds.
fn standing(subject: &Snapshot) -> Vec<String> {
    let Some(work) = &subject.work else {
        return vec![field("base", "not read; see the last section")];
    };
    let commit = work
        .base_commit
        .as_ref()
        .map_or_else(|| String::from("unknown"), |oid| short(oid.as_str()));
    let mut lines = vec![
        field("base", &format!("{} at {commit}", work.base)),
        field(
            "staleness",
            &format!(
                "{} behind {}, {} ahead",
                count(as_usize(work.divergence.behind), "commit"),
                work.base,
                count(as_usize(work.divergence.ahead), "commit")
            ),
        ),
        field("merging", &work.integration.label()),
    ];
    lines.extend(list("uncommitted", "file", &files(&work.uncommitted)));
    lines.extend(list("changed since the base commit", "file", &files(&work.touched)));
    let commits: Vec<String> = work
        .commits
        .iter()
        .map(|commit| format!("{} {}", commit.short(), commit.subject))
        .collect();
    lines.extend(list("commits on this branch", "commit", &commits));
    lines
}

/// The last commands run in the unit, newest first.
fn commands(subject: &Snapshot) -> Vec<String> {
    let lines: Vec<String> = subject
        .commands
        .iter()
        .map(|event| format!("{} {} — {}", event.ts, one_line(&event.body), outcome(event)))
        .collect();
    list("last commands", "command", &lines)
}

/// What a command event says became of the command.
fn outcome(event: &Event) -> String {
    reference(event, EXIT_CODE)
        .map_or_else(|| String::from("no exit code recorded"), |code| format!("exit {code}"))
}

/// What the log says about the tests, when it says anything.
///
/// A counted result wins, because it is the number a person wants. Failing that, the
/// last run of the command the recipe calls the test suite is reported with what it
/// exited with, which is what Nodal itself watched happen.
fn tests(subject: &Snapshot, test_command: Option<&str>) -> Option<String> {
    if let Some(event) = subject.tests.first() {
        let counts = [(FAILING, "failing"), (PASSING, "passing")]
            .iter()
            .filter_map(|(name, word)| {
                reference(event, name).map(|count| format!("{count} {word}"))
            })
            .collect::<Vec<String>>();
        let body = if counts.is_empty() { one_line(&event.body) } else { counts.join(", ") };
        return Some(field("tests", &format!("{body} ({})", event.ts)));
    }
    let command = test_command?;
    let event = subject.commands.iter().find(|event| is_command(&event.body, command))?;
    Some(field("tests", &format!("{} — {} ({})", one_line(&event.body), outcome(event), event.ts)))
}

/// Whether an event body is a run of `command`, rather than of a command that starts
/// with the same word.
fn is_command(body: &str, command: &str) -> bool {
    body == command || body.strip_prefix(command).is_some_and(|rest| rest.starts_with(' '))
}

/// What somebody said, as opposed to what Nodal watched.
fn stated(subject: &Snapshot) -> Vec<String> {
    let mut lines = vec![String::from("## Stated"), String::new()];
    if subject.stated.is_empty() {
        lines.push(String::from("Nobody has stated a note or a handoff in this unit."));
    } else {
        for event in &subject.stated {
            lines.push(format!(
                "- {} {} ({}): {}",
                event.ts,
                kind_label(event.kind),
                event.actor.name,
                one_line(&event.body)
            ));
        }
    }
    lines.push(String::new());
    lines
}

/// Every other open unit, and what the base gained under this one.
fn project_ledger(ledger: &Ledger) -> Vec<String> {
    let mut lines = vec![String::from("## Project ledger"), String::new()];
    lines.extend(gained(ledger.gained.as_ref()));
    if ledger.siblings.is_empty() {
        lines.push(String::from("No other unit of this project has work off the base."));
        lines.push(String::new());
    }
    for sibling in &ledger.siblings {
        lines.extend(sibling_lines(sibling));
    }
    lines
}

/// What merged into the base since this unit left it.
fn gained(gained: Option<&Gained>) -> Vec<String> {
    let Some(gained) = gained else {
        return vec![
            String::from("What the base gained is not read; see the last section."),
            String::new(),
        ];
    };
    let mut lines = vec![
        format!(
            "### {} gained {} since this unit's base commit",
            gained.base,
            count(as_usize(gained.total), "commit")
        ),
        String::new(),
    ];
    lines.extend(bullets(&gained.commits, CAP.saturating_sub(3), "commit", "- "));
    lines.push(String::new());
    lines
}

/// One sibling, inside the cap, saying what the cap dropped.
fn sibling_lines(sibling: &Entry) -> Vec<String> {
    let mut lines =
        vec![format!("### {}", sibling.heading), String::new(), summary(sibling), String::new()];
    let labels = usize::from(!sibling.commits.is_empty()) + usize::from(!sibling.files.is_empty());
    let wanted = sibling.commits.len() + sibling.files.len();
    // The blank line that separates this sibling from the next is part of its block,
    // so it is counted against the cap rather than spent outside it.
    let fixed = lines.len() + labels + 1;
    let (commits, files, dropping) = if fixed + wanted <= CAP {
        (sibling.commits.len(), sibling.files.len(), false)
    } else {
        let room = CAP.saturating_sub(fixed + 1);
        let (commits, files) = share(room, sibling.commits.len(), sibling.files.len());
        (commits, files, true)
    };
    lines.extend(section("commits:", &sibling.commits, commits));
    lines.extend(section("files:", &sibling.files, files));
    if dropping {
        lines.push(dropped(sibling, commits, files));
    }
    lines.push(String::new());
    lines
}

/// What one sibling has done, in one line.
fn summary(sibling: &Entry) -> String {
    let Some(base) = &sibling.base else {
        return String::from(sibling.trouble.unwrap_or("nothing could be read about it"));
    };
    format!(
        "{} against {base}, {} changed, {} uncommitted",
        count(as_usize(sibling.ahead), "commit"),
        count(sibling.files.len(), "file"),
        count(sibling.uncommitted, "file")
    )
}

/// What the cap left out, counted against what Git says is there rather than against
/// what was read: a branch with four hundred commits has three hundred and thirty-six
/// of them unread, and every one of those is also unshown.
fn dropped(sibling: &Entry, commits: usize, files: usize) -> String {
    format!(
        "… the {CAP}-line cap dropped {} and {}",
        count(as_usize(sibling.ahead).saturating_sub(commits), "commit"),
        count(sibling.files.len().saturating_sub(files), "file")
    )
}

/// One labelled part of a sibling's entry, with the first `take` of its lines.
fn section(label: &str, lines: &[String], take: usize) -> Vec<String> {
    if take == 0 {
        return Vec::new();
    }
    let mut out = vec![String::from(label)];
    out.extend(lines.iter().take(take).map(|line| format!("- {line}")));
    out
}

/// Divide the room between two lists: half each, and whatever half the shorter list
/// does not need goes to the longer one.
fn share(room: usize, commits: usize, files: usize) -> (usize, usize) {
    let half = room / 2;
    let taken = commits.min(half.max(room.saturating_sub(files)));
    (taken, files.min(room - taken))
}

/// A labelled list in the facts section: how long it is, then its first [`LIST`]
/// lines, then what was left out.
fn list(label: &str, noun: &str, lines: &[String]) -> Vec<String> {
    let mut out = vec![field(label, &count(lines.len(), noun))];
    out.extend(bullets(lines, LIST, noun, "  - "));
    out
}

/// At most `cap` bullets, and a line saying how many more there were.
fn bullets(lines: &[String], cap: usize, noun: &str, mark: &str) -> Vec<String> {
    let mut out: Vec<String> = lines.iter().take(cap).map(|line| format!("{mark}{line}")).collect();
    if lines.len() > cap {
        out.push(format!("{mark}… {} more not shown", count(lines.len() - cap, noun)));
    }
    out
}

/// A count, said the way English says it.
fn count(number: usize, noun: &str) -> String {
    match number {
        0 => format!("no {noun}s"),
        1 => format!("1 {noun}"),
        many => format!("{many} {noun}s"),
    }
}

/// A Git count as a length. Every count Git gives fits, and one that somehow does not
/// reads as the largest length there is rather than as a smaller number.
fn as_usize(number: u32) -> usize {
    usize::try_from(number).unwrap_or(usize::MAX)
}

/// One fact.
fn field(label: &str, value: &str) -> String {
    format!("- {label}: {value}")
}

/// The files of a change list, as lines.
fn files(changes: &[FileChange]) -> Vec<String> {
    changes
        .iter()
        .map(|change| match &change.origin {
            Some(origin) => {
                format!("{} {} (from {})", change.letter(), change.path.display(), origin.display())
            }
            None => format!("{} {}", change.letter(), change.path.display()),
        })
        .collect()
}

/// What could not be read, so that a missing fact is never read as an absent one.
fn unread(subject: &Snapshot) -> Vec<String> {
    if subject.notes.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![String::from("## Not read"), String::new()];
    lines.extend(subject.notes.iter().map(|note| format!("- {}", one_line(note))));
    lines.push(String::new());
    lines
}

/// The short form of an object identifier, as Git prints it.
fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

/// One line of text, whatever the text held.
///
/// Every character that would end a line, start a heading of its own, or leave a
/// control code in the file becomes a space. The file states what Nodal computed, and
/// nothing that arrives in it from an event body may change its shape.
fn one_line(text: &str) -> String {
    let flattened: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flattened.trim().to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{CAP, Entry, one_line, share, sibling_lines};

    fn entry(commits: usize, files: usize) -> Entry {
        Entry {
            heading: String::from("payroll-export · nodal/payroll-export"),
            base: Some(String::from("refs/remotes/origin/main")),
            ahead: u32::try_from(commits).unwrap(),
            uncommitted: 0,
            commits: (0..commits).map(|index| format!("c0{index} commit {index}")).collect(),
            files: (0..files).map(|index| format!("M file-{index}.ts")).collect(),
            trouble: None,
        }
    }

    #[test]
    fn a_small_sibling_is_printed_whole_and_says_nothing_about_a_cap() {
        let lines = sibling_lines(&entry(2, 3));
        assert!(lines.len() <= CAP, "{lines:?}");
        assert!(lines.iter().all(|line| !line.contains("cap")), "{lines:?}");
        assert_eq!(lines.iter().filter(|line| line.starts_with("- M file")).count(), 3);
    }

    #[test]
    fn a_large_sibling_is_capped_and_says_what_the_cap_dropped() {
        let lines = sibling_lines(&entry(30, 400));
        assert!(lines.len() <= CAP, "the cap is a cap: {}", lines.len());
        let dropped = lines.iter().find(|line| line.starts_with('…')).unwrap();
        assert!(dropped.contains("dropped"), "{dropped}");
        let shown_files = lines.iter().filter(|line| line.starts_with("- M file")).count();
        let shown_commits = lines.iter().filter(|line| line.starts_with("- c0")).count();
        assert!(shown_files > 0 && shown_commits > 0, "both halves are represented");
        assert!(dropped.contains(&(400 - shown_files).to_string()), "{dropped}");
        assert!(dropped.contains(&(30 - shown_commits).to_string()), "{dropped}");
    }

    #[test]
    fn a_sibling_with_one_long_list_gives_the_room_to_that_list() {
        let lines = sibling_lines(&entry(0, 400));
        assert_eq!(lines.len(), CAP, "the room is spent, not left: {lines:#?}");
        let shown = lines.iter().filter(|line| line.starts_with("- M file")).count();
        assert!(shown >= CAP - 8, "one list may have all the room there is: {shown}");
    }

    #[test]
    fn the_room_is_shared_and_never_over_spent() {
        for (room, commits, files) in [(10, 3, 50), (10, 50, 3), (10, 50, 50), (0, 5, 5)] {
            let (took_commits, took_files) = share(room, commits, files);
            assert!(took_commits + took_files <= room, "{room} {commits} {files}");
            assert!(took_commits <= commits && took_files <= files);
        }
    }

    #[test]
    fn a_body_with_a_heading_in_it_cannot_write_a_heading() {
        assert_eq!(
            one_line("done\n## Facts\n- objective: mine"),
            "done ## Facts - objective: mine"
        );
    }
}
