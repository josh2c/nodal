//! The oracle: the same question, answered from Git's own records and from the process
//! table, with no line of Nodal in it.
//!
//! The oracle is the reason the grid is worth running. A grid that asked the predicate
//! twice would agree with itself about everything, FS-14 included. So the contract is read
//! here as a procedure over records Git writes, and the procedure shares no function with
//! the crate it checks.
//!
//! ## What it reads, clause by clause
//!
//! `safety-contract-draft.md` §1 names the loss set, §2 names a proven witness and §3 names
//! a dated observation. Each becomes one reading:
//!
//! | clause | the reading |
//! |---|---|
//! | §1, the commits | `for-each-ref` in the home, less `refs/remotes/` and `refs/nodal/`, plus `HEAD`; `rev-list` over those tips |
//! | §1, the tree | `status --porcelain=v2 --untracked-files=all`; tracked changes and untracked paths no ignore rule covers |
//! | §2.1, reaches | `rev-list --no-walk <oid> --not --exclude=refs/remotes/* --all` in the store prints nothing |
//! | §2.2, complete | `rev-list --objects --missing=print --no-walk <oid>` prints no `?` line |
//! | §2.3, owned | `objects/info/alternates` of the store names no path inside the home |
//! | §2.4, not the home | the store's `--git-common-dir` is not the home's |
//! | §2, partial by rule | `extensions.partialclone` or any `remote.*.promisor` is Unknown |
//! | §3, the observation | a `FETCH_HEAD` line of a store, dated by that file, whose sha reaches the commit |
//! | §3, the local clock | the last reflog entry of the home's own ref, else the ref file's own time |
//!
//! ## Two things the oracle does deliberately
//!
//! **It reads every store under the fixture's root**, not only the ones Nodal looks at.
//! Nodal walks the checkout and its siblings to a depth. The oracle walks the whole
//! temporary machine. So where the two differ, the oracle finds *more* copies, and the
//! disagreement falls in the direction that is recorded and not failed. An oracle that
//! looked in fewer places than the predicate would fail the grid over the predicate being
//! better than it.
//!
//! **It never lets Git fetch.** `GIT_NO_LAZY_FETCH=1` is on every call. A promisor store
//! answers `cat-file -e` about an object it has not got by fetching it, so a reading
//! without that variable would call a blobless clone complete, which is FS-14 read back
//! into the oracle.
//!
//! `refs/nodal/` is left out of the loss set here for the reason
//! `tests/safety/tests/home_refs.rs` states: every ref under it is a record Nodal wrote
//! whose tree is the home's working tree and whose parent is the home's own branch, and
//! both of those are read separately. The `wip` shape of the [`Refs`](crate::shape::Refs)
//! axis builds such a record faithfully, so a `wip` that ever held content the tree does
//! not would appear as a tree the oracle counts and Nodal does not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// What the oracle found. Each field is a reading, and `loses` is the verdict over them.
#[derive(Debug, Clone, Default)]
pub struct Answer {
    /// The commits the home's own refs reach that no store outside it proved a copy of.
    pub unproved: Vec<Unproved>,
    /// The paths the working tree holds that the loss set names and no commit holds.
    pub tree: Vec<String>,
    /// Every store the oracle asked, and what it said about the commits.
    pub asked: Vec<PathBuf>,
    /// What holds the home, when the host let the oracle read that.
    ///
    /// Recorded and never part of [`Answer::loses`]. A process holding a home is a reason to
    /// wait and not a thing that disappears, and the oracle cannot tell the unit's own process
    /// from a stranger's without reading an environment it may not read. So the grid counts what
    /// the oracle saw and the two occupancy shapes are named cases, where the refusal is asserted
    /// directly.
    pub occupancy: Occupancy,
}

/// One commit of the loss set with no proven copy outside the home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unproved {
    /// The commit.
    pub oid: String,
    /// The refs of the home that reach it, so a report can say what the person would lose.
    pub refs: Vec<String>,
}

/// What is holding the home, or why the oracle cannot say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Occupancy {
    /// The processes that hold it, named as `<pid> <how>`.
    Held(Vec<String>),
    /// Nothing holds it, and the reading was made.
    Nothing,
    /// The host would not answer. It is not "nothing holds it", and it is the default because a
    /// reading nobody took must never read as a reading that found nothing.
    #[default]
    Unread,
}

