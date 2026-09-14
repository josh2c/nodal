//! What a checkout holds that no commit does, read out of one repository and put into
//! another without either of them gaining a commit.
//!
//! `nodal new --carry` is the only caller. A person asking for it is saying that the
//! half-finished edit in front of them is the work the unit is for, so the unit starts
//! at their `HEAD` with that edit in it, in the same shape they had it: what they had
//! staged is staged, what they had unstaged is unstaged, and what Git had never heard
//! of is there and still untracked. Nodal manufactures no commit, writes no ref and
//! makes no network call to do it.
//!
//! **Why two patches and a copy.** Git's own model of "work" has three parts, and only
//! two of them are in the object database at all. `git diff --cached HEAD` is what a
//! commit would take; `git diff` is what the working tree holds over that; and an
//! untracked file is in neither. Reproducing the first with `git apply --index`, the
//! second with a plain `git apply`, and the third by copying bytes is what keeps the
//! distinction between staged and unstaged rather than flattening it into one dirty
//! tree. A single `git add -A` in the destination would lose it, and a commit would
//! lose it and add a commit nobody wrote.
//!
//! **Why the source is only ever read.** Every invocation here goes through
//! [`cmd`], which sets `GIT_OPTIONAL_LOCKS=0`. That is what stops `git diff` and `git
//! status` from taking `index.lock` to write back the stat cache they refresh, so the
//! person's index file is not opened for writing by anything Nodal runs. Nothing here
//! stages, stashes, checks out or commits in the source; the whole of the read is three
//! `git` processes and the bytes of the untracked files.
//!
//! **Why it is read twice.** The refusals — an unmerged index, a `HEAD` with no branch,
//! a carried set over a ceiling — are made before `nodal new` asks for a base, because
//! asking for a base may build one and a build is minutes. The step that reproduces the
//! work reads the checkout again when it runs. That is deliberate: a patch is content,
//! a plan is written to the journal, and the journal is a table in the registry, so a
//! plan that carried the bytes would be a plan that wrote a person's unpushed edit —
//! and any secret in it — into a database. What the plan holds is a flag.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::cmd;
use super::oid::Oid;
use super::status::{Change, State, Summary};
use crate::error::{Error, Result};

/// How many paths one carry may bring across.
///
/// A ceiling, not a target. Uncommitted work is by definition what one person has in
/// front of them, and a set this size is a build directory no ignore rule covers rather
/// than an edit in progress. The refusal says so and says what to do about it.
pub const MAX_FILES: usize = 5_000;

/// How many bytes one carry may bring across: both patches and every untracked file.
///
/// The same ceiling in the other unit. The patches are held in memory while they are
/// written, so this is also what bounds what a carry costs the process running it.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The flags every diff here is read with.
///
/// They exist because a patch has to be read by `git apply` in another repository, and
/// the person's own configuration can make one that cannot be. `--binary` and
/// `--full-index` carry a binary file and pin its blobs whatever `core.abbrev` says;
/// the two prefixes defeat `diff.noprefix` and `diff.mnemonicPrefix`, either of which
/// would leave a patch that `-p1` cannot strip; `--no-ext-diff` and `--no-textconv`
/// keep a configured filter from replacing the content with its own rendering; and
/// `--ignore-submodules=dirty` keeps the uncommitted state of a submodule's own working
/// tree — which is not a patch at all — out of one, while a submodule's committed
/// position still travels.
const DIFF: &[&str] = &[
    "--binary",
    "--full-index",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--ignore-submodules=dirty",
];

/// How one of the two patches is applied, and how it is recognised as already applied.
///
/// The two differ, and the difference is the whole of what makes this step repeatable.
/// The staged patch is applied to the index *and* the working tree, because the
/// unstaged patch that follows is a difference from the index and needs the working
/// tree to have reached the index's content first. But it is recognised as applied by
/// reversing it against the index alone: by the time the question is asked a second
/// time the working tree has moved on past it, and a reverse check that looked there
/// would say "not applied" about a patch that is.
#[derive(Debug, Clone, Copy)]
struct Application {
    /// What `git apply` is given when the patch is applied.
    apply: &'static [&'static str],
    /// What it is given when the patch is reversed to see whether it is already there.
    check: &'static [&'static str],
}

