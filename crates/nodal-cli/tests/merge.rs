//! Acceptance test for `nodal merge`, end to end, against real repositories.
//!
//! Every assertion is about something a person can check for themselves once the
//! command has finished: the target branch in their own checkout carries the work, the
//! home is in the trash, `git status` in a home the merge did not commit still shows
//! what was there, and the ref that holds the folded commits answers `git log`.
//!
//! Four of them are the ones the command exists for.
//!
//! **One command.** A dirty unit reaches the project's `main` and leaves nothing behind
//! but its trash entry.
//!
//! **Every `--no-` flag drops exactly its stage**, and nothing else changes.
//!
//! **A conflict stops cleanly.** The unit is left in the middle of a rebase with the
//! paths named; a second `nodal merge` carries it on, and `--abort` puts the branch back
//! where the merge found it. Both ways out are proved here.
//!
//! **Nothing is rewritten.** A target that moved under the merge is refused rather than
//! forced, and a merge killed with `SIGKILL` part-way is rolled back by the next
//! invocation, which leaves the work in the home exactly as it was.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nodal_core::lifecycle::journal;
use nodal_safety::git::{commit, git_text as git};
use nodal_safety::text::{answer, stderr, stdout};
use nodal_safety::{InState as _, Workspace};

/// How long a test waits for a killed run to reach the step it is being killed in.
const REACH_TIMEOUT: Duration = Duration::from_secs(60);

/// How often any wait looks.
const POLL: Duration = Duration::from_millis(5);

/// The readings this suite needs beyond the ones the shared fixture has.
trait Merging {
    /// The subject lines of the target branch, newest first.
    fn main_log(&self) -> Vec<String>;
}

impl Merging for Workspace {
    fn main_log(&self) -> Vec<String> {
        git(&self.source, &["log", "--format=%s", "main"]).lines().map(ToOwned::to_owned).collect()
    }
}

/// Whether a repository is in the middle of a rebase.
fn rebasing(repo: &Path) -> bool {
    Path::new(git(repo, &["rev-parse", "--absolute-git-dir"]).trim()).join("rebase-merge").exists()
}

// ---------------------------------------------------------------------------
// One command, from a dirty unit to a merged target.
// ---------------------------------------------------------------------------

#[test]
fn a_dirty_unit_reaches_the_target_in_one_command_and_the_home_is_in_the_trash() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("main.txt"), "edited by the unit\n").unwrap();
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();

    let merged = workspace.nodal(&["merge", "worker-import", "--yes"]);
    let report = stdout(&merged);
    assert!(merged.status.success(), "{}", stderr(&merged));
    assert!(report.contains("nodal/worker-import into main"), "{report}");
    assert!(report.contains("the home is in the trash"), "{report}");

    // The plan is shown first, and on standard error, so the answer stays one document.
    let plan = stderr(&merged);
    assert!(plan.contains("plan"), "{plan}");
    assert!(plan.contains("2 changed path(s)"), "{plan}");

    assert_eq!(workspace.main_log().len(), 2, "one commit landed on main");
    assert_eq!(
        git(&workspace.source, &["show", "main:app/main.txt"]),
        "edited by the unit\n",
        "and it carries the work"
    );
    assert_eq!(git(&workspace.source, &["show", "main:app/new.txt"]), "made here\n");
    assert!(!home.exists(), "the home is not where it was");
    assert!(workspace.homes().is_empty(), "no live home is left");
    assert_eq!(workspace.trashed().len(), 1, "and the trash holds it");
}

