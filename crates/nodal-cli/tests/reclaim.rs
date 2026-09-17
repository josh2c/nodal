//! Acceptance test for `nodal reclaim` and `nodal gc`, end to end, against a
//! real repository.
//!
//! Every assertion here is about something a person can check for themselves after the
//! command has run: the home is not where it was, the trash directory holds it, the
//! ports are free, `git status` in a refused unit still shows the work that was there,
//! and the file a hook appended to says which hooks ran and in what order.
//!
//! Three of them are the ones the operation exists for.
//!
//! A unit with uncommitted work, an untracked file or a commit no other tree has is
//! **refused, and told why** — the message names the paths, not a policy. `--force`
//! goes on, and the work is in a snapshot ref inside the trashed home rather than gone.
//!
//! A process planted in the unit's home is **stopped, and its port comes back**. The
//! plant is a real process, started detached so that it is reaped by the system rather
//! than left as a child of the test, because a child this test has not waited for still
//! answers "yes" to "does this process exist" and would make a passing stop look like a
//! failure.
//!
//! A reclaim killed with `SIGKILL` between two steps **resolves on the next
//! invocation**. The kill lands inside the step that stops the runtime, which is held
//! open by a plant that ignores `SIGTERM`; the next `nodal` rolls the run back, and the
//! unit is still there to be reclaimed properly.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::lifecycle::journal;
use nodal_core::model::{EnvState, UnitStatus};
use nodal_core::store::{environments, projects, sessions, trash, units};
use nodal_safety::git::git_text as git;
use nodal_safety::process::{self, Owned, alive, until, wait_for};
use nodal_safety::project::Layout;
use nodal_safety::project::resolved;
use nodal_safety::text::{answer, stderr, stdout};
use nodal_safety::{InState as _, Workspace};

/// How long a test waits for a killed run to reach the step it is being killed in.
const REACH_TIMEOUT: Duration = Duration::from_secs(60);

/// How often it looks.
const POLL: Duration = Duration::from_millis(5);

/// This suite's project, which ignores its own build output as well.
///
/// `built/` is a row of the project's ignore file rather than of Nodal's default table,
/// so a file under it is the person's own decision to leave out of Git and the refusal
/// tests are about what a reclaim does with one.
fn workspace() -> Workspace {
    Workspace::laid_out(
        state::BINARY,
        &Layout { ignore: "node_modules/\nbuilt/\n", ..Layout::default() },
    )
}

/// The same, with a recipe this test wrote and this machine has approved.
///
/// Approving is what `nodal init` does, so the fixture runs it rather than writing the
/// approvals file: an approval a test made by hand would not be evidence that the
/// command a person runs makes one.
fn workspace_with_recipe(recipe: &str) -> Workspace {
    let workspace = workspace();
    workspace.approve_recipe(recipe);
    workspace
}

/// The readings this suite needs beyond the ones the shared fixture has.
trait Reclaiming {
    /// The one unit this project has: its identifier, and its home.
    fn one_unit_and_home(&self) -> (String, PathBuf);

    /// Make a checkout of this project a unit adopted in place, with `nodal adopt`.
    ///
    /// The directory is a clone of the project, because that is what an adopted
    /// checkout is: a tree whose commits the project already has. The command is the
    /// real one rather than rows written by hand, so what this suite reclaims is what
    /// an adoption actually leaves on a disk — the marker, the activation files, and
    /// the exclusions that keep them out of `git status`.
    ///
    /// The command is given the name the caller has and the answer is the name the
    /// filesystem uses. Those are two different strings whenever the temporary
    /// directory is reached through a link — always on macOS, and on Linux under this
    /// suite's second run — and it is the resolved one the registry holds and every
    /// report prints, so it is the one a caller can compare anything against.
    fn adopt_in_place(&self, slug: &str) -> PathBuf;

    /// Add a linked worktree of the project on a new branch and adopt it in place.
    fn adopt_worktree(&self, slug: &str) -> PathBuf;
}

impl Reclaiming for Workspace {
    fn one_unit_and_home(&self) -> (String, PathBuf) {
        (self.one_unit().id.to_string(), self.homes().pop().expect("it has a home"))
    }

    fn adopt_in_place(&self, slug: &str) -> PathBuf {
        let root = self.state.parent().expect("the state directory has a parent").join(slug);
        drop(git(&self.source, &["clone", "-q", "--", ".", root.to_str().expect("a path")]));
        drop(stdout(&self.nodal(&[
            "adopt",
            root.to_str().expect("a path"),
            "--in-place",
            "--name",
            slug,
        ])));
        resolved(&root)
    }

    /// Add a linked worktree of the project on a new branch and adopt it in place.
    fn adopt_worktree(&self, slug: &str) -> PathBuf {
        let path = self.state.parent().expect("the state directory has a parent").join(slug);
        drop(git(
            &self.source,
            &["worktree", "add", "-q", "-b", &format!("feature/{slug}"), path.to_str().unwrap()],
        ));
        drop(stdout(&self.nodal(&["adopt", path.to_str().unwrap(), "--in-place", "--name", slug])));
        resolved(&path)
    }
}

/// The JSON a `--json` command answered with.
fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).unwrap()
}

/// Insist that the verification found nothing, and that it said so honestly.
///
/// There are two honest lines and the host decides which. A machine that read every
/// signal says "nothing left by id". A machine that could not read one — no `/proc`, no
/// Docker daemon — says "nothing found by id" and puts a note underneath. What must
/// never appear is the first line on a host that could only manage the second.
fn assert_nothing_left(report: &str) {
    let claimed = report.contains("nothing left by id");
    let hedged = report.contains("nothing found by id; a signal could not be read");
    assert!(claimed || hedged, "the verification said neither of the two honest things: {report}");
    assert!(!(claimed && hedged), "{report}");
}

/// Insist that the reclaim read the process table: a note about the process signal may
/// say what the host refused to show, and never that the table went unread.
fn assert_process_signal(report: &serde_json::Value) {
    let notes = report["notes"].as_array().expect("a report carries its notes");
    assert!(
        !notes.iter().any(|note| note["signal"] == "environment" && note["reach"] == "unread"),
        "the process table was read: {report}"
    );
}

/// Reclaim a clean unit by name.
fn reclaim(workspace: &Workspace, slug: &str) -> Output {
    workspace.nodal(&["reclaim", slug])
}

/// A `sleep` of `seconds` whose variables the process table shows, as a shell line.
fn readable_sleep(seconds: u32) -> String {
    format!("'{}' {seconds}", process::readable_sleep().display())
}