impl Answer {
    /// Whether a member of the contracted loss set would be lost with the directory.
    ///
    /// The commits and the tree, and not the occupancy. A process holding a home is a
    /// reason to wait, not a thing that disappears, so it is no part of this verdict; the
    /// two shapes that made occupancy a safety question are named cases of their own.
    #[must_use]
    pub fn loses(&self) -> bool {
        !self.unproved.is_empty() || !self.tree.is_empty()
    }
}

/// Ask the oracle about one home, over every store under `root`.
///
/// # Panics
///
/// If the home is not a repository the oracle can list the refs of, which is a shape that
/// was not built rather than a reading.
#[must_use]
pub fn ask(home: &Path, root: &Path) -> Answer {
    let mut answer = Answer { occupancy: occupancy(home), ..Answer::default() };
    let stores = stores(root, home);
    let tips = own_refs(home);
    let observations = observations(&stores);
    for (oid, refs) in commits(home, &tips) {
        let clock = newest(&tips, &refs, home);
        if stores.iter().any(|store| proves(store, &oid, home))
            || observations.iter().any(|seen| seen.proves(&oid, clock))
        {
            continue;
        }
        answer.unproved.push(Unproved { oid, refs });
    }
    answer.tree = tree(home);
    answer.asked = stores;
    answer
}

// ---------------------------------------------------------------------------
// §1 — the loss set.
// ---------------------------------------------------------------------------

/// The namespaces of a home that hold no work of its own.
///
/// `refs/remotes/` is a record of somewhere else. `refs/nodal/` is a record Nodal wrote,
/// and the module note says why reading it as work would keep every home for ever.
const NOT_ITS_OWN: [&str; 2] = ["refs/remotes/", "refs/nodal/"];

/// Every ref of the home that holds its own work, with the commit it stands at.
///
/// `HEAD` is added under its own name, so a detached `HEAD` is a tip and a `HEAD` on a
/// branch is the same oid twice, which costs one entry in a map.
fn own_refs(home: &Path) -> BTreeMap<String, String> {
    let listed = plumbing(home, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    let mut tips = BTreeMap::new();
    for line in text(&listed).lines() {
        if let Some((name, oid)) = line.split_once(' ')
            && !NOT_ITS_OWN.iter().any(|kind| name.starts_with(kind))
        {
            tips.insert(name.to_owned(), oid.to_owned());
        }
    }
    let head = plumbing(home, &["rev-parse", "HEAD"]);
    if head.status.success() {
        tips.insert(String::from("HEAD"), text(&head).trim().to_owned());
    }
    tips
}

/// Every commit those tips reach, with the tips that reach each one.
///
/// The walk is the whole history the home's refs hold, and not a delta against anything.
/// A delta would need a baseline, and the baseline is what the question is about.
fn commits(home: &Path, tips: &BTreeMap<String, String>) -> Vec<(String, Vec<String>)> {
    let mut reached: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, oid) in tips {
        let walked = plumbing(home, &["rev-list", oid]);
        for commit in text(&walked).lines() {
            reached.entry(commit.to_owned()).or_default().push(name.clone());
        }
    }
    reached.into_iter().collect()
}

/// The paths of the loss set the working tree holds and no commit does.
///
/// Tracked changes and untracked paths no ignore rule covers, which is §1 exactly. Nodal's
/// own files are left out: the marker and the activation files are what Nodal wrote into
/// the home, and §1 calls them environment.
fn tree(home: &Path) -> Vec<String> {
    let status = plumbing(home, &["status", "--porcelain=v2", "--untracked-files=all"]);
    text(&status).lines().filter_map(entry).filter(|path| !path.starts_with(".nodal/")).collect()
}

/// One porcelain v2 line as a path of the loss set, or nothing.
///
/// `1` and `2` are a changed tracked path, `u` is an unmerged one and `?` is untracked.
/// `!` is ignored and is not in the guarantee. The path is the last field of the line, and
/// a rename line holds two separated by a tab, of which the first is the one that moved.
fn entry(line: &str) -> Option<String> {
    let (kind, rest) = line.split_once(' ')?;
    if !["1", "2", "u", "?"].contains(&kind) {
        return None;
    }
    let path = rest.rsplit(' ').next()?;
    Some(path.split('\t').next().unwrap_or(path).to_owned())
}

// ---------------------------------------------------------------------------
// §2 — a proven witness.
// ---------------------------------------------------------------------------