/// What the checkout had staged: into the index and the working tree, recognised in the
/// index.
const STAGED: Application = Application { apply: &["--index"], check: &["--cached"] };

/// What it held over the index: into the working tree, recognised there.
const UNSTAGED: Application = Application { apply: &[], check: &[] };

/// Where a patch is written while `git apply` reads it: inside the destination's Git
/// directory, so it is on the same filesystem and goes away with the repository.
const PATCH_FILE: &str = "nodal-carry.patch";

/// What a checkout holds that no commit does.
#[derive(Debug, Clone)]
pub struct Work {
    /// The commit `HEAD` is on. Both patches are against this tree, so a destination at
    /// any other commit is not one this applies to.
    pub head: Oid,
    /// `git diff --cached HEAD`, as a patch: what a commit would take.
    pub staged: Vec<u8>,
    /// `git diff`, as a patch: what the working tree holds over the index.
    pub unstaged: Vec<u8>,
    /// Paths Git tracks nothing of and no ignore rule covers, relative to the root.
    pub untracked: Vec<PathBuf>,
    /// The counts and the weight of all of it.
    pub report: Report,
}

/// How much work a carry moved, and of which kind.
///
/// Counts and bytes, and nothing of the content: this is what the journal keeps and
/// what the unit's log says, and neither is a place for a person's unpushed edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Paths whose index differs from `HEAD`.
    pub staged: usize,
    /// Paths whose working tree differs from the index.
    pub unstaged: usize,
    /// Paths Git tracks nothing of and no ignore rule covers.
    pub untracked: usize,
    /// How many distinct paths that is. Smaller than the sum where one path is both
    /// staged and unstaged, which is what a partial `git add -p` leaves.
    pub files: usize,
    /// The two patches and every untracked file, in bytes.
    pub bytes: u64,
}

impl Report {
    /// Whether the checkout had nothing to carry.
    #[must_use]
    pub fn carried_nothing(&self) -> bool {
        self.files == 0
    }

    /// One line for the unit's log.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "carried {} uncommitted {} from the checkout: {} staged, {} unstaged, {} untracked",
            self.files,
            if self.files == 1 { "path" } else { "paths" },
            self.staged,
            self.unstaged,
            self.untracked,
        )
    }
}

/// Read everything `repo` holds that no commit does, and refuse what cannot be carried.
///
/// The refusals are here rather than at the destination so that a `nodal new --carry`
/// that cannot work says so before it builds anything: an unmerged index, a `HEAD` that
/// is not a branch with a commit, a submodule holding work of its own, a path whose kind
/// a copy cannot reproduce, and a set over either ceiling.
///
/// Every one of them is made before the first `git diff` is run, which is what bounds
/// this: the classification is `lstat` and nothing else, so no path is opened by
/// anything here until it is known to be a regular file or a symbolic link.
///
/// # Errors
/// [`Error::CarryUnmerged`] when a path has unresolved merge stages,
/// [`Error::CarryUnanchored`] when `HEAD` is detached or has no commit yet,
/// [`Error::CarrySubmoduleWork`] when a submodule's own working tree holds work,
/// [`Error::CarryUnsupportedKind`] when a carried path is neither a regular file nor a
/// symbolic link, [`Error::CarryTooLarge`] when the set is over a ceiling, and whatever
/// `git` reported.
pub fn read(repo: &Path) -> Result<Work> {
    let git = super::Git::at(repo);
    let summary = git.status_of_submodules_too()?;
    refuse_unmerged(repo)?;
    let Some((_, head)) = git.head_position()? else {
        return Err(Error::CarryUnanchored { repo: repo.to_path_buf() });
    };
    refuse_submodules_holding_work(repo, &summary)?;
    refuse_unsupported_kinds(repo, &summary)?;
    let staged = diff(repo, &["--cached", head.as_str()])?;
    let unstaged = diff(repo, &[])?;
    let untracked = untracked_in(&summary);
    let mut report = count(&summary);
    report.bytes = weigh(repo, &untracked, staged.len() + unstaged.len());
    if report.files > MAX_FILES || report.bytes > MAX_BYTES {
        return Err(Error::CarryTooLarge {
            repo: repo.to_path_buf(),
            files: report.files,
            bytes: report.bytes,
        });
    }
    Ok(Work { head, staged, unstaged, untracked, report })
}

