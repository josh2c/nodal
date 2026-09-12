//! The reading behind `nodal` in a repository Nodal has never seen.
//!
//! A person with twelve worktrees types `nodal` in their checkout. Nothing has been
//! initialised, no recipe is on the disk and the registry holds no row for the
//! directory. **The answer is a table, and producing it writes nothing at all**: not a
//! registry row, not a state directory, not a file in the checkout. That is the whole
//! point of the command. A tool that asked to be set up before it would say what is on
//! the disk would be asking for trust it has not earned yet, and the person with a full
//! disk and nine directories they cannot account for is the person least able to give
//! it.
//!
//! So every call here reads. `git worktree list`, `git status`, `git rev-list`, `git
//! merge-tree --write-tree`, and a walk of each directory for its size. Git runs with
//! `GIT_OPTIONAL_LOCKS=0` ([`crate::git::cmd`]), so not even the index is refreshed.
//! `merge-tree --write-tree` puts one unreachable tree object in the object database
//! and the next `git gc` takes it; nothing else touches a ref, an index or a file.
//! Nothing contacts a network: every revision compared with is one this checkout
//! already holds.
//!
//! ## What each row costs
//!
//! Per worktree, at most four Git processes and one directory walk:
//!
//! | call | what it answers |
//! |---|---|
//! | `rev-list --not --remotes` | commits that exist on no remote |
//! | `status` | paths no commit holds, and the upstream and its distance |
//! | `rev-list --left-right --count` | how far the base has moved under it |
//! | `merge-tree --write-tree` | whether merging it would change a file |
//!
//! The last is skipped for a worktree that is not ahead of the base, because a branch
//! with nothing of its own is in the base's history already and there is nothing to
//! merge. A locked worktree costs none of the four and is not walked either
//! ([`crate::doctor::worktrees`] states the rule and this module keeps it).
//!
//! ## What is measured against what
//!
//! **DONE** is measured against the checkout's own default branch, resolved the way the
//! branch audit resolves it ([`crate::doctor::branches::base_of`]): `origin/HEAD`, then
//! `main`, then `master`, and only where the repository really has one.
//!
//! **BEHIND is measured against that same branch**, and the row names it. The
//! resolution is what carries the founder's answer here: `origin/HEAD` is preferred, so
//! the reference is the default branch's own upstream where the checkout has one, and
//! the local branch only where it does not. The two are different claims — twelve
//! commits behind a remote-tracking ref fetched this morning and twelve behind a local
//! `main` nobody has pulled for a month are not the same fact about a directory
//! somebody is deciding whether to delete — so the reference is printed beside the
//! count rather than assumed.
//!
//! What BEHIND is **not** measured against is the worktree's own upstream. A branch
//! level with `origin/<itself>` is not up to date; it is a branch nobody has rebased,
//! and reading `0` against its own remote copy would say the opposite of what the
//! column is for. A checkout with no default branch at all says `unknown`, never `0`.
//!
//! ## What a failed reading does
//!
//! It leaves its fact off the row, and the row stays. A checkout that was removed under
//! Git's record, a directory this account may not read, a repository mid-rebase: each
//! costs one column of one row and nothing else. A verdict that refused to print
//! because one of twelve directories could not be opened would be worth less than the
//! eleven rows it was holding.

use std::path::{Path, PathBuf};

use crate::Result;
use crate::doctor::{branches, intent, size};
use crate::git::Git;
use crate::git::integration::Integration;
use crate::model::{ProjectName, Timestamp};
use crate::output::view::verdict::{Behind, RowKind, Verdict, WorktreeRow};
use crate::{Error, git};

/// Read the checkout at `root` and say what each of its other worktrees would cost to
/// lose.
///
/// `sessions` is where Claude Code keeps its records, when this machine has them; it is
/// what a worktree's intent is recovered from and nothing else is read out of it.
/// `project` is what the registry calls this checkout, when it holds a row for it, and
/// is carried only so the heading can say so.
///
/// `now` dates the answer, so a rendering is a function of its inputs.
///
/// # Errors
/// [`Error::Git`] when the repository's own record of its worktrees could not be read.
/// That is the one failure that ends the answer, because without the record there are
/// no rows to print.
pub fn read(
    root: &Path,
    sessions: Option<&Path>,
    project: Option<ProjectName>,
    now: Timestamp,
) -> Result<Verdict> {
    let git = Git::open(root)?;
    // Every path here is the one Git itself uses. `git worktree list` prints the
    // resolved path of each worktree, and `--show-toplevel` prints the resolved path of
    // the checkout, so the one comparison below is between two names of one shape. The
    // caller's own `root` is not compared with anything: a registry row holds whatever
    // name was written into it, and on a host whose temporary directory is a link that
    // is not the name Git answers with.
    let resolved = git.toplevel().unwrap_or_else(|_| root.to_path_buf());
    let base = branches::base_of(&git).unwrap_or_default();
    // How old the BEHIND readings are. Nothing here makes them newer: only a fetch could,
    // and Nodal makes no network call of its own, so the age is what the closing line says
    // instead of a freshness it cannot honestly claim.
    let base_moved_at = base.as_deref().and_then(|name| git::refs::last_moved(&resolved, name));
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    for registered in git.worktrees()? {
        if registered.path == resolved {
            continue;
        }
        rows.push(row(&resolved, &registered, base.as_deref(), sessions, &mut notes));
    }
    order(&mut rows);
    Ok(Verdict { checkout: resolved, project, now, base, base_moved_at, rows, notes })
}