/// Whether this store is a proven copy of one commit, by all four checks of §2.
fn proves(store: &Path, oid: &str, home: &Path) -> bool {
    if !outside(store, home) || borrows_from(store, home) || partial(store) {
        return false;
    }
    reaches(store, oid) && complete(store, oid)
}

/// §2.4 — the store is a repository of its own and not a worktree of the home's.
///
/// The common directory and not the path. A linked worktree of the home is at a second
/// path, holds every ref the home holds, and keeps its objects in the home: the removal
/// takes both.
fn outside(store: &Path, home: &Path) -> bool {
    let resolved = std::fs::canonicalize(store).unwrap_or_else(|_| store.to_path_buf());
    if resolved.starts_with(std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf())) {
        return false;
    }
    common_dir(store).is_some_and(|theirs| common_dir(home) != Some(theirs))
}

/// Where a repository keeps the objects and the refs every worktree of it shares.
fn common_dir(repo: &Path) -> Option<PathBuf> {
    let asked = plumbing(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    asked.status.success().then(|| PathBuf::from(text(&asked).trim()))?;
    std::fs::canonicalize(text(&asked).trim()).ok()
}

/// §2.3 — the store borrows objects from inside the home being judged.
///
/// Git's own manual documents the hazard: the borrower holds the names and the lender holds
/// the objects, and a removal of the lender leaves the borrower unreadable.
fn borrows_from(store: &Path, home: &Path) -> bool {
    let Some(common) = common_dir(store) else { return false };
    let Ok(listed) = std::fs::read_to_string(common.join("objects/info/alternates")) else {
        return false;
    };
    let inside = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    listed.lines().filter(|line| !line.trim().is_empty()).any(|line| {
        let named = common.join(line.trim());
        std::fs::canonicalize(&named).unwrap_or(named).starts_with(&inside)
    })
}

/// §2, Unknown by rule — the store fills missing objects from a remote on demand.
fn partial(store: &Path) -> bool {
    let asked = plumbing(
        store,
        &["config", "--get-regexp", "^(extensions\\.partialclone|remote\\..*\\.promisor)$"],
    );
    asked.status.success() && !text(&asked).trim().is_empty()
}

/// §2.1 — a ref the store keeps of its own accord reaches the commit.
///
/// Two exclusions, and each one is a ruling this project made after a reading went wrong.
///
/// An **object under no ref** is not a copy (DL-066). It is what `git gc` removes, and one
/// collection in a sibling that changed no work moved three units from safe to refuse. So
/// the question is reachability and never presence.
///
/// A ref under **`refs/remotes/`** is not a ref the store keeps of its own accord. It is that
/// store's record of a fetch or a push, and `git fetch --prune` deletes it the moment the
/// remote drops the branch — exactly as `git gc` deletes an object under no ref. A copy one
/// ordinary command takes away is not what a fourteen-day trash timer may rest on, so
/// `crates/nodal-core/src/git/outside.rs` leaves that namespace out and this reading leaves it
/// out with it. The remote question those refs are about has its own answer here: a dated
/// `FETCH_HEAD` observation, in §3 below.
fn reaches(store: &Path, oid: &str) -> bool {
    if !plumbing(store, &["cat-file", "-e", oid]).status.success() {
        return false;
    }
    let left = plumbing(
        store,
        &["rev-list", "--no-walk", "--ignore-missing", oid, "--not", NOT_TRACKING, "--all"],
    );
    left.status.success() && text(&left).trim().is_empty()
}

/// What takes a store's own record of a remote out of the exclusion.
///
/// `--exclude` applies to the `--all` that follows it.
const NOT_TRACKING: &str = "--exclude=refs/remotes/*";

/// §2.2 — every object the commit names is in the store.
///
/// This is the check FS-14 needed. A blobless clone answers §2.1 about every commit and
/// prints a `?` line for every blob here.
fn complete(store: &Path, oid: &str) -> bool {
    let walked = plumbing(store, &["rev-list", "--objects", "--missing=print", "--no-walk", oid]);
    walked.status.success() && !text(&walked).lines().any(|line| line.starts_with('?'))
}

/// Every repository under `root`, the home and everything inside it left out.
///
/// The walk is bounded to [`DEPTH`] and skips the directories a fixture fills with files
/// that are not repositories, because a walk that read `node_modules` would cost more than
/// the whole of the rest of the reading.
fn stores(root: &Path, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, home, 0, &mut found);
    found.sort();
    found
}