#[test]
fn the_commits_a_squash_folded_stay_on_the_premerge_ref() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    for step in ["one", "two"] {
        std::fs::write(home.join("app").join(format!("{step}.txt")), format!("{step}\n")).unwrap();
        commit(&home, &format!("work {step}"));
    }
    let id = workspace.one_unit().id.to_string();
    let before = git(&home, &["rev-parse", "HEAD"]).trim().to_owned();

    drop(stdout(&workspace.nodal(&[
        "merge",
        "worker-import",
        "--yes",
        "-m",
        "one squashed commit",
    ])));

    assert_eq!(workspace.main_log().first().map(String::as_str), Some("one squashed commit"));
    assert_eq!(workspace.main_log().len(), 2, "two commits became one: {:?}", workspace.main_log());

    let trashed = workspace.trashed().pop().expect("the home is in the trash");
    let reference = format!("refs/nodal/{id}/premerge");
    let kept = git(&trashed, &["rev-parse", &reference]);
    assert_eq!(kept.trim(), before, "the ref holds the branch as it was");
    let folded = git(&trashed, &["log", "--format=%s", &reference]);
    assert!(folded.contains("work one") && folded.contains("work two"), "{folded}");
}

// ---------------------------------------------------------------------------
// Every flag drops exactly its stage.
// ---------------------------------------------------------------------------

#[test]
fn no_commit_leaves_the_work_where_it_is_and_the_unit_with_it() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("committed.txt"), "committed\n").unwrap();
    commit(&home, "the part that was committed");
    std::fs::write(home.join("app").join("main.txt"), "edited but not committed\n").unwrap();

    let merged = workspace.nodal(&["merge", "worker-import", "--yes", "--no-commit"]);
    let report = answer(&merged);
    assert!(!merged.status.success(), "the unit was not removed, so the exit code says so");
    assert!(report.contains("was not removed"), "{report}");

    assert_eq!(
        git(&workspace.source, &["show", "main:app/committed.txt"]),
        "committed\n",
        "what was committed is on main"
    );
    assert_eq!(
        git(&workspace.source, &["show", "main:app/main.txt"]),
        "shared\n",
        "and what was not is not"
    );
    assert!(home.is_dir(), "the home is where it was");
    assert_eq!(git(&home, &["status", "--porcelain"]).lines().count(), 1, "and so is the work");
}

#[test]
fn no_squash_puts_every_commit_of_the_branch_on_the_target() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    for step in ["one", "two"] {
        std::fs::write(home.join("app").join(format!("{step}.txt")), format!("{step}\n")).unwrap();
        commit(&home, &format!("work {step}"));
    }

    drop(stdout(&workspace.nodal(&["merge", "worker-import", "--yes", "--no-squash"])));

    let log = workspace.main_log();
    assert_eq!(log, ["work two", "work one", "first"], "both commits are on main: {log:?}");
}

#[test]
fn no_rebase_refuses_a_target_that_has_moved_and_a_rebase_takes_it() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();
    // Somebody else merges something while this unit is open.
    std::fs::write(workspace.source.join("app").join("other.txt"), "theirs\n").unwrap();
    commit(&workspace.source, "somebody else");

    let refused = workspace.nodal(&["merge", "worker-import", "--yes", "--no-rebase"]);
    assert!(!refused.status.success(), "a target that moved is not rewritten");
    let told = stderr(&refused);
    assert!(told.contains("has moved"), "{told}");
    assert_eq!(workspace.main_log().len(), 2, "main was not moved: {:?}", workspace.main_log());
    assert!(home.is_dir(), "and the unit is still there");
    assert_eq!(
        git(&home, &["status", "--porcelain"]).lines().count(),
        1,
        "with its work put back exactly as it was"
    );

    // The same merge, allowed to rebase, takes the target as it is now.
    drop(stdout(&workspace.nodal(&["merge", "worker-import", "--yes"])));
    let log = workspace.main_log();
    assert_eq!(log.len(), 3, "{log:?}");
    assert_eq!(git(&workspace.source, &["show", "main:app/other.txt"]), "theirs\n");
    assert_eq!(git(&workspace.source, &["show", "main:app/new.txt"]), "made here\n");
}