// ---------------------------------------------------------------------------
// The check, the trash and the verification.
// ---------------------------------------------------------------------------

#[test]
fn a_clean_unit_is_reclaimed_and_nothing_of_it_is_left_but_the_trash_entry() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();

    let report = stdout(&reclaim(&workspace, "worker-import"));
    assert!(report.contains("nothing that is only here"), "{report}");
    assert_nothing_left(&report);

    assert!(!home.exists(), "the home is not where it was");
    assert!(workspace.homes().is_empty(), "and no live home is left: {:?}", workspace.homes());
    let trashed = workspace.trashed();
    assert_eq!(trashed.len(), 1, "the trash holds it, and only it: {trashed:?}");
    assert_eq!(trashed[0].file_name(), home.file_name(), "under the name it had");
    assert!(trashed[0].join("app").join("main.txt").is_file(), "with its content");

    let store = workspace.store();
    let entry = trash::list(store.conn()).unwrap().pop().expect("the move was recorded");
    assert_eq!(entry.path, trashed[0]);
    assert_eq!(entry.home, home);
    assert!(entry.snapshot.is_none(), "a clean home needed nothing preserved");
    assert!(
        !report.contains("git worktree remove"),
        "a home Nodal made is not a worktree to remove: {report}"
    );
}

#[test]
fn a_unit_with_work_that_is_only_there_is_refused_and_told_exactly_what() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "edited\n").unwrap();
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();
    std::fs::create_dir_all(home.join("built")).unwrap();
    std::fs::write(home.join("built").join("out.js"), "ignored\n").unwrap();

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success(), "a dirty unit is not reclaimed");
    let told = stderr(&refused);
    assert!(told.contains("uncommitted changes (1): app/main.txt"), "{told}");
    assert!(told.contains("untracked files (1): app/new.txt"), "{told}");
    assert!(!told.contains("built/out.js"), "an ignored path is not work: {told}");

    assert!(home.is_dir(), "the home is where it was");
    assert_eq!(git(&home, &["status", "--porcelain"]).lines().count(), 2, "and so is the work");
    assert!(workspace.trashed().is_empty(), "nothing was moved");
}

#[test]
fn a_commit_no_other_tree_has_refuses_a_reclaim_and_a_shared_one_does_not() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "committed here\n").unwrap();
    // A home is a clone and carries no identity of its own, as a person's would from
    // their global configuration.
    drop(git(&home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(&home, &["config", "user.name", "Test"]));
    drop(git(&home, &["add", "-A"]));
    drop(git(&home, &["commit", "-qm", "work only this home has"]));

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success());
    let told = stderr(&refused);
    assert!(told.contains("commits on no remote (1)"), "{told}");

    // The commits the home inherited are in the person's own checkout, so they are not
    // work that is only here. Without that, a project with no remote could never have a
    // unit reclaimed at all.
    //
    // The fetch names a branch. A commit under no ref of the checkout is an object the
    // next `git gc` there removes, and the reading counts a second copy and never a
    // second object (`nodal_core::git::outside`).
    drop(git(&workspace.source, &["fetch", "-q", home.to_str().unwrap(), "HEAD:refs/heads/kept"]));
    let accepted = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(accepted.status.success(), "{}", stderr(&accepted));
}

/// A second clone beside the checkout, which is the layout this reading was missing.
///
/// A clone under `siblings/` held a unit's commit and `--check` called that commit "only
/// here", because the reading asked the project's checkout and nothing else while the
/// help promised the machine. The mislabel was conservative and could not cause a
/// false-safe, and it was still a wrong answer to the question a person asks before they
/// clear a machine.
///
/// The commit is proved in the sibling's own object store. A repository that only names
/// it counts for nothing, which is the second half of this test.
#[test]
fn a_commit_a_sibling_clone_on_this_machine_holds_is_not_only_here() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "the duplicated commit\n").unwrap();
    drop(git(&home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(&home, &["config", "user.name", "Test"]));
    drop(git(&home, &["add", "-A"]));
    drop(git(&home, &["commit", "-qm", "work this home and one sibling have"]));
    let commit = git(&home, &["rev-parse", "HEAD"]).trim().to_owned();

    // Nothing beside the checkout holds it yet, so it is the only copy and the refusal
    // says so. This is the reading Nodal made before it asked the siblings.
    let refused = workspace.nodal(&["reclaim", "worker-import", "--check"]);
    assert!(!refused.status.success(), "{}", answer(&refused));
    let before = answer(&refused);
    assert!(before.contains("only"), "the commit is the only copy: {before}");

    // A second clone beside the checkout, one directory down. It fetches the commit, so
    // its own object store holds it.
    let siblings = workspace.root().join("siblings");
    std::fs::create_dir_all(&siblings).unwrap();
    let mirror = siblings.join("mirror");
    drop(git(
        workspace.root(),
        &["clone", "-q", workspace.source.to_str().unwrap(), mirror.to_str().unwrap()],
    ));
    drop(git(
        &mirror,
        &["fetch", "-q", home.to_str().unwrap(), &format!("{commit}:refs/heads/partial-copy")],
    ));

    let answered = workspace.nodal(&["reclaim", "worker-import", "--check"]);
    let after = answer(&answered);
    assert!(answered.status.success(), "the verdict is the exit code: {after}");
    assert!(after.contains("second local copy"), "the commit has a second copy: {after}");

    // The report names the repository that holds it, so a person can go and look, and no
    // group calls the commit the only copy any more.
    let report: serde_json::Value = serde_json::from_str(&answer(&workspace.nodal(&[
        "reclaim",
        "worker-import",
        "--check",
        "--json",
    ])))
    .unwrap();
    let groups = report["commits"].as_array().expect("the report carries commit groups");
    assert!(
        !groups.iter().any(|group| group["copies"]["kind"] == "only_here"),
        "no group calls it the only copy: {report}"
    );
    let held_by: Vec<&str> = report["commits"]
        .as_array()
        .expect("the report carries commit groups")
        .iter()
        .filter_map(|group| group["copies"]["held_by"].as_str())
        .collect();
    assert!(
        held_by.iter().any(|path| resolved(Path::new(path)) == resolved(&mirror)),
        "the sibling that holds it is named: {held_by:?}"
    );
}