/// How far below the fixture's root the oracle looks for a store.
///
/// The fixture puts the checkout, the state directory and the bare remote one level down,
/// a base or a home two more, and a store a test makes beside the checkout one down. Six is
/// past all of those.
const DEPTH: usize = 6;

/// The names the walk does not enter.
const SKIPPED: [&str; 4] = ["node_modules", "target", ".git", "objects"];

/// Add every repository at or under `directory` to `found`.
fn walk(directory: &Path, home: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > DEPTH {
        return;
    }
    if repository(directory) {
        found.push(directory.to_path_buf());
    }
    let Ok(listed) = std::fs::read_dir(directory) else { return };
    for entry in listed.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if !path.is_dir() || path.is_symlink() || SKIPPED.iter().any(|skip| *skip == name) {
            continue;
        }
        if path == home {
            continue;
        }
        walk(&path, home, depth + 1, found);
    }
}

/// Whether a directory is a repository: one with a working tree, or a bare one.
fn repository(directory: &Path) -> bool {
    directory.join(".git").exists()
        || (directory.join("HEAD").is_file() && directory.join("objects").is_dir())
}

// ---------------------------------------------------------------------------
// §3 — a dated observation of a remote.
// ---------------------------------------------------------------------------

/// One line of one store's `FETCH_HEAD`, with the instant that file carries.
#[derive(Debug, Clone)]
struct Observation {
    /// The store the reading was taken in, so the reachability question can be asked there.
    store: PathBuf,
    /// The sha the remote reported.
    sha: String,
    /// When the file the line is in was last written, in seconds.
    at: u64,
}

impl Observation {
    /// Whether this observation proves a commit, given when the home's own ref last moved.
    ///
    /// Two halves, and both are needed. The sha the remote reported must reach the commit,
    /// and the reading must be *after* the last local change to the work. A reading taken
    /// before the work was pushed says nothing about the work.
    fn proves(&self, oid: &str, clock: Option<u64>) -> bool {
        let Some(moved) = clock else { return false };
        if self.at <= moved {
            return false;
        }
        plumbing(&self.store, &["merge-base", "--is-ancestor", oid, &self.sha]).status.success()
    }
}

/// Every `FETCH_HEAD` line of every store, dated by the file it is in.
///
/// DL-073 rules that `FETCH_HEAD` is the record and the reflog is not: Git writes no reflog
/// entry until a ref first moves, so a fetch that changed nothing leaves a reflog with
/// nothing in it and a `FETCH_HEAD` that lists everything it saw.
fn observations(stores: &[PathBuf]) -> Vec<Observation> {
    let mut seen = Vec::new();
    for store in stores {
        let Some(common) = common_dir(store) else { continue };
        let record = common.join("FETCH_HEAD");
        let Some(at) = written(&record) else { continue };
        let Ok(listed) = std::fs::read_to_string(&record) else { continue };
        for line in listed.lines() {
            if let Some(sha) = line.split_whitespace().next() {
                seen.push(Observation { store: store.clone(), sha: sha.to_owned(), at });
            }
        }
    }
    seen
}

/// When the newest of the home's refs that reach a commit last moved.
///
/// The local clock of §3. `None` where no ref that reaches the commit can be dated, and
/// then no observation proves it: an undated local change cannot be shown to be older than
/// a reading.
fn newest(tips: &BTreeMap<String, String>, reaching: &[String], home: &Path) -> Option<u64> {
    reaching
        .iter()
        .filter(|name| tips.contains_key(*name))
        .filter_map(|name| last_moved(home, name))
        .max()
}

/// When one ref of the home last moved: its reflog if it keeps one, else its own file.
///
/// `packed-refs` is deliberately not read. It is rewritten whenever any ref in it is
/// packed, so it would date this ref by another ref's update, and the error would be in the
/// direction of calling a stale reading fresh. That is FS-3.
fn last_moved(home: &Path, name: &str) -> Option<u64> {
    let logged = plumbing(home, &["log", "-g", "-1", "--format=%ct", name]);
    if logged.status.success()
        && let Ok(seconds) = text(&logged).trim().parse()
    {
        return Some(seconds);
    }
    let common = common_dir(home)?;
    written(&common.join(name))
}