/// Refuse a submodule whose own working tree holds work, because nothing here can carry
/// it.
///
/// A submodule is a second repository, and the superproject records only the commit it
/// stands at. That gitlink is in the superproject's own index and travels like any other
/// change; the modified and untracked files inside the submodule are in objects and a
/// working tree the superproject does not hold, and reach neither patch. The diffs here
/// pass `--ignore-submodules=dirty` precisely so a patch never carries the unusable
/// `-dirty` rendering of that state — which means the state would be left behind in
/// silence unless this says so.
///
/// So it is a refusal rather than a warning. `--carry` says the unit starts with the
/// work the person has, and a unit that quietly started without part of it would be
/// worse than one that was never made. Carrying a submodule's work recursively is a
/// larger thing than this operation, and a person who wants the unit anyway commits or
/// stashes inside the submodule first, which is what the message says.
///
/// The reading this acts on is [`super::Git::status_of_submodules_too`], so a submodule
/// hidden by `submodule.<name>.ignore` or `diff.ignoreSubmodules` is still seen.
fn refuse_submodules_holding_work(repo: &Path, summary: &Summary) -> Result<()> {
    let holding: Vec<PathBuf> = summary
        .uncommitted()
        .filter(|entry| entry.submodule.holds_work())
        .map(|entry| entry.path.clone())
        .collect();
    if holding.is_empty() {
        return Ok(());
    }
    Err(Error::CarrySubmoduleWork { repo: repo.to_path_buf(), paths: holding })
}

/// Refuse a carried path whose kind a copy cannot reproduce, before anything opens it.
///
/// Every path in the carried set is classified by `symlink_metadata` — one `lstat`,
/// which opens nothing and blocks on nothing. That ordering is the property: a named
/// pipe has no end of file and a reader of one waits for a writer that may never come,
/// so the answer has to be reached without reading. `git diff` refuses such a path too
/// (`cannot hash`), but it refuses it *after* this would have, and it says it in Git's
/// words rather than in a reason a person can act on.
///
/// Regular files and symbolic links are what a carry reproduces, a broken link included:
/// a link is recreated from its target's name, which is there whether the target is.
///
/// A directory is on this list and it is the case that actually occurs. Git does not
/// descend into a repository it does not own, so an embedded clone — a `git clone` run
/// inside the project — is reported as one untracked path with a trailing separator.
/// Copying it would make a second copy of somebody's repository inside the unit.
///
/// A submodule is skipped, because a submodule is a directory and its gitlink is what
/// travels rather than its tree. [`refuse_submodules_holding_work`] has already had the
/// only say about those, and it runs first.
fn refuse_unsupported_kinds(repo: &Path, summary: &Summary) -> Result<()> {
    for entry in summary.uncommitted().filter(|entry| !entry.submodule.is_one()) {
        let path = repo.join(&entry.path);
        // A path that is not there is not a kind. A tracked file deleted in the working
        // tree is ordinary work, and the patch that records the deletion is what carries
        // it.
        let Ok(about) = path.symlink_metadata() else { continue };
        if about.is_file() || about.is_symlink() {
            continue;
        }
        return Err(Error::CarryUnsupportedKind {
            repo: repo.to_path_buf(),
            path: entry.path.clone(),
            kind: kind_of(&about),
        });
    }
    Ok(())
}

/// What a path is, in the words a refusal uses.
///
/// Portable: a build for a platform with no such file types answers the general phrase,
/// and the refusal is the same refusal. The specific names are there because "a named
/// pipe" tells a person what to go and look for and "not a file" does not.
fn kind_of(about: &std::fs::Metadata) -> &'static str {
    if about.is_dir() {
        return "a directory";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        let kind = about.file_type();
        if kind.is_fifo() {
            return "a named pipe";
        }
        if kind.is_socket() {
            return "a socket";
        }
        if kind.is_block_device() {
            return "a block device";
        }
        if kind.is_char_device() {
            return "a character device";
        }
    }
    "neither a regular file nor a symbolic link"
}