#[test]
fn no_remove_leaves_the_unit_where_it_is() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();

    let report = stdout(&workspace.nodal(&["merge", "worker-import", "--yes", "--no-remove"]));
    assert!(report.contains("not asked for"), "{report}");

    assert_eq!(workspace.main_log().len(), 2, "the merge still happened");
    assert!(home.is_dir(), "and the home is where it was");
    assert!(workspace.trashed().is_empty(), "with nothing in the trash");
    let listed = stdout(&workspace.nodal(&["ls"]));
    assert!(listed.contains("worker-import"), "the unit is still on the list: {listed}");
}

// ---------------------------------------------------------------------------
// A conflict, and both ways out of it.
// ---------------------------------------------------------------------------

/// A unit and a target that have each changed the same line.
fn conflicting() -> (Workspace, PathBuf) {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("main.txt"), "the unit's line\n").unwrap();
    std::fs::write(workspace.source.join("app").join("main.txt"), "somebody else's line\n")
        .unwrap();
    commit(&workspace.source, "somebody else");
    (workspace, home)
}

#[test]
fn a_conflict_stops_the_merge_and_a_second_merge_resumes_it() {
    let (workspace, home) = conflicting();

    let stopped = workspace.nodal(&["merge", "worker-import", "--yes"]);
    let report = answer(&stopped);
    assert!(!stopped.status.success(), "a merge that stopped says so in its exit code");
    assert!(report.contains("the rebase stopped"), "{report}");
    assert!(report.contains("app/main.txt"), "it names the paths: {report}");
    assert!(report.contains("nodal merge worker-import"), "and both ways out: {report}");
    assert!(report.contains("--abort"), "{report}");

    assert!(rebasing(&home), "the unit is left in the middle of the rebase");
    assert_eq!(workspace.main_log().len(), 2, "and main was not moved");
    assert!(workspace.trashed().is_empty(), "and nothing was removed");

    resume(&workspace, &home);
}

/// What a person does next: resolve the paths, then run the same command again.
fn resume(workspace: &Workspace, home: &Path) {
    std::fs::write(home.join("app").join("main.txt"), "both lines\n").unwrap();
    drop(git(home, &["add", "app/main.txt"]));
    let finished = workspace.nodal(&["merge", "worker-import", "--yes"]);
    assert!(finished.status.success(), "{}", stderr(&finished));
    assert!(!home.exists(), "the home went to the trash");
    assert_eq!(git(&workspace.source, &["show", "main:app/main.txt"]), "both lines\n");
    assert_eq!(workspace.main_log().len(), 3, "{:?}", workspace.main_log());
}

#[test]
fn a_conflict_can_be_aborted_and_the_branch_is_back_where_the_merge_found_it() {
    let (workspace, home) = conflicting();
    let id = workspace.one_unit().id.to_string();
    let stopped = answer(&workspace.nodal(&["merge", "worker-import", "--yes"]));
    assert!(stopped.contains("the rebase stopped"), "{stopped}");
    let stopped_at = git(&home, &["rev-parse", &format!("refs/nodal/{id}/premerge")]);

    let report = stdout(&workspace.nodal(&["merge", "worker-import", "--abort"]));
    assert!(report.contains("restore"), "{report}");

    assert!(!rebasing(&home), "the rebase is over");
    assert_eq!(
        git(&home, &["rev-parse", "HEAD"]),
        stopped_at,
        "the branch is back at the commit the merge recorded"
    );
    assert_eq!(
        git(&home, &["show", "HEAD:app/main.txt"]),
        "the unit's line\n",
        "with the work the merge committed still on it"
    );
    assert_eq!(workspace.main_log().len(), 2, "main was never moved");
    assert!(home.is_dir(), "and the unit is still there");

    // A second abort has nothing to stop, and says so rather than pretending.
    let again = workspace.nodal(&["merge", "worker-import", "--abort"]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("not in the middle of a merge"), "{}", stderr(&again));
}

// ---------------------------------------------------------------------------
// The hooks.
// ---------------------------------------------------------------------------