/// When a file was last written, in seconds, and nothing when it is not there.
fn written(path: &Path) -> Option<u64> {
    let modified = std::fs::symlink_metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs())
}

// ---------------------------------------------------------------------------
// Occupancy.
// ---------------------------------------------------------------------------

/// What holds the home, read from the host's own table.
///
/// Recorded and never a loss. `Unread` on a host that publishes no such table, which is
/// what macOS does for the per-descriptor open flags the rule turns on.
#[must_use]
pub fn occupancy(home: &Path) -> Occupancy {
    let resolved = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    #[cfg(target_os = "linux")]
    return proc_table(&resolved);
    #[cfg(not(target_os = "linux"))]
    return lsof(&resolved);
}

/// The `/proc` reading: a working directory, a root, or a descriptor open for writing.
#[cfg(target_os = "linux")]
fn proc_table(home: &Path) -> Occupancy {
    let Ok(listed) = std::fs::read_dir("/proc") else { return Occupancy::Unread };
    let mut held = Vec::new();
    for entry in listed.flatten() {
        let pid = entry.file_name();
        let Some(number) =
            pid.to_str().filter(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        else {
            continue;
        };
        let under = entry.path();
        for link in ["cwd", "root"] {
            if std::fs::read_link(under.join(link)).is_ok_and(|at| at.starts_with(home)) {
                held.push(format!("{number} {link}"));
            }
        }
        held.extend(write_descriptors(&under, home).into_iter().map(|at| format!("{number} {at}")));
    }
    if held.is_empty() { Occupancy::Nothing } else { Occupancy::Held(held) }
}

/// Every descriptor of one process that is open for writing on a file inside the home.
///
/// The flags are in `fdinfo`, and the read-only half is what makes the rule affordable: an
/// editor and a `tail` hold descriptors inside a home all day, and a rule that refused over
/// those would refuse every reclaim on a working machine.
#[cfg(target_os = "linux")]
fn write_descriptors(under: &Path, home: &Path) -> Vec<String> {
    let Ok(listed) = std::fs::read_dir(under.join("fd")) else { return Vec::new() };
    let mut writing = Vec::new();
    for entry in listed.flatten() {
        let Ok(at) = std::fs::read_link(entry.path()) else { continue };
        if !at.starts_with(home) {
            continue;
        }
        let name = entry.file_name();
        let Ok(info) = std::fs::read_to_string(under.join("fdinfo").join(&name)) else { continue };
        if writable(&info) {
            writing.push(format!("fd {}", name.to_string_lossy()));
        }
    }
    writing
}

/// Whether one `fdinfo` says the descriptor was opened for writing.
///
/// The `flags:` line holds the octal of what `open` was called with. The low two bits are
/// the access mode, and `O_WRONLY` and `O_RDWR` are the two that write.
#[cfg(target_os = "linux")]
fn writable(info: &str) -> bool {
    info.lines()
        .filter_map(|line| line.strip_prefix("flags:"))
        .filter_map(|octal| u32::from_str_radix(octal.trim(), 8).ok())
        .any(|flags| flags & 0o3 != 0)
}

/// The macOS reading, which `lsof` answers and which carries no per-descriptor flags.
#[cfg(not(target_os = "linux"))]
fn lsof(home: &Path) -> Occupancy {
    let Ok(asked) = Command::new("lsof").arg("-t").arg("+D").arg(home).output() else {
        return Occupancy::Unread;
    };
    let named: Vec<String> = text(&asked).lines().map(|pid| format!("{pid} lsof")).collect();
    if named.is_empty() { Occupancy::Nothing } else { Occupancy::Held(named) }
}

// ---------------------------------------------------------------------------
// The one `git` call.
// ---------------------------------------------------------------------------

/// One `git` plumbing call, with the machine shut out and no fetch allowed.
///
/// The kit has a `git` runner, and this is not it. Two things are different and both are
/// the point: a plumbing call here is asked *because it may fail*, so nothing asserts the
/// status; and `GIT_NO_LAZY_FETCH` is on, so a promisor store answers about the disk rather
/// than about its remote. Without that variable a blobless clone answers `cat-file -e`
/// about a blob it has not got, and the oracle would agree with the reading FS-14 is.
///
/// # Panics
///
/// If `git` is not on the path at all, which is a machine no shape can be built on.
fn plumbing(repo: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git runs")
}

/// What a command printed on standard output.
fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
