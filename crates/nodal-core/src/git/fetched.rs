//! What the last fetch in a repository saw, read from the record Git writes for it.
//!
//! A remote-tracking ref says what a repository once saw of a remote. It never says when,
//! and it is never corrected: `git fetch` without `--prune` leaves `refs/remotes/origin/x`
//! standing over a branch the remote dropped, and `git pull` is such a fetch by default.
//! That is the ordinary shape the hour a pull request merges, and a reading that believed
//! the ref called the only copy of a commit proved on the remote.
//!
//! `FETCH_HEAD` is the record that tells the two apart. Git rewrites the whole file on
//! every fetch, with one line per ref the fetch saw:
//!
//! ```text
//! 874c61bb…\t\tbranch 'main' of /srv/git/app
//! 9491bc5f…\tnot-for-merge\tbranch 'topic' of /srv/git/app
//! ```
//!
//! A branch the remote has dropped is not in it after the next fetch, whether or not that
//! fetch pruned. So the file is a reading of the remote as the last fetch found it, the
//! sha on the line is what the fetch saw, and the file's own modification time is when.
//! That is an observation: a remote, a ref, a commit, and an instant.
//!
//! # What it cannot say, and what is read beside it
//!
//! It cannot say that the remote is right now. Nothing on this disk can, and Nodal makes
//! no network call, so what a verdict rests on is an observation with its date printed
//! beside it, and never a claim about a server.
//!
//! It also cannot tell a fetch of one branch from a fetch of all of them: a person who
//! ran `git fetch origin main` leaves a file naming `main` alone. Every other branch is
//! then unobserved, which is the refusing direction and the true one.
//!
//! What it cannot answer alone is whether an absence in the listing is an absence on the
//! remote, and the ref answers that: a fetch that pruned deleted the ref of a branch the
//! remote dropped, and a fetch of one branch by name left every other ref standing
//! ([`crate::lifecycle::witness`]).
//!
//! Nothing here starts a process. Both records are files Git wrote, and they are read
//! with `read_to_string` and `stat`.

use std::path::Path;

use super::oid::Oid;
use super::{layout, remote};
use crate::model::Timestamp;

/// The file Git rewrites on every fetch, under the repository's common git directory.
const FETCH_HEAD: &str = "FETCH_HEAD";

/// What a line says it is about, where a branch is the only kind that answers here.
const BRANCH: &str = "branch ";

/// What a line puts between the kind and the url.
const OF: &str = "' of ";

/// One ref the last fetch saw: the branch, and the commit the remote had it at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The branch name, without the remote, as the remote spells it.
    pub branch: String,
    /// The commit the fetch saw it at.
    pub oid: Oid,
}

/// What the last fetch in one repository saw of one remote, and when it saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// Every branch of that remote the fetch saw. A branch not in this was not seen.
    pub seen: Vec<Seen>,
    /// When the fetch was made, from the file Git wrote.
    pub at: Timestamp,
}

impl Observation {
    /// Where this observation found one branch, and `None` for a branch it did not see.
    #[must_use]
    pub fn branch(&self, name: &str) -> Option<&Oid> {
        self.seen.iter().find(|seen| seen.branch == name).map(|seen| &seen.oid)
    }
}

/// The last fetch this repository made of the remote at `url`, and `None` when there is
/// no record of one.
///
/// `None` covers a repository that has never fetched, whose `FETCH_HEAD` Git has not
/// written yet, and one whose last fetch was of another remote, which rewrote the file
/// with that remote's refs. Both of those are repositories that have made no observation
/// of this remote, and neither is a repository that observed the remote to be empty.
///
/// The url on each line is reduced the way every remote url is
/// ([`crate::git::remote::identity`]), because Git writes the url it was given and a
/// person may have typed it three ways.
#[must_use]
pub fn last(repo: &Path, url: &str) -> Option<Observation> {
    let path = layout::common_dir(repo)?.join(FETCH_HEAD);
    let at = written(&path)?;
    let text = std::fs::read_to_string(&path).ok()?;
    let wanted = remote::identity(url)?;
    let seen = text.lines().filter_map(|line| one(line, &wanted)).collect();
    Some(Observation { seen, at })
}

/// One line as a branch of the wanted remote, and nothing for every other line.
///
/// A line is `<sha>`, a tab, `not-for-merge` or nothing, a tab, and then what was
/// fetched. Only branches answer: a tag is not a branch, and `HEAD` is a pointer at one.
fn one(line: &str, wanted: &str) -> Option<Seen> {
    let mut fields = line.split('\t');
    let oid = Oid::parse(fields.next()?).ok()?;
    let _merge = fields.next()?;
    let what = fields.next()?;
    let named = what.strip_prefix(BRANCH)?.strip_prefix('\'')?;
    let (branch, url) = named.split_once(OF)?;
    (remote::identity(url).as_deref() == Some(wanted))
        .then(|| Seen { branch: branch.to_owned(), oid })
}

/// When a file was last written, `None` when it is not there or has no time.
fn written(path: &Path) -> Option<Timestamp> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let seconds = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Timestamp::from_unix_seconds(i64::try_from(seconds).ok()?).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::one;

    /// The two shapes Git writes, one for the branch that would be merged and one for
    /// every other branch of the same fetch.
    #[test]
    fn both_shapes_of_a_branch_line_are_read() {
        let merged = "874c61bb7f159c37afe25250e67b036ef4f7cd67\t\tbranch 'main' of /srv/git/app";
        let other = "9491bc5f30392aded4182a8601d22a0574abadb2\tnot-for-merge\tbranch 'topic' of \
                     /srv/git/app.git";
        assert_eq!(one(merged, "/srv/git/app").unwrap().branch, "main");
        assert_eq!(one(other, "/srv/git/app").unwrap().branch, "topic");
    }

    /// A branch of another remote is not an observation of this one. `backup/main` is not
    /// a reading of `origin/main`, and a fetch of one rewrites the file with its own refs.
    #[test]
    fn a_line_of_another_remote_is_not_read() {
        let line = "874c61bb7f159c37afe25250e67b036ef4f7cd67\t\tbranch 'main' of /srv/git/other";
        assert_eq!(one(line, "/srv/git/app"), None);
    }

    /// A tag is not a branch, and neither is the pointer at a default branch.
    #[test]
    fn only_a_branch_answers() {
        for kind in ["tag 'v1' of /srv/git/app", "HEAD of /srv/git/app"] {
            let line = format!("874c61bb7f159c37afe25250e67b036ef4f7cd67\t\t{kind}");
            assert_eq!(one(&line, "/srv/git/app"), None, "{kind}");
        }
    }

    /// A line of any other shape says nothing rather than stopping the reading.
    #[test]
    fn a_line_that_is_not_a_record_says_nothing() {
        assert_eq!(one("", "/srv/git/app"), None);
        assert_eq!(one("not-an-id\t\tbranch 'main' of /srv/git/app", "/srv/git/app"), None);
    }
}