/// A recipe whose six hooks each append their own line to one file. The two merge hooks
/// are written with template variables rather than with the environment, because that is
/// the surface this task added.
const HOOKS: &str = r#"
[hooks]
pre_new = "printf 'pre_new %s\\n' \"$NODAL_UNIT\" >> \"$NODAL_SOURCE/hooks.log\""
post_new = "printf 'post_new %s\\n' \"$NODAL_ROOT\" >> \"$NODAL_SOURCE/hooks.log\""
pre_merge = "printf 'pre_merge %s %s %s %s\\n' '{branch}' '{sanitize}' '{hash_port}' \"$PWD\" >> '{repo_root}/hooks.log'"
post_merge = "printf 'post_merge %s %s\\n' '{unit_path}' \"$PWD\" >> '{repo_root}/hooks.log'"
pre_reclaim = "printf 'pre_reclaim %s\\n' \"$NODAL_UNIT\" >> \"$NODAL_SOURCE/hooks.log\""
post_reclaim = "printf 'post_reclaim %s\\n' \"$NODAL_UNIT\" >> \"$NODAL_SOURCE/hooks.log\""
"#;

#[test]
fn the_six_hooks_run_in_order_and_the_merge_hooks_are_told_which_branch() {
    let workspace = Workspace::with_recipe(state::BINARY, HOOKS);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();
    let stood_in = std::fs::canonicalize(&home).unwrap();

    drop(stdout(&workspace.nodal(&["merge", "worker-import", "--yes"])));

    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    let phases: Vec<&str> = log.lines().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(
        phases,
        ["pre_new", "post_new", "pre_merge", "post_merge", "pre_reclaim", "post_reclaim"],
        "{log}"
    );
    let merge_line = log.lines().find(|line| line.starts_with("pre_merge ")).unwrap();
    let filled: Vec<&str> = merge_line.split(' ').collect();
    assert_eq!(filled[1], "nodal/worker-import", "{{branch}}: {merge_line}");
    assert_eq!(filled[2], "nodal_worker_import", "{{sanitize}}: {merge_line}");
    let port: u16 = filled[3].parse().expect("{hash_port} is a port");
    assert!(
        (30_000..=32_767).contains(&port),
        "{{hash_port}} is outside the granted range: {port}"
    );
    assert_eq!(filled[4], stood_in.to_string_lossy(), "pre_merge runs in the home: {merge_line}");
    let after = log.lines().find(|line| line.starts_with("post_merge ")).unwrap();
    assert!(after.contains(&stood_in.to_string_lossy().to_string()), "{{unit_path}}: {after}");
    assert!(
        after.ends_with(
            &std::fs::canonicalize(&workspace.source).unwrap().to_string_lossy().to_string()
        ),
        "post_merge runs in the project root: {after}"
    );
}

#[test]
fn a_merge_hook_nobody_approved_refuses_to_run() {
    let workspace = Workspace::with_recipe(state::BINARY, HOOKS);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();
    // The recipe changes after it was approved, which is what arrives with a pull.
    workspace.write_recipe(&HOOKS.replace("pre_merge %s", "SOMETHING ELSE %s"));

    let refused = workspace.nodal(&["merge", "worker-import", "--yes"]);
    assert!(!refused.status.success(), "an unapproved command does not run");
    let told = stderr(&refused);
    assert!(told.contains("pre_merge hook"), "{told}");
    assert!(told.contains("is not approved"), "{told}");

    assert_eq!(workspace.main_log().len(), 1, "the refusal came before anything moved");
    assert!(home.is_dir(), "and the unit is untouched");
    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    assert!(!log.contains("SOMETHING ELSE"), "{log}");
}

// ---------------------------------------------------------------------------
// A plan nobody agreed to, and a merge nobody finished.
// ---------------------------------------------------------------------------