/// A clone of the project on a branch of its own, adopted in place.
///
/// A branch each, because an open unit holds its branch and a second unit on `main` is
/// refused. A clone and not a linked worktree, because the point of the test is two
/// object stores that hold each other's only copy.
fn adopt_on_its_own_branch(workspace: &Workspace, slug: &str) -> PathBuf {
    let root = workspace.state.parent().expect("the state directory has a parent").join(slug);
    drop(git(&workspace.source, &["clone", "-q", "--", ".", root.to_str().expect("a path")]));
    drop(git(&root, &["switch", "-q", "-c", &format!("work/{slug}")]));
    drop(stdout(&workspace.nodal(&[
        "adopt",
        root.to_str().expect("a path"),
        "--in-place",
        "--name",
        slug,
    ])));
    resolved(&root)
}

/// One commit in a home, made by that home and nowhere else yet.
fn commit_only_here(home: &Path, text: &str) -> String {
    std::fs::write(home.join("app").join("main.txt"), text).unwrap();
    drop(git(home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(home, &["config", "user.name", "Test"]));
    drop(git(home, &["add", "-A"]));
    drop(git(home, &["commit", "-qm", "work only this home has"]));
    git(home, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// Put a second copy of `commit` in `holder`, under a ref of its own, so that its own
/// object store really reaches it.
fn fetch_into(holder: &Path, source: &Path, commit: &str) {
    drop(git(
        holder,
        &["fetch", "-q", source.to_str().unwrap(), &format!("{commit}:refs/heads/copy-of")],
    ));
}

/// Per-unit safety is not joint safety.
///
/// Two units each hold the only second copy of the other's work. Each is safe on its own,
/// truthfully, and reclaiming both loses both. A reading of five unit homes on one
/// machine found exactly that, and nothing answered the question; `--check` over more
/// than one unit is what answers it now.
///
/// The per-unit verdict stays exactly what it was, because it is still true: a person
/// reclaiming one of them is not about to lose anything.
#[test]
fn two_units_that_hold_each_others_only_copy_are_refused_together_and_allowed_apart() {
    let workspace = workspace();
    let alpha = adopt_on_its_own_branch(&workspace, "alpha");
    let beta = adopt_on_its_own_branch(&workspace, "beta");

    // One commit in each home, and each fetched into the other, so the only second copy
    // of either is inside the other unit.
    let mine = commit_only_here(&alpha, "alpha's morning\n");
    let theirs = commit_only_here(&beta, "beta's morning\n");
    fetch_into(&beta, &alpha, &mine);
    fetch_into(&alpha, &beta, &theirs);

    // Each on its own: safe, and the report names the other home as the store that holds
    // the copy. Naming the store is the whole of what the joint rule then discounts, so
    // a reading that named this home instead would refuse nothing and discount nothing.
    assert_safe_alone(&workspace, "alpha", &beta);
    assert_safe_alone(&workspace, "beta", &alpha);

    // Both together: refused, and the reason names the home that goes with them.
    let both = workspace.nodal(&["reclaim", "alpha", "beta", "--check"]);
    let report = answer(&both);
    assert!(!both.status.success(), "the pair is not safe: {report}");
    assert!(report.contains("refuse — a reclaim of all 2 would stop"), "{report}");
    assert!(report.contains("which the same removal takes"), "{report}");
    assert!(
        report.contains("safe — a reclaim would go ahead"),
        "the per-unit verdict stays: {report}"
    );

    let document: serde_json::Value = serde_json::from_str(&answer(
        &workspace.nodal(&["reclaim", "alpha", "beta", "--check", "--json"]),
    ))
    .unwrap();
    assert_eq!(document["safe_together"], serde_json::json!(false), "{document}");
    let units = document["units"].as_array().expect("one entry per unit");
    assert_eq!(units.len(), 2, "{document}");
    for unit in units {
        assert_eq!(unit["safe_to_reclaim"], serde_json::json!(true), "{document}");
        assert_eq!(unit["together"]["safe"], serde_json::json!(false), "{document}");
    }
}

/// One unit, read on its own, is safe over a copy `holder` holds.
///
/// `holder` is asserted and not ignored. A home adopted in place sits beside the
/// project's checkout, which is exactly where the reading looks for other repositories,
/// so the home is offered its own path as a store that holds a second copy — and it does
/// hold every one of those commits, because they are its own. A reading that believed it
/// would print "the second copy is in this very directory" and the joint rule would then
/// have nothing to discount.
fn assert_safe_alone(workspace: &Workspace, slug: &str, holder: &Path) {
    let checked = workspace.nodal(&["reclaim", slug, "--check"]);
    let report = answer(&checked);
    assert!(checked.status.success(), "{slug} is safe on its own: {report}");
    assert!(report.contains("second local copy"), "{slug}: {report}");
    assert!(
        report.contains(holder.to_str().expect("a path")),
        "{slug}: the copy is held by {}, not by the home being read: {report}",
        holder.display()
    );
}

/// A reclaim that is not a check does one unit at a time, and says so rather than
/// guessing which of a list was meant.
#[test]
fn a_reclaim_of_more_than_one_unit_is_refused_and_names_the_check_that_reads_them() {
    let workspace = workspace();
    drop(adopt_on_its_own_branch(&workspace, "alpha"));
    drop(adopt_on_its_own_branch(&workspace, "beta"));

    let refused = workspace.nodal(&["reclaim", "alpha", "beta"]);
    assert!(!refused.status.success());
    let told = stderr(&refused);
    assert!(told.contains("one unit at a time"), "{told}");
    assert!(told.contains("--check"), "{told}");
}

/// A force-push that rewrote history with a byte-identical tree.
///
/// The remote's new tip and the home's commit are two
/// identifiers over one tree object, so not one byte of the work is at risk; Nodal
/// compares commit identity, so it read the commit as only here and refused, and nothing
/// said the refusal was about a name rather than about the content.
///
/// Both halves are asserted. The report names the ref and calls the row reconstructable,
/// and the verdict does not move: taking the tree from that ref rebuilds the content and
/// not the commit, its message, its author or its parents.
#[test]
fn a_commit_a_remote_tip_holds_the_tree_of_is_named_and_still_refused() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "work a force-push rewrote\n").unwrap();
    drop(git(&home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(&home, &["config", "user.name", "Test"]));
    drop(git(&home, &["add", "-A"]));
    drop(git(&home, &["commit", "-qm", "the work, under the id this home wrote"]));
    let commit = git(&home, &["rev-parse", "HEAD"]).trim().to_owned();
    let tree = git(&home, &["rev-parse", "HEAD^{tree}"]).trim().to_owned();

    // The same tree under another identifier, which is what a rewrite leaves behind, and
    // the home's own record of what the remote now holds.
    let rewritten =
        git(&home, &["commit-tree", &tree, "-m", "the same work, rewritten"]).trim().to_owned();
    let reference = "refs/nodal/origin/nodal/worker-import";
    drop(git(&home, &["update-ref", reference, &rewritten]));

    let checked = workspace.nodal(&["reclaim", "worker-import", "--check"]);
    let report = answer(&checked);
    assert!(!checked.status.success(), "the content is not the commit: {report}");
    assert!(report.contains("same content as"), "{report}");
    assert!(report.contains(reference), "the ref is named: {report}");
    assert!(report.contains(&rewritten[..8]), "the tip is named: {report}");
    assert!(report.contains("a reclaim keeps this home"), "the refusal stands: {report}");

    let document: serde_json::Value = serde_json::from_str(&answer(&workspace.nodal(&[
        "reclaim",
        "worker-import",
        "--check",
        "--json",
    ])))
    .unwrap();
    assert_eq!(document["safe_to_reclaim"], serde_json::json!(false), "{document}");
    let rows = document["content"].as_array().expect("the report carries the content rows");
    let row = rows.iter().find(|row| row["commit"] == serde_json::json!(commit)).expect("the row");
    assert_eq!(row["reference"], serde_json::json!(reference), "{document}");
    assert_eq!(row["tip"], serde_json::json!(rewritten), "{document}");
    assert_eq!(row["tree"], serde_json::json!(tree), "{document}");
    assert_eq!(row["disposition"], serde_json::json!("reconstructable"), "{document}");

    // It is never read as a second copy: the commit is still in the group a reclaim
    // refuses over, and the refusal names it.
    let groups = document["commits"].as_array().expect("the report carries commit groups");
    assert!(
        groups.iter().any(|group| group["copies"]["kind"] == "only_here"
            || group["copies"]["kind"] == "not_checked"),
        "the commit is still refused over: {document}"
    );
}

/// A sibling that names a commit without holding it proves nothing.
///
/// This is the invariant the reading rests on: a refusal is weakened by an object in a
/// second store, proved by `git rev-list` in that store, and never by a name. A clone
/// that has been `reflog expire`d and garbage collected keeps names over an empty store,
/// and that is the case that must not weaken anything.
#[test]
fn a_sibling_that_names_a_commit_without_holding_it_does_not_weaken_the_refusal() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "the unique commit\n").unwrap();
    drop(git(&home, &["config", "user.email", "unit@example.invalid"]));
    drop(git(&home, &["config", "user.name", "Test"]));
    drop(git(&home, &["add", "-A"]));
    drop(git(&home, &["commit", "-qm", "work only this home has"]));
    let commit = git(&home, &["rev-parse", "HEAD"]).trim().to_owned();

    // A clone beside the checkout that carries the name and not the object.
    let siblings = workspace.root().join("siblings");
    std::fs::create_dir_all(&siblings).unwrap();
    let mirror = siblings.join("mirror");
    drop(git(
        workspace.root(),
        &["clone", "-q", workspace.source.to_str().unwrap(), mirror.to_str().unwrap()],
    ));
    // The ref is written as a file rather than through `update-ref`, because Git refuses
    // to name an object it does not have. This is the state `reflog expire` and
    // `gc --prune=now` leave: the objects are taken and the names stay.
    let named = mirror.join(".git").join("refs").join("heads").join("partial-copy");
    std::fs::create_dir_all(named.parent().unwrap()).unwrap();
    std::fs::write(&named, format!("{commit}\n")).unwrap();

    let refused = workspace.nodal(&["reclaim", "worker-import", "--check"]);
    assert!(!refused.status.success(), "a name is not a second copy: {}", answer(&refused));
    let told = answer(&refused);
    assert!(
        !told.contains(mirror.to_str().unwrap()),
        "and the repository that only names it is not offered as a copy: {told}"
    );
}