/// Refuse an index that holds a path at more than one stage.
///
/// `git ls-files --unmerged` rather than the status reading, because the status reading
/// does not answer this on its own: a conflict inside a merge is a `u` record, but an
/// index left unmerged with no operation in progress — which is the state that reaches
/// here, since [`super::Git::ensure_no_operation_in_progress`] has already refused the
/// other one — is reported as an ordinary record with a `UU` code. The plumbing command
/// answers the question that is actually being asked, whichever of the two it is.
fn refuse_unmerged(repo: &Path) -> Result<()> {
    if cmd::run_ok(repo, &["ls-files", "--unmerged"])?.stdout.is_empty() {
        return Ok(());
    }
    Err(Error::CarryUnmerged { repo: repo.to_path_buf() })
}

/// Put `work` into the repository at `home`, which must be a clean checkout of
/// [`Work::head`], and answer what was put there.
///
/// Nothing here writes a ref, makes a commit or reaches a network. The index is written
/// only by `git apply --index`, and only with what the source had staged.
///
/// Repeatable, and the same answer twice: a patch that is already applied is recognised
/// by `git apply --reverse --check` and not applied again, and an untracked file
/// already in place with the same bytes is written with the same bytes.
///
/// # Errors
/// [`Error::CarryCollision`] when an untracked path is already in the home with other
/// content, [`Error::Io`] when a file could not be read or written, and whatever
/// `git apply` reported.
pub fn reproduce(work: &Work, source: &Path, home: &Path, git_dir: &Path) -> Result<Report> {
    refuse_collisions(work, source, home)?;
    let patch = git_dir.join(PATCH_FILE);
    apply(home, &patch, &work.staged, STAGED)?;
    apply(home, &patch, &work.unstaged, UNSTAGED)?;
    for relative in &work.untracked {
        place(&source.join(relative), &home.join(relative))?;
    }
    Ok(work.report.clone())
}

/// One patch, written to a file `git apply` reads and removed again whatever happened.
///
/// The file rather than a pipe because [`cmd`] is the one place a `git` process is
/// started and it does not carry standard input; the destination's own Git directory
/// because that is where the work-in-progress snapshot already puts its temporary index
/// ([`super::snapshot`]), on the same filesystem and removed with the repository.
fn apply(home: &Path, patch: &Path, content: &[u8], how: Application) -> Result<()> {
    if content.is_empty() {
        return Ok(());
    }
    std::fs::write(patch, content).map_err(Error::io(patch))?;
    let applied = run_apply(home, patch, how);
    let removed = std::fs::remove_file(patch).map_err(Error::io(patch));
    applied?;
    removed
}

/// Apply the patch, unless it is already applied.
fn run_apply(home: &Path, patch: &Path, how: Application) -> Result<()> {
    let file = patch.to_string_lossy().into_owned();
    let mut checking = vec!["apply", "--binary", "--reverse", "--check"];
    checking.extend_from_slice(how.check);
    checking.extend_from_slice(&["--", file.as_str()]);
    if cmd::run(home, &checking)?.ok() {
        return Ok(());
    }
    let mut args = vec!["apply", "--binary", "--whitespace=nowarn"];
    args.extend_from_slice(how.apply);
    args.extend_from_slice(&["--", file.as_str()]);
    cmd::run_ok(home, &args)?;
    Ok(())
}

/// Refuse every untracked path the home already holds with other content, before any of
/// them is written.
///
/// The whole set is checked first so that a carry that cannot finish has not half
/// finished. A home holds content of its own — the base's build left files no ignore
/// rule covers, and Nodal wrote its own activation — and overwriting one of those with
/// a file of the same name out of somebody's checkout is a silent loss on whichever
/// side loses. Equal bytes are not a collision: that is the same file, and writing it
/// again is what makes this repeatable.
fn refuse_collisions(work: &Work, source: &Path, home: &Path) -> Result<()> {
    for relative in &work.untracked {
        let destination = home.join(relative);
        if destination.symlink_metadata().is_err() {
            continue;
        }
        if !same_bytes(&source.join(relative), &destination) {
            return Err(Error::CarryCollision { path: relative.clone(), home: home.to_path_buf() });
        }
    }
    Ok(())
}