#[test]
fn a_plan_nobody_agreed_to_does_nothing() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();

    let asked = workspace.nodal(&["merge", "worker-import"]);
    assert!(!asked.status.success(), "a plan nobody agreed to is not run");
    let told = stderr(&asked);
    assert!(told.contains("plan"), "the plan is still shown: {told}");
    assert!(told.contains("--yes"), "and it says how a script agrees: {told}");

    assert_eq!(workspace.main_log().len(), 1, "nothing was merged");
    assert_eq!(git(&home, &["status", "--porcelain"]).lines().count(), 1, "and nothing committed");
}

/// A hook the test installs in the project's own repository, so that the merge stops
/// inside the step that moves the target branch and can be killed there.
///
/// Git runs this while the reference transaction is prepared, which is the moment the
/// step is doing its one irreversible-looking thing. Nodal never installs a Git hook
/// anywhere; this is the test's own fixture holding the door open.
const PARK: &str = r#"#!/bin/sh
if [ "$1" = prepared ]; then
  echo $$ > parked
  sleep 300
fi
"#;

#[test]
fn a_merge_killed_between_two_steps_is_rolled_back_by_the_next_invocation() {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.unit("worker-import");
    std::fs::write(home.join("app").join("new.txt"), "made here\n").unwrap();
    park(&workspace);

    let mut child = workspace
        .command(&["merge", "worker-import", "--yes"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let parked = wait_for_the_park(&workspace, &mut child);
    child.kill().unwrap();
    child.wait().unwrap();
    release(&workspace, parked);

    assert_eq!(workspace.main_log().len(), 1, "the kill landed before main moved");
    assert!(home.is_dir(), "and the home is where it was");

    // What a person does next. The run is rolled back and the work is back in the tree.
    let next = workspace.nodal(&["merge", "worker-import", "--yes"]);
    let told = stderr(&next);
    assert!(told.contains("merge (worker-import) was interrupted"), "{told}");
    assert!(told.contains("rolled back"), "{told}");
    assert!(next.status.success(), "and the second merge finishes: {told}");
    assert_eq!(workspace.main_log().len(), 2);
    assert_eq!(git(&workspace.source, &["show", "main:app/new.txt"]), "made here\n");
    assert!(!home.exists());
}

/// Install the parking hook in the project's repository.
fn park(workspace: &Workspace) {
    let hooks = workspace.source.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join("reference-transaction");
    std::fs::write(&path, PARK).unwrap();
    make_executable(&path);
}

/// Wait until the merge is held inside the step that moves the branch.
fn wait_for_the_park(workspace: &Workspace, child: &mut Child) -> u32 {
    let marker = workspace.source.join("parked");
    let deadline = Instant::now() + REACH_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(pid) =
            std::fs::read_to_string(&marker).ok().and_then(|text| text.trim().parse().ok())
        {
            assert!(journalled(workspace, "merge"), "the merge is journalled before it is killed");
            return pid;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the merge finished before it could be killed: {status}");
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    panic!("the merge did not reach the park within {REACH_TIMEOUT:?}");
}

/// Let the parked `git` go, and wait for the lock it holds to be released.
fn release(workspace: &Workspace, parked: u32) {
    let _ = Command::new("kill").args(["-9", &parked.to_string()]).status();
    let lock = workspace.source.join(".git").join("refs").join("heads").join("main.lock");
    let deadline = Instant::now() + REACH_TIMEOUT;
    while lock.exists() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    std::fs::remove_file(workspace.source.join(".git").join("hooks").join("reference-transaction"))
        .unwrap();
    std::fs::remove_file(workspace.source.join("parked")).unwrap();
    assert!(!lock.exists(), "the parked git let go of the branch");
}

/// Whether an operation of this kind is running as far as the journal knows.
fn journalled(workspace: &Workspace, kind: &str) -> bool {
    let store = workspace.store();
    let running = journal::unfinished(store.conn()).unwrap();
    running.iter().any(|record| record.kind == kind)
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