#[test]
fn a_forced_reclaim_commits_the_work_before_it_moves_the_home() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (id, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("app").join("main.txt"), "edited\n").unwrap();
    std::fs::write(home.join("only-here.txt"), "never committed\n").unwrap();

    let report = stdout(&workspace.nodal(&["reclaim", "worker-import", "--force"]));
    assert!(report.contains("forced past uncommitted changes"), "{report}");
    let reference = format!("refs/nodal/{id}/wip");
    assert!(report.contains(&reference), "the report says where the work went: {report}");

    let trashed = workspace.trashed().pop().expect("the home is in the trash");
    let listed = git(&trashed, &["ls-tree", "-r", "--name-only", &reference]);
    assert!(listed.contains("only-here.txt"), "the untracked file is in the snapshot: {listed}");
    let content = git(&trashed, &["show", &format!("{reference}:app/main.txt")]);
    assert_eq!(content, "edited\n", "and so is the edit");

    let store = workspace.store();
    let entry = trash::list(store.conn()).unwrap().pop().unwrap();
    assert_eq!(entry.snapshot.as_deref(), Some(reference.as_str()));
}

/// A caller standing in the home it is reclaiming is never a target of its own stop.
///
/// The command stands in the home it is about, which is where a person runs it from,
/// and it carries no `NODAL_ID` because nothing activated the directory for it. That is
/// the probable level, so it is not signalled and it is not a leftover either: the two
/// processes a stop spares are left out of the probable list at the scan.
///
/// The home still goes. A caller is not the bystander the move refuses over, because a
/// person whose own command is what stands in the home can see the directory move.
#[test]
fn reclaiming_from_inside_the_home_does_not_stop_the_shell_that_asked() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();

    let reclaimed = workspace
        .command(&["reclaim", "worker-import", "--json"])
        .current_dir(&home)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout(&reclaimed)).unwrap();

    assert_process_signal(&report);
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    assert!(report["stopped"]["killed"].as_array().unwrap().is_empty(), "{report}");
    assert!(
        report["stopped"]["asked"].as_array().unwrap().is_empty(),
        "the caller became a target of its own stop: {report}"
    );
    assert!(!home.exists(), "and the home still went");
}

