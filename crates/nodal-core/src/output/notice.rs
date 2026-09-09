//! Recurring notices: one line per cause, whatever the number of things it is about.
//!
//! A note states what a command could not do. Most notes are about one thing, and the
//! shape they were written in — `"{subject}: {why}"` — says so plainly. A few are about
//! every unit of the project at once. The pointer notice is the one a person meets
//! first: a project that tracks its own `CLAUDE.md` gets the same two sentences for
//! every unit, on every `nodal ls`, so a project of eight units prints sixteen lines
//! that say two things.
//!
//! This module is the rule that stops that. A notice keeps *why* apart from *what it
//! is about*, and one pass over a command's notices prints each distinct cause once:
//!
//! ```text
//! 3 units: the project tracks CLAUDE.md, so nodal did not write in it
//! ```
//!
//! The count is of subjects, not of notices, because the same cause reaching the list
//! twice for one unit is still one unit. Where there are few enough subjects to name,
//! they are named instead of counted: `env, cwd: /proc is not on this host` tells a
//! person which two signals went quiet, which a bare "2 signals" would not.
//!
//! Collapsing happens where a command reports, not where a note is made. The producer
//! states one fact per thing it could not do; how many of those become how many lines
//! is a reading decision, and it is made here for every command the same way.

/// How many subjects are named before a cause counts them instead.
///
/// Two is the number that keeps both readings honest. A cause about one or two things
/// can afford to say which, and every list longer than that is a list a person scans
/// for the count rather than for the names.
const NAMED: usize = 2;

/// One thing a command could not do: why, and what it was about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What the cause was about, when it was about a nameable thing. The unit whose
    /// home could not be written, the signal that could not run.
    pub subject: Option<String>,
    /// Why, as one line, and stated the same way for every subject. Two notices are
    /// collapsed when this text matches, so it must carry nothing that varies per
    /// subject — a cause that names the unit it is about can never be collapsed.
    pub cause: String,
}

impl Notice {
    /// A notice about one named thing.
    pub fn about(subject: impl Into<String>, cause: impl Into<String>) -> Self {
        Self { subject: Some(subject.into()), cause: cause.into() }
    }

    /// A notice about the run rather than about any one thing.
    pub fn general(cause: impl Into<String>) -> Self {
        Self { subject: None, cause: cause.into() }
    }
}

/// The lines to print: one per distinct cause, in the order the causes first appeared.
///
/// `plural` names what the subjects are, for the causes that have too many to list.
/// Pass the plural: "units", "signals".
#[must_use]
pub fn collapse(notices: &[Notice], plural: &str) -> Vec<String> {
    let mut causes: Vec<(&str, Vec<&str>)> = Vec::new();
    for notice in notices {
        let at = causes.iter().position(|(cause, _)| *cause == notice.cause).unwrap_or_else(|| {
            causes.push((&notice.cause, Vec::new()));
            causes.len() - 1
        });
        // The same cause reaching the list twice for one subject is still one subject:
        // a unit whose two pointer files are both tracked is one unit, not two.
        if let Some(subject) = notice.subject.as_deref()
            && !causes[at].1.contains(&subject)
        {
            causes[at].1.push(subject);
        }
    }
    causes.into_iter().map(|(cause, subjects)| line(cause, &subjects, plural)).collect()
}

/// One collapsed line: the subjects named, counted, or left out when there are none.
fn line(cause: &str, subjects: &[&str], plural: &str) -> String {
    match subjects.len() {
        0 => cause.to_owned(),
        count if count <= NAMED => format!("{}: {cause}", subjects.join(", ")),
        count => format!("{count} {plural}: {cause}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Notice, collapse};

    /// The reported shape: the pointer notice, once per unit, every invocation.
    const TRACKED: &str = "the project tracks CLAUDE.md, so nodal did not write in it";

    fn tracked(units: &[&str]) -> Vec<Notice> {
        units.iter().map(|unit| Notice::about(*unit, TRACKED)).collect()
    }

    #[test]
    fn one_cause_about_many_units_is_one_line_and_a_count() {
        let lines = collapse(&tracked(&["auth", "export", "import"]), "units");
        assert_eq!(lines, vec![format!("3 units: {TRACKED}")]);
    }

    /// The property the field report asked for: the number of lines does not grow with
    /// the number of units.
    #[test]
    fn the_line_count_does_not_grow_with_the_unit_count() {
        let one = collapse(&tracked(&["auth"]), "units");
        let many = collapse(&tracked(&["a", "b", "c", "d", "e", "f", "g", "h"]), "units");
        assert_eq!(one.len(), 1);
        assert_eq!(many.len(), one.len());
    }

    #[test]
    fn a_short_list_of_subjects_is_named_rather_than_counted() {
        let lines = collapse(&tracked(&["auth", "export"]), "units");
        assert_eq!(lines, vec![format!("auth, export: {TRACKED}")]);
    }

    #[test]
    fn one_subject_keeps_the_shape_a_note_has_always_had() {
        let lines = collapse(&tracked(&["auth"]), "units");
        assert_eq!(lines, vec![format!("auth: {TRACKED}")]);
    }

    #[test]
    fn a_notice_about_the_run_prints_its_cause_alone() {
        let lines = collapse(&[Notice::general("git could not be asked what it tracks")], "units");
        assert_eq!(lines, vec![String::from("git could not be asked what it tracks")]);
    }

    #[test]
    fn one_subject_that_reports_a_cause_twice_is_still_one_subject() {
        let twice = vec![Notice::about("auth", TRACKED), Notice::about("auth", TRACKED)];
        assert_eq!(collapse(&twice, "units"), vec![format!("auth: {TRACKED}")]);
    }

    #[test]
    fn different_causes_keep_the_order_they_first_appeared_in() {
        let notices = vec![
            Notice::about("auth", "second cause"),
            Notice::about("export", "first cause"),
            Notice::about("import", "second cause"),
        ];
        let lines = collapse(&notices, "units");
        assert_eq!(
            lines,
            vec![String::from("auth, import: second cause"), String::from("export: first cause"),]
        );
    }

    #[test]
    fn nothing_to_report_is_no_lines() {
        assert!(collapse(&[], "units").is_empty());
    }
}