/// Whether two paths are the same file to carry: both regular files, byte for byte
/// equal, or both links to the same place. Anything else differs, a directory where a
/// file is wanted included.
fn same_bytes(source: &Path, destination: &Path) -> bool {
    let (Ok(here), Ok(there)) = (source.symlink_metadata(), destination.symlink_metadata()) else {
        return false;
    };
    if here.is_symlink() || there.is_symlink() {
        return std::fs::read_link(source).ok() == std::fs::read_link(destination).ok();
    }
    here.is_file()
        && there.is_file()
        && std::fs::read(source).ok() == std::fs::read(destination).ok()
}

/// Copy one untracked path into the home, as what it is.
///
/// A symbolic link is recreated as a link rather than followed: a link into the
/// person's own checkout would otherwise become a file in the unit that reads from
/// their tree. The executable bit travels, because a script that arrives unexecutable
/// is a script that does not run.
fn place(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    let about = source.symlink_metadata().map_err(Error::io(source))?;
    if about.is_symlink() {
        return relink(source, destination);
    }
    std::fs::copy(source, destination).map_err(Error::io(source))?;
    Ok(())
}

/// Recreate a symbolic link at `destination`, pointing where `source` points.
#[cfg(unix)]
fn relink(source: &Path, destination: &Path) -> Result<()> {
    let target = std::fs::read_link(source).map_err(Error::io(source))?;
    match std::os::unix::fs::symlink(&target, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(Error::io(destination)(error)),
    }
}

/// One `git diff`, with the flags that make its answer applicable elsewhere.
fn diff(repo: &Path, extra: &[&str]) -> Result<Vec<u8>> {
    let mut args = vec!["diff"];
    args.extend_from_slice(DIFF);
    args.extend_from_slice(extra);
    args.push("--");
    Ok(cmd::run_ok(repo, &args)?.stdout)
}

/// The untracked paths of a status reading, in the order Git listed them.
fn untracked_in(summary: &Summary) -> Vec<PathBuf> {
    summary
        .entries
        .iter()
        .filter(|entry| entry.state == State::Untracked)
        .map(|entry| entry.path.clone())
        .collect()
}

/// How many paths of each kind a status reading holds.
fn count(summary: &Summary) -> Report {
    let mut report = Report::default();
    for entry in summary.uncommitted() {
        report.files += 1;
        match entry.state {
            State::Untracked => report.untracked += 1,
            State::Tracked { index, worktree } => {
                report.staged += usize::from(index != Change::Unmodified);
                report.unstaged += usize::from(worktree != Change::Unmodified);
            }
            State::Unmerged | State::Ignored => {}
        }
    }
    report
}