/// A caller in an *activated* home carries the unit's identifier, which is the certain
/// level, so it does become a target — and the stop spares it.
///
/// This is the half of the rule the scan cannot make: a process that says it belongs to
/// the unit is signalled, and the one exception is the process that asked. Killing the
/// shell somebody typed the command into, in the middle of the command, is not a thing
/// to do.
#[test]
fn a_caller_carrying_the_units_identifier_is_a_target_and_is_spared() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (id, home) = workspace.one_unit_and_home();

    let reclaimed = workspace
        .command(&["reclaim", "worker-import", "--json"])
        .current_dir(&home)
        .env("NODAL_ID", &id)
        .output()
        .unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout(&reclaimed)).unwrap();

    let spared = report["stopped"]["spared"].as_array().unwrap().len();
    assert_eq!(spared, 1, "the command's own process was left alone: {report}");
    assert!(report["stopped"]["killed"].as_array().unwrap().is_empty(), "{report}");
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    assert!(!home.exists(), "and the home still went");
}

#[test]
fn a_reclaimed_unit_is_listed_as_archived_with_no_home_and_no_complaint() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    drop(stdout(&reclaim(&workspace, "worker-import")));

    // The unit stays on the list, because the list is the ledger. What must not stay is
    // its home: asking Git about a directory a reclaim moved away on purpose would put
    // a note under every list from now on.
    let listed = stdout(&workspace.nodal(&["ls"]));
    assert!(listed.contains("worker-import"), "{listed}");
    assert!(listed.contains("archived"), "{listed}");
    assert!(!listed.contains("No such file or directory"), "{listed}");
    assert!(!listed.contains("git status"), "the list has nothing to complain about: {listed}");
}

/// A reclaimed unit used to go on holding its name, so making the unit again gave
/// `<name>-2` on the branch the archived unit already had. Two units then shared
/// `nodal/<name>`.
///
/// A handle is unique among the units that hold one, and an archived unit holds none, so
/// the name is free the moment the unit is archived. Nothing of the archived row is
/// rewritten to free it: it keeps the name a person typed, its identifier and its branch.
#[test]
fn a_reclaimed_units_name_is_free_again_and_the_archived_row_keeps_its_own_identity() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let archived = slug_id(&workspace, "worker-import");
    drop(stdout(&reclaim(&workspace, "worker-import")));

    // Until somebody takes the name, it still reaches the unit that had it, so a person
    // who reclaims twice is told what happened rather than that there is no such unit.
    let again = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("was reclaimed already"), "{}", stderr(&again));

    // A caller reads the handle back out of this document, and this is the document the
    // suffix showed up in: a second `new --name X` after `reclaim X` answered
    // `worker-import-2`, on the branch the archived unit already held.
    let created = json(&workspace.nodal(&["new", "--name", "worker-import", "--json"]));
    assert_eq!(created["unit"]["slug"], "worker-import", "the create's own answer: {created}");
    assert_eq!(created["unit"]["branch"], "nodal/worker-import", "{created}");

    let listed = json(&workspace.nodal(&["ls", "--json"]));
    let units = listed["units"].as_array().expect("the list has units");
    let made = units
        .iter()
        .find(|row| row["slug"] == "worker-import" && row["status"] != "archived")
        .expect("the name was free, so the new unit has it");
    assert_eq!(made["branch"], "nodal/worker-import", "and the branch is the one the name makes");
    assert_ne!(made["id"], serde_json::json!(archived), "it is a new unit, not the archived one");
    assert!(
        !units.iter().any(|row| row["slug"] == "worker-import-2"),
        "no unit was pushed onto a suffix: {listed}"
    );

    // The archived row is still there, with the name a person typed, its own identifier
    // and the branch it always had.
    let kept = units
        .iter()
        .find(|row| row["id"] == serde_json::json!(archived))
        .expect("the archived unit is still on the list");
    assert_eq!(kept["status"], "archived", "{kept}");
    assert_eq!(kept["slug"], "worker-import", "nothing was renamed to free the name: {kept}");
    assert_eq!(kept["branch"], "nodal/worker-import", "the archived unit keeps its own branch");
}

/// A name made, reclaimed, made again and reclaimed again leaves two archived units
/// under it. The name means the last unit that held it.
#[test]
fn a_name_reclaimed_twice_reaches_the_unit_that_held_it_last() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let first = slug_id(&workspace, "worker-import");
    drop(stdout(&reclaim(&workspace, "worker-import")));
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let second = slug_id(&workspace, "worker-import");
    assert_ne!(first, second, "the second unit is a new one");
    drop(stdout(&reclaim(&workspace, "worker-import")));

    // Two archived units carry the name and the branch, and neither is wrong.
    let listed = json(&workspace.nodal(&["ls", "--json"]));
    let units = listed["units"].as_array().expect("the list has units");
    let both: Vec<&serde_json::Value> = units
        .iter()
        .filter(|row| row["slug"] == "worker-import" && row["status"] == "archived")
        .collect();
    assert_eq!(both.len(), 2, "two units held the name and gave it back: {listed}");
    assert!(
        both.iter().all(|row| row["branch"] == "nodal/worker-import"),
        "each keeps the branch its name made: {listed}"
    );

    // The name reaches the one that held it last, and the answer is the same every time
    // it is asked.
    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("was reclaimed already"), "{}", stderr(&refused));
    let shown = json(&workspace.nodal(&["show", "worker-import", "--json"]));
    assert_eq!(shown["unit"]["id"], serde_json::json!(second), "the newest of the two: {shown}");
}

/// The identifier of the unit that holds this handle now.
///
/// A handle an archived unit also carries is the case this is asked in, so the row that
/// holds it is the row that is not archived.
fn slug_id(workspace: &Workspace, slug: &str) -> String {
    let listed = json(&workspace.nodal(&["ls", "--json"]));
    listed["units"]
        .as_array()
        .expect("the list has units")
        .iter()
        .find(|row| row["slug"] == slug && row["status"] != "archived")
        .and_then(|row| row["id"].as_str())
        .expect("the unit is on the list")
        .to_owned()
}

#[test]
fn a_unit_that_has_been_reclaimed_is_not_reclaimed_again() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    drop(stdout(&reclaim(&workspace, "worker-import")));

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("was reclaimed already"), "{}", stderr(&refused));
    assert_eq!(workspace.trashed().len(), 1, "and nothing was moved twice");
}

// ---------------------------------------------------------------------------
// The runtime.
// ---------------------------------------------------------------------------

