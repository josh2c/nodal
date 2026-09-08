//! What `nodal done` answers with.
//!
//! The report has to make three things plain, because each of them is a promise Nodal
//! makes about what it did and did not do: which refs went, that one `git push` is the
//! whole of what left this machine, and that no pull request was opened.
//!
//! The remote's URL is deliberately not a field. An HTTPS remote may carry a token in
//! front of its host, and a report is written to a terminal, a log and a `--json`
//! consumer. What is printed is the host and the page, both built by
//! [`crate::git::host`], which drops the credentials on the way.

use serde::{Deserialize, Serialize};

use crate::model::{BranchName, Slug, Timestamp, UnitStatus};
use crate::output::Render;
use crate::output::human::{Block, Doc, Field};

/// The line that says what, exactly, went over a network.
const NETWORK: &str = "one `git push`, run through your own git; nothing else left this machine";

/// The line that says what Nodal did not do, in the one place a person would look for
/// it having been done.
const NO_PULL_REQUEST: &str = "none opened; nodal opens none";

/// What one `nodal done` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {
    /// The instant the answer was taken.
    pub now: Timestamp,
    /// The unit's handle.
    pub slug: Slug,
    /// The branch that was pushed.
    pub branch: BranchName,
    /// The remote it went to, by name.
    pub remote: String,
    /// The host that remote names, when it names one rather than a path.
    pub host: Option<String>,
    /// The refs that were sent, in the order they were given to `git push`.
    pub pushed: Vec<String>,
    /// The work-in-progress ref the push carried, when the home had a commit to make
    /// one from.
    pub snapshot: Option<String>,
    /// Where the change is opened, when the host is one Nodal knows a page for.
    pub compare: Option<String>,
    /// The state the unit is in now.
    pub status: UnitStatus,
    /// What could not be answered. A note is never a failure.
    pub notes: Vec<String>,
}

impl Render for Done {
    const KIND: &'static str = "done unit";

    fn doc(&self) -> Doc {
        let mut doc = Doc::from_iter([Block::fields(vec![
            Field::new("unit", format!("{} ({})", self.slug, status_label(self.status))),
            Field::new("pushed", self.pushed_cell()),
            Field::new("network", NETWORK),
            Field::new("compare", self.compare_cell()),
            Field::new("pull request", NO_PULL_REQUEST),
        ])]);
        for note in &self.notes {
            doc.push(Block::line(note.clone()));
        }
        doc
    }
}

impl Done {
    /// Which refs went where, named in full so the claim can be checked.
    fn pushed_cell(&self) -> String {
        let refs = self.pushed.join(" and ");
        format!("{refs} to {}", self.remote)
    }

    /// Where the change is opened, or why there is no address for it.
    fn compare_cell(&self) -> String {
        if let Some(url) = &self.compare {
            return url.clone();
        }
        match &self.host {
            Some(host) => format!("no compare page is known for {host}; the branch is pushed"),
            None => format!("{} names a path, which serves no compare page", self.remote),
        }
    }
}

/// The word a unit's state carries here. The same table `crate::output::view::unit`
/// prints, kept short rather than shared, because one word in two reports is not a
/// dependency worth making.
const fn status_label(status: UnitStatus) -> &'static str {
    match status {
        UnitStatus::Open => "open",
        UnitStatus::Review => "review",
        UnitStatus::Merged => "merged",
        UnitStatus::Archived => "archived",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::Done;
    use crate::model::{BranchName, Slug, Timestamp, UnitStatus};
    use crate::output::Render;

    fn done() -> Done {
        Done {
            now: Timestamp::parse("2026-09-07T09:00:00Z").unwrap(),
            slug: Slug::parse("worker-import").unwrap(),
            branch: BranchName::parse("nodal/worker-import").unwrap(),
            remote: String::from("origin"),
            host: Some(String::from("github.com")),
            pushed: vec![
                String::from("refs/heads/nodal/worker-import"),
                String::from("refs/nodal/01J/wip"),
            ],
            snapshot: Some(String::from("refs/nodal/01J/wip")),
            compare: Some(String::from("https://github.com/a/b/compare/nodal/worker-import")),
            status: UnitStatus::Review,
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_report_names_both_refs_and_the_one_command_that_sent_them() {
        let lines = done().doc().lines().join("\n");
        assert!(lines.contains("refs/heads/nodal/worker-import"), "{lines}");
        assert!(lines.contains("refs/nodal/01J/wip"), "{lines}");
        assert!(lines.contains("one `git push`"), "{lines}");
        assert!(lines.contains("worker-import (review)"), "{lines}");
    }

    #[test]
    fn the_report_says_that_no_pull_request_was_opened() {
        let lines = done().doc().lines().join("\n");
        assert!(lines.contains("pull request"), "{lines}");
        assert!(lines.contains("nodal opens none"), "{lines}");
    }

    #[test]
    fn a_host_with_no_known_page_is_said_so_rather_than_guessed_at() {
        let mut report = done();
        report.compare = None;
        report.host = Some(String::from("git.example.invalid"));
        let lines = report.doc().lines().join("\n");
        assert!(lines.contains("no compare page is known for git.example.invalid"), "{lines}");
    }
}