/// Put the worktrees a person has to deal with at the top.
///
/// Three rules, in this order, and the order is the safety property. A row holding work
/// that exists nowhere else is first whatever else is true of it, because every other
/// column is a reason to remove a directory and all of them are wrong about that row.
/// Then the ones the base does not carry yet, most behind first, because that is the
/// row whose next command is a rebase. Done and empty sinks. Ties go by name, so two
/// readings of one checkout print one list.
pub fn order(rows: &mut [WorktreeRow]) {
    rows.sort_by(|left, right| {
        let key = |row: &WorktreeRow| {
            (
                !row.holds_unique_work(),
                row.done.is_integrated(),
                std::cmp::Reverse(row.behind.as_ref().map_or(0, |behind| behind.commits)),
            )
        };
        key(left).cmp(&key(right)).then_with(|| left.name.cmp(&right.name))
    });
}

/// One worktree as a row.
///
/// Named relative to the checkout when it sits inside it and by its whole path when it
/// does not, which is [`crate::doctor::worktrees`]'s rule: a relative name for a
/// worktree beside the checkout would be a walk back out of it, and a bare directory
/// name would not say where to look at all.
fn row(
    root: &Path,
    registered: &git::worktree::Registered,
    base: Option<&str>,
    sessions: Option<&Path>,
    notes: &mut Vec<String>,
) -> WorktreeRow {
    let mut row = blank(display_name(root, &registered.path), registered);

    // A lock is a statement that another tool is working in that directory. Nodal does
    // not argue with one and does not walk it either, so the row stops here.
    if let Some(reason) = &registered.locked {
        row.note = Some(locked_note(reason));
        return row;
    }
    let measured = size::measure(&registered.path);
    row.bytes = Some(measured.bytes);
    row.partial = !measured.complete;
    row.made_at = made_at(&registered.path);

    // Git's own verdict, in git's own word. The record points at a location that is not
    // there, so there is no checkout here to ask about a status or a base.
    if let Some(reason) = &registered.prunable {
        row.note = Some(prunable_note(reason));
        return row;
    }
    match Git::open(&registered.path) {
        Ok(open) => state(&mut row, &open, base, notes),
        Err(error) => {
            row.note = Some(String::from("not a checkout"));
            notes.push(format!("{}: {error}", row.name));
        }
    }
    row.intent = sessions.and_then(|config| intent::recover(config, &registered.path));
    row
}

/// How a worktree is named, which is the shortest name that still finds it from the
/// checkout.
///
/// Three cases, and the rule behind all of them is [`crate::doctor::worktrees`]'s: a
/// tool that makes worktrees may put them under the checkout, beside it, or under a
/// directory of its own somewhere else, and a name that does not say where to look is
/// worth nothing to somebody trying to find the directory.
///
/// Under the checkout is the relative path. Beside it is that path with `../` in front,
/// which is both shorter and still a path a person can type. Anywhere else is the whole
/// path, because nothing shorter would reach it.
fn display_name(root: &Path, path: &Path) -> String {
    if let Ok(inside) = path.strip_prefix(root) {
        return inside.display().to_string();
    }
    if let Some(beside) = root.parent().and_then(|above| path.strip_prefix(above).ok()) {
        return format!("../{}", beside.display());
    }
    path.display().to_string()
}

/// A row with the record's own facts on it and nothing read from the disk yet.
fn blank(name: String, registered: &git::worktree::Registered) -> WorktreeRow {
    WorktreeRow {
        kind: RowKind::Worktree,
        name,
        path: registered.path.clone(),
        branch: registered.branch.clone(),
        intent: None,
        done: Integration::Unknown,
        unpushed: 0,
        uncommitted: 0,
        behind: None,
        bytes: None,
        partial: false,
        made_at: None,
        note: None,
    }
}

/// The lock, and the reason the tool gave for it.
fn locked_note(reason: &str) -> String {
    match reason.trim() {
        "" => String::from("locked"),
        stated => format!("locked ({stated})"),
    }
}

/// Git's word for a worktree whose record points at a location that is not there, and
/// the reason git gave.
fn prunable_note(reason: &str) -> String {
    match reason.trim() {
        "" => String::from(crate::doctor::worktrees::PRUNABLE),
        stated => format!("{} ({stated})", crate::doctor::worktrees::PRUNABLE),
    }
}