/// A planted process is stopped, and its port comes back.
#[test]
fn a_process_planted_in_a_unit_is_stopped_and_its_port_comes_back() {
    let workspace = workspace();
    let created = json(&workspace.nodal(&["new", "--name", "worker-import", "--json"]));
    let (id, home) = workspace.one_unit_and_home();
    let port = created["unit"]["environment"]["ports"]["app"].as_u64().expect("a port was granted");
    let planted = plant(&home, &id, &readable_sleep(300));

    let report = json(&workspace.nodal(&["reclaim", "worker-import", "--json"]));
    assert_process_signal(&report);
    // The port is given back in the transaction that records the reclaim.
    assert_eq!(report["released"]["allocated"], serde_json::json!([port]));
    let store = workspace.store();
    assert!(
        nodal_core::store::port_allocations::get(store.conn(), u16::try_from(port).unwrap())
            .unwrap()
            .is_none(),
        "the port is free for the next unit"
    );
    drop(store);

    assert_eq!(report["stopped"]["asked"].as_array().unwrap().len(), 1, "{report}");
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    wait_for("the planted process to be stopped", || !alive(planted.pid()));
    assert_eq!(workspace.trashed().len(), 1, "the home went");
}

/// A reclaim stops a tether, and its report names the group.
///
/// A tether is a group the registry recorded, so a reclaim reaches it by record and not
/// by a reading of the process table.
#[test]
fn a_reclaim_stops_a_tether_and_names_its_group() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    let mut command = workspace.command_in(&home, &["run", "--tether", "sleep", "600"]);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let _run = Owned::spawn(&mut command);
    let group = tether_of(&workspace);
    let _held = Owned::adopt(group);

    let report = json(&workspace.nodal(&["reclaim", "worker-import", "--json"]));
    let groups: Vec<u64> = report["stopped"]["asked"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|target| target.get("group").and_then(serde_json::Value::as_u64))
        .collect();
    assert_eq!(groups, vec![u64::from(group)], "the report names the tether: {report}");
    wait_for("the tether to be stopped", || !alive(group));
    assert!(!home.exists(), "and the home went");
}

/// The process group of the one tether this project's one unit holds, once it is recorded.
fn tether_of(workspace: &Workspace) -> u32 {
    let unit = workspace.one_unit().id;
    until("a tether to be recorded", || {
        let store = workspace.store();
        let environment = environments::latest_for_unit(store.conn(), unit).unwrap().unwrap();
        let open = sessions::list_open_tethers(store.conn(), environment.id).unwrap();
        open.first().and_then(|session| session.pgid)
    })
}

/// Start a detached process inside a home, carrying that unit's identifier, and answer
/// with its process id.
///
/// Detached on purpose. A process this test started and has not waited for stays in the
/// table as a zombie after it is stopped, and "does this process exist" then answers yes
/// for something that is not running — which would report a stop that worked as a
/// process left behind. Starting it from a shell that then exits hands it to the system,
/// which reaps it properly.
fn plant(home: &Path, unit: &str, command: &str) -> Owned {
    let line = format!("{command} >/dev/null 2>&1 & printf %s \"$!\"");
    let mut planting = Command::new("sh");
    planting.arg("-c").arg(line).current_dir(home).env("NODAL_ID", unit);
    let output = process::mark(&mut planting).output().unwrap();
    let pid: u32 = String::from_utf8(output.stdout).unwrap().trim().parse().unwrap();
    assert!(alive(pid), "the plant is running");
    // Adopted, not owned as a group: the shell that started it has already exited, and
    // this process is nobody's child. What the test asserts is still that `nodal reclaim`
    // stopped it. The adoption is what reclaims it when an assertion before that one
    // fails, and it signals nothing it cannot prove is still the process it adopted.
    Owned::adopt(pid)
}

// ---------------------------------------------------------------------------
// The hooks.
// ---------------------------------------------------------------------------

/// A recipe whose four hooks each append their own name to one file.
const HOOKS: &str = r#"
[hooks]
pre_new = "printf 'pre_new %s\\n' \"$NODAL_UNIT\" >> \"$NODAL_SOURCE/hooks.log\""
post_new = "printf 'post_new %s\\n' \"$NODAL_ROOT\" >> \"$NODAL_SOURCE/hooks.log\""
pre_reclaim = "printf 'pre_reclaim %s\\n' \"$PWD\" >> \"$NODAL_SOURCE/hooks.log\""
post_reclaim = "printf 'post_reclaim %s\\n' \"$NODAL_ROOT\" >> \"$NODAL_SOURCE/hooks.log\""
"#;

#[test]
fn the_four_hooks_run_in_order_and_are_told_which_unit_they_are_about() {
    let workspace = workspace_with_recipe(HOOKS);
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    // Resolved now, while the directory is still there. A path that has been moved
    // cannot be resolved, and asking afterwards would quietly answer with the
    // unresolved name and compare it against the resolved one the shell reported.
    let stood_in = resolved(&home);
    drop(stdout(&reclaim(&workspace, "worker-import")));

    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    let phases: Vec<&str> = log.lines().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(phases, ["pre_new", "post_new", "pre_reclaim", "post_reclaim"], "{log}");
    assert!(log.contains("pre_new worker-import"), "{log}");
    // Every path a hook is told about is resolved, whether it arrives as a variable or
    // as the directory the hook is started in. `$PWD` is what the shell got from the
    // kernel and has always been resolved; `NODAL_ROOT` now agrees with it. On a host
    // whose temporary directory is reached through a symbolic link — macOS reaches
    // `/var` through `/private/var` — that is not the text the registry holds, and a
    // hook that compared the two would have found one directory under two names.
    assert!(
        log.contains(&format!("post_new {}", stood_in.display())),
        "post_new is told the home by the name the filesystem uses: {log}"
    );
    assert!(
        log.contains(&format!("pre_reclaim {}", stood_in.display())),
        "pre_reclaim runs in the home it is about: {log}"
    );
    let trashed = resolved(&workspace.trashed().pop().unwrap());
    assert!(
        log.contains(&format!("post_reclaim {}", trashed.display())),
        "post_reclaim is told where the home went: {log}"
    );
}

#[test]
fn a_hook_command_nobody_approved_refuses_to_run() {
    let workspace = workspace_with_recipe(HOOKS);
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    // The recipe changes after it was approved, which is what arrives with a pull.
    workspace.write_recipe(&HOOKS.replace("pre_reclaim %s", "SOMETHING ELSE %s"));

    let refused = reclaim(&workspace, "worker-import");
    assert!(!refused.status.success(), "an unapproved command does not run");
    let told = stderr(&refused);
    assert!(told.contains("pre_reclaim hook"), "{told}");
    assert!(told.contains("is not approved"), "{told}");
    assert!(told.contains("SOMETHING ELSE"), "the message shows what it would have run: {told}");

    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    assert!(!log.contains("SOMETHING ELSE"), "and it did not run: {log}");
    assert!(workspace.trashed().is_empty(), "the refusal happened before anything moved");

    // Approving is what `nodal init` does, and the same reclaim then works.
    drop(stdout(&workspace.nodal(&["init", "--force"])));
    drop(stdout(&reclaim(&workspace, "worker-import")));
    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    assert!(log.contains("SOMETHING ELSE"), "{log}");
}