/// What the whole carried set weighs: the two patches, and every untracked file.
///
/// A link weighs what its target's name weighs, because that is what travels. A path
/// that has gone between the status reading and this is nothing rather than a failure:
/// the copy is what has to deal with a file that is no longer there.
fn weigh(repo: &Path, untracked: &[PathBuf], patches: usize) -> u64 {
    let mut bytes = u64::try_from(patches).unwrap_or(u64::MAX);
    for relative in untracked {
        let path = repo.join(relative);
        let Ok(about) = path.symlink_metadata() else { continue };
        bytes = bytes.saturating_add(about.len());
    }
    bytes
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::PathBuf;

    use super::{
        Report, count, refuse_submodules_holding_work, refuse_unsupported_kinds, untracked_in,
    };
    use crate::git::status::{Change, Entry, Head, State, Submodule, Summary};

    fn summary(entries: Vec<Entry>) -> Summary {
        Summary {
            head: Head::Branch(String::from("main")),
            upstream: None,
            ahead: 0,
            behind: 0,
            entries,
        }
    }

    fn entry(path: &str, state: State) -> Entry {
        Entry { path: PathBuf::from(path), origin: None, state, submodule: Submodule::No }
    }

    #[test]
    fn a_path_that_is_staged_and_unstaged_is_counted_once_as_a_path() {
        let both = State::Tracked { index: Change::Modified, worktree: Change::Modified };
        let report = count(&summary(vec![
            entry("a.txt", both),
            entry("b.txt", State::Tracked { index: Change::Added, worktree: Change::Unmodified }),
            entry("c.txt", State::Untracked),
            entry("node_modules/x.js", State::Ignored),
        ]));
        assert_eq!(
            report,
            Report { staged: 2, unstaged: 1, untracked: 1, files: 3, bytes: 0 },
            "the ignored path is not work, and a.txt is one path in two states"
        );
    }

    #[test]
    fn only_the_untracked_paths_are_copied_and_ignored_ones_are_not() {
        let listed = untracked_in(&summary(vec![
            entry("notes.txt", State::Untracked),
            entry("target/debug/app", State::Ignored),
            entry(
                "src/lib.rs",
                State::Tracked { index: Change::Unmodified, worktree: Change::Modified },
            ),
        ]));
        assert_eq!(listed, [PathBuf::from("notes.txt")]);
    }

    /// The status reading a classification test acts on: one entry per path.
    fn tracked(path: &str) -> Entry {
        entry(path, State::Tracked { index: Change::Unmodified, worktree: Change::Modified })
    }

    #[test]
    fn a_path_that_is_not_a_file_or_a_link_is_refused_and_named() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("ordinary.txt"), "work\n").unwrap();
        std::fs::create_dir(root.join("embedded")).unwrap();
        std::os::unix::fs::symlink("ordinary.txt", root.join("link.txt")).unwrap();
        std::os::unix::fs::symlink("nowhere.txt", root.join("broken.txt")).unwrap();

        let carried = summary(vec![
            tracked("ordinary.txt"),
            entry("link.txt", State::Untracked),
            entry("broken.txt", State::Untracked),
            entry("gone.txt", State::Tracked { index: Change::Deleted, worktree: Change::Deleted }),
        ]);
        refuse_unsupported_kinds(root, &carried).expect("files and links, one of them broken");

        let refused =
            refuse_unsupported_kinds(root, &summary(vec![entry("embedded", State::Untracked)]))
                .expect_err("an embedded repository arrives as one untracked directory");
        assert!(refused.to_string().contains("embedded"), "{refused}");
        assert!(refused.to_string().contains("a directory"), "{refused}");
    }

    /// A named pipe is the case the classification is bounded for: it is read with
    /// `lstat`, which answers whatever is at the other end of the pipe and whether
    /// anybody is there at all.
    #[test]
    #[cfg(unix)]
    fn a_named_pipe_is_refused_without_anything_opening_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        nodal_safety::tree::fifo(root.join("tail.fifo"));

        // The pipe stands where a tracked file used to be, which is the shape a person
        // actually reaches: `git status` reports it as an ordinary modification and
        // `git diff` would stop on it with `cannot hash`, after this has already
        // answered.
        let refused = refuse_unsupported_kinds(root, &summary(vec![tracked("tail.fifo")]))
            .expect_err("a named pipe is not something a carry reproduces");
        assert!(refused.to_string().contains("tail.fifo"), "{refused}");
        assert!(refused.to_string().contains("a named pipe"), "{refused}");
    }

    #[test]
    fn a_submodule_is_refused_for_its_own_work_and_never_for_being_a_directory() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("vendor")).unwrap();

        // A submodule whose recorded commit moved is a gitlink: it travels in the patch,
        // and the directory it happens to be is nothing the classification may refuse.
        let mut moved = tracked("vendor");
        moved.submodule = Submodule::parse("SC..");
        let carried = summary(vec![moved]);
        refuse_submodules_holding_work(root, &carried).expect("a moved commit is carryable");
        refuse_unsupported_kinds(root, &carried).expect("and its directory is skipped");

        let mut dirty = tracked("vendor");
        dirty.submodule = Submodule::parse("S.MU");
        let refused = refuse_submodules_holding_work(root, &summary(vec![dirty]))
            .expect_err("work inside a submodule reaches neither patch");
        assert!(refused.to_string().contains("vendor"), "{refused}");
        assert!(refused.to_string().contains("cannot reach"), "{refused}");
    }

    #[test]
    fn a_carry_of_nothing_says_so_and_one_path_reads_as_one_path() {
        assert!(Report::default().carried_nothing());
        let one = Report { staged: 1, unstaged: 0, untracked: 0, files: 1, bytes: 12 };
        assert!(!one.carried_nothing());
        assert!(one.describe().contains("1 uncommitted path"), "{}", one.describe());
    }
}