/// Fill in what the four Git reads say about one worktree.
///
/// Each read that fails leaves its own column off and says why in a note. None of them
/// ends the row: a person with a broken checkout among eleven good ones is owed the
/// eleven.
fn state(row: &mut WorktreeRow, git: &Git, base: Option<&str>, notes: &mut Vec<String>) {
    match git.remote_containment("HEAD") {
        // A repository with no remote contains nothing anywhere else, so every commit
        // of it is work that exists only here. That is what `remote_containment` means
        // and nothing here softens it.
        Ok(containment) => {
            row.unpushed = u32::try_from(containment.unpushed.len()).unwrap_or(u32::MAX);
        }
        Err(error) => notes.push(format!("{}: {error}", row.name)),
    }
    match git.status() {
        Ok(status) => {
            row.uncommitted = u32::try_from(status.uncommitted().count()).unwrap_or(u32::MAX);
        }
        Err(error) => notes.push(format!("{}: {error}", row.name)),
    }
    let Some(base) = base else {
        // No default branch: nothing to be done against and nothing to be behind. Both
        // columns say so rather than saying zero.
        return;
    };
    // One reading answers both columns. `standing` counts each side of `base...HEAD`
    // and merges the trees, and the merge is skipped for a branch that is not ahead.
    match git.standing(base) {
        Ok(standing) => {
            row.done = standing.integration;
            row.behind = Some(Behind {
                commits: standing.divergence.behind,
                reference: base.to_owned(),
                upstream: base.contains('/'),
            });
        }
        Err(error) => notes.push(format!("{}: {error}", row.name)),
    }
}

/// When a worktree was made, as far as the machine can say.
///
/// `git worktree add` writes a `.git` file in the new directory and nothing rewrites it
/// afterwards, so its modification time is when the worktree was made. This is a
/// reading of the disk and not a record Nodal kept, so a directory whose metadata
/// cannot be read is dated `None` rather than dated now.
fn made_at(path: &Path) -> Option<Timestamp> {
    let modified = std::fs::symlink_metadata(path.join(".git")).ok()?.modified().ok()?;
    let seconds = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Timestamp::from_unix_seconds(i64::try_from(seconds).ok()?).ok()
}

/// The checkout a directory is in, when Git knows one.
///
/// The verdict is about a repository, so a directory that is in none has no verdict
/// rather than an empty one.
///
/// # Errors
/// Nothing. A directory Git does not know is `None`.
#[must_use]
pub fn checkout_at(path: &Path) -> Option<PathBuf> {
    Git::open(path).and_then(|git| git.toplevel()).ok()
}

/// The error a directory that is neither a project nor a checkout gets.
///
/// Stated here so that the one sentence a person sees when Nodal can say nothing at all
/// about where they are stands beside the reading that would otherwise have answered.
#[must_use]
pub fn nowhere(path: &Path) -> Error {
    Error::ProjectNotFound { path: path.to_path_buf() }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::order;
    use crate::git::integration::{Integration, Reason};
    use crate::output::view::verdict::{Behind, RowKind, WorktreeRow};

    fn row(name: &str, done: Integration, unpushed: u32, behind: u32) -> WorktreeRow {
        WorktreeRow {
            kind: RowKind::Worktree,
            name: String::from(name),
            path: PathBuf::from(name),
            branch: None,
            intent: None,
            done,
            unpushed,
            uncommitted: 0,
            behind: Some(Behind {
                commits: behind,
                reference: String::from("main"),
                upstream: false,
            }),
            bytes: Some(0),
            partial: false,
            made_at: None,
            note: None,
        }
    }

    fn names(rows: &[WorktreeRow]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    #[test]
    fn a_worktree_holding_work_nothing_else_has_is_first_even_when_it_is_done() {
        let mut rows = vec![
            row("far-behind", Integration::Open, 0, 90),
            row("holds-work", Integration::Integrated(Reason::Absorbed), 1, 0),
        ];
        order(&mut rows);
        assert_eq!(names(&rows), ["holds-work", "far-behind"]);
    }

    #[test]
    fn done_and_empty_worktrees_sink_to_the_bottom() {
        let done = Integration::Integrated(Reason::Ancestor);
        let mut rows = vec![
            row("finished", done, 0, 0),
            row("open-a-little", Integration::Open, 0, 1),
            row("also-finished", done, 0, 40),
        ];
        order(&mut rows);
        assert_eq!(names(&rows), ["open-a-little", "also-finished", "finished"]);
    }

    #[test]
    fn among_the_unfinished_the_one_the_base_has_moved_furthest_under_is_first() {
        let mut rows = vec![
            row("near", Integration::Open, 0, 2),
            row("far", Integration::Open, 0, 40),
            row("middle", Integration::Open, 0, 9),
        ];
        order(&mut rows);
        assert_eq!(names(&rows), ["far", "middle", "near"]);
    }

    #[test]
    fn two_readings_of_one_checkout_print_one_list() {
        let mut first =
            vec![row("beta", Integration::Open, 0, 3), row("alpha", Integration::Open, 0, 3)];
        let mut second =
            vec![row("alpha", Integration::Open, 0, 3), row("beta", Integration::Open, 0, 3)];
        order(&mut first);
        order(&mut second);
        assert_eq!(names(&first), names(&second));
        assert_eq!(names(&first), ["alpha", "beta"]);
    }
}