#[test]
fn a_hook_that_fails_stops_the_reclaim_before_anything_moves() {
    let workspace = workspace_with_recipe("[hooks]\npre_reclaim = \"exit 3\"\n");
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();

    let refused = reclaim(&workspace, "worker-import");
    assert!(!refused.status.success(), "a hook that fails is a reclaim that does not happen");
    let told = stderr(&refused);
    assert!(told.contains("pre_reclaim hook failed"), "{told}");
    assert!(told.contains("exited 3"), "the message says how: {told}");
    assert!(home.is_dir(), "and the home is where it was");
    assert!(workspace.trashed().is_empty());
}

#[test]
fn no_hooks_runs_none_of_them_without_needing_an_approval() {
    let workspace = workspace();
    workspace.write_recipe(HOOKS);
    drop(stdout(&workspace.nodal(&["--no-hooks", "new", "--name", "worker-import"])));
    drop(stdout(&workspace.nodal(&["--no-hooks", "reclaim", "worker-import"])));
    assert!(!workspace.source.join("hooks.log").exists(), "no hook ran");
    assert_eq!(workspace.trashed().len(), 1, "and the unit was still reclaimed");
}

// ---------------------------------------------------------------------------
// The sweep.
// ---------------------------------------------------------------------------

#[test]
fn gc_removes_a_trashed_home_once_its_retention_has_run_out_and_not_before() {
    let workspace = workspace_with_recipe("[reclaim]\ntrash_retention = 14\n");
    drop(stdout(&workspace.nodal(&["new", "--name", "kept"])));
    drop(stdout(&reclaim(&workspace, "kept")));
    let kept = workspace.trashed().pop().expect("the home is in the trash");

    let held = stdout(&workspace.nodal(&["gc"]));
    assert!(held.contains("0 homes"), "nothing goes before its retention is up: {held}");
    assert!(kept.is_dir(), "and the directory is still there");

    // A project that keeps nothing: the same reclaim, the same sweep, the other answer.
    workspace.write_recipe("[reclaim]\ntrash_retention = 0\n");
    drop(stdout(&workspace.nodal(&["init", "--force"])));
    drop(stdout(&workspace.nodal(&["new", "--name", "swept"])));
    drop(stdout(&reclaim(&workspace, "swept")));
    assert_eq!(workspace.trashed().len(), 2, "two homes in the trash");

    let swept = stdout(&workspace.nodal(&["gc"]));
    assert!(swept.contains("1 home"), "{swept}");
    assert!(swept.contains("swept"), "the sweep names what went: {swept}");
    assert_eq!(workspace.trashed(), vec![kept.clone()], "the one that expired went, and only it");
    assert!(kept.is_dir());

    let store = workspace.store();
    let left = trash::list(store.conn()).unwrap();
    assert_eq!(left.len(), 1, "and the row went with the directory");
    assert_eq!(left[0].slug.as_str(), "kept");
}

// ---------------------------------------------------------------------------
// The kill.
// ---------------------------------------------------------------------------

/// A reclaim killed between two steps is rolled back by the next invocation.
///
/// What holds the run open long enough to be killed is a process that ignores being
/// asked to stop: the step waits out its grace period, and the kill lands inside that
/// window.
#[test]
fn a_reclaim_killed_between_two_steps_is_rolled_back_by_the_next_invocation() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (id, home) = workspace.one_unit_and_home();
    let planted = plant(&home, &id, &format!("trap '' TERM; {}", readable_sleep(300)));

    let mut command = workspace.command(&["reclaim", "worker-import"]);
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = Owned::spawn(&mut command);
    wait_for_the_reclaim_to_start(&workspace, &child);
    child.reclaim();

    assert!(home.is_dir(), "the kill landed before the home was moved");
    assert!(workspace.trashed().is_empty());

    // What a person does next. The run is rolled back, and the unit is still there.
    let next = workspace.nodal(&["reclaim", "worker-import"]);
    let told = stderr(&next);
    assert!(told.contains("reclaim (worker-import) was interrupted"), "{told}");
    assert!(told.contains("rolled back"), "{told}");
    assert!(next.status.success(), "and the second reclaim finishes: {told}");
    assert!(!home.exists());
    assert_eq!(workspace.trashed().len(), 1);
    wait_for("the planted process to be stopped", || !alive(planted.pid()));
}

/// Wait until the reclaim has journalled itself and is inside its first step.
fn wait_for_the_reclaim_to_start(workspace: &Workspace, child: &Owned) {
    let deadline = Instant::now() + REACH_TIMEOUT;
    while Instant::now() < deadline {
        let store = workspace.store();
        let running = journal::unfinished(store.conn()).unwrap();
        if running.iter().any(|record| record.kind == "reclaim") {
            return;
        }
        drop(store);
        assert!(!child.exited(), "the reclaim finished before it could be killed");
        std::thread::sleep(POLL);
    }
    panic!("no reclaim was journalled within {REACH_TIMEOUT:?}");
}

// ---------------------------------------------------------------------------
// The directory Nodal did not make.
// ---------------------------------------------------------------------------

#[test]
fn a_checkout_adopted_in_place_is_unregistered_and_never_trashed() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let root = workspace.adopt_in_place("in-place");

    let report = stdout(&workspace.nodal(&["reclaim", "in-place"]));
    assert!(report.contains("left in place"), "{report}");
    assert!(report.contains(root.to_str().unwrap()), "{report}");
    assert!(
        !report.contains("git worktree remove"),
        "a clone adopted in place is not a worktree to remove: {report}"
    );
    assert_nothing_left(&report);

    assert!(root.is_dir(), "the person's own directory is where it was");
    assert!(root.join("app").join("main.txt").is_file(), "with everything in it");
    assert!(root.join(".git").is_dir(), "and its repository");
    assert!(workspace.trashed().is_empty(), "nothing of it went to the trash");

    let store = workspace.store();
    assert!(trash::list(store.conn()).unwrap().is_empty(), "and nothing was recorded as trash");
    let unit = units::list(store.conn(), projects::list(store.conn()).unwrap()[0].id)
        .unwrap()
        .into_iter()
        .find(|unit| unit.slug.as_str() == "in-place")
        .expect("the unit is still on record");
    assert_eq!(unit.status, UnitStatus::Archived, "it is unregistered, not forgotten");
    let environment =
        environments::latest_for_unit(store.conn(), unit.id).unwrap().expect("its row is there");
    assert_eq!(environment.state, EnvState::Absent);

    // The one home Nodal did make is untouched by any of this.
    assert_eq!(workspace.homes().len(), 1, "{:?}", workspace.homes());
}

#[test]
fn a_home_somebody_deleted_by_hand_still_closes_its_rows() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (_, home) = workspace.one_unit_and_home();
    std::fs::remove_dir_all(&home).unwrap();

    let report = stdout(&workspace.nodal(&["reclaim", "worker-import"]));
    assert_nothing_left(&report);
    assert!(workspace.trashed().is_empty(), "there was nothing to move");

    let store = workspace.store();
    let unit = units::list(store.conn(), projects::list(store.conn()).unwrap()[0].id)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(unit.status, UnitStatus::Archived);
}

/// A dirty adopted worktree is refused. Nothing runnable is printed, even with `--yes`.
#[test]
fn reclaim_of_a_dirty_adopted_worktree_refuses_and_prints_nothing_runnable() {
    let workspace = workspace();
    let dirty = workspace.adopt_worktree("dirty");
    std::fs::write(dirty.join("only-here.txt"), "unique\n").unwrap();

    let refused = workspace.nodal(&["reclaim", "dirty", "--yes"]);
    assert!(!refused.status.success(), "a dirty worktree was reclaimed");
    let told = stderr(&refused);
    assert!(told.contains("untracked files"), "{told}");
    assert!(!told.contains("git worktree remove"), "{told}");
    assert!(!answer(&refused).contains("git worktree remove"), "{}", answer(&refused));
    assert!(dirty.is_dir(), "the dirty worktree was removed");
    assert!(dirty.join("only-here.txt").is_file(), "the unique work is still there");
}

/// A done adopted worktree prints `git worktree remove`. `--yes` runs it. Default is no.
#[test]
fn reclaim_of_a_done_adopted_worktree_prints_the_removal_line_and_yes_runs_it() {
    let workspace = workspace();
    let stays = workspace.adopt_worktree("stays");
    let gone = workspace.adopt_worktree("gone");

    let printed = stdout(&workspace.nodal(&["reclaim", "stays"]));
    let stay_cmd = format!("git worktree remove {}", stays.display());
    assert!(printed.contains(&stay_cmd), "{printed}");
    assert!(stays.is_dir(), "without --yes the worktree stays");

    let removed = stdout(&workspace.nodal(&["reclaim", "gone", "--yes"]));
    let gone_cmd = format!("git worktree remove {}", gone.display());
    assert!(removed.contains(&gone_cmd), "{removed}");
    assert!(!gone.exists(), "--yes ran git worktree remove");
    let listed = git(&workspace.source, &["worktree", "list"]);
    assert!(!listed.contains(gone.to_str().unwrap()), "git no longer names it: {listed}");
    assert!(listed.contains(stays.to_str().unwrap()), "the unconfirmed worktree remains: {listed}");
}

// ------------------------------------------- what the runner records before it acts

/// Every operation that changes a unit's tree or its refs records the home first, on a
/// ref named by the run. A reclaim moves a home away, so the record travels with it into
/// the trash and is what a person reads a file back out of.
///
/// This is the ordinary reclaim, with nothing unique in the home and no `--force`. The
/// work-in-progress ref is a forced reclaim's; this one is the runner's.
#[test]
fn an_ordinary_reclaim_records_the_home_before_it_moves_it() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (id, _) = workspace.one_unit_and_home();

    drop(stdout(&reclaim(&workspace, "worker-import")));

    let trashed = workspace.trashed().pop().expect("the home is in the trash");
    let refs =
        git(&trashed, &["for-each-ref", "--format=%(refname)", &format!("refs/nodal/{id}/")]);
    let recorded: Vec<&str> =
        refs.lines().filter(|name| name.contains(&format!("/{id}/pre/"))).collect();
    assert_eq!(recorded.len(), 1, "one record per run: {refs}");

    let store = workspace.store();
    for record in recorded {
        let listed = git(&trashed, &["ls-tree", "-r", "--name-only", record]);
        assert!(listed.contains("app/main.txt"), "the record holds the home: {listed}");
        let operation = record.rsplit('/').next().unwrap();
        let run = journal::get(store.conn(), operation.parse().unwrap()).unwrap().unwrap();
        assert_eq!(run.kind, "reclaim", "the ref is named by the run that took it");
    }
}

/// Nothing about the record reaches the working tree or the index. The commit is built
/// in an index of its own, so a person's staged work is exactly as they left it — which
/// is the whole reason the snapshot is plumbing and not `git add -A`.
#[test]
fn the_record_touches_neither_the_index_nor_the_working_tree() {
    let workspace = workspace();
    drop(stdout(&workspace.nodal(&["new", "--name", "worker-import"])));
    let (id, home) = workspace.one_unit_and_home();
    std::fs::write(home.join("staged.txt"), "staged\n").unwrap();
    drop(git(&home, &["add", "staged.txt"]));
    drop(git(
        &home,
        &["-c", "user.email=t@example.invalid", "-c", "user.name=test", "commit", "-m", "staged"],
    ));
    std::fs::write(home.join("app").join("main.txt"), "edited\n").unwrap();
    std::fs::write(home.join("second.txt"), "also staged\n").unwrap();
    drop(git(&home, &["add", "second.txt"]));
    let before = git(&home, &["status", "--porcelain"]);

    drop(stdout(&workspace.nodal(&["reclaim", "worker-import", "--force"])));

    let trashed = workspace.trashed().pop().expect("the home is in the trash");
    assert_eq!(git(&trashed, &["status", "--porcelain"]), before, "the record changed the tree");
    assert_eq!(
        std::fs::read_to_string(trashed.join("app").join("main.txt")).unwrap(),
        "edited\n",
        "the record rewrote a file of the home"
    );
    let refs =
        git(&trashed, &["for-each-ref", "--format=%(refname)", &format!("refs/nodal/{id}/")]);
    assert!(refs.contains(&format!("/{id}/pre/")), "the run left no record: {refs}");
}
