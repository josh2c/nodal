//! Acceptance test for T1.3: `nodal new`, end to end, against a real repository.
//!
//! Every assertion here is about a property a person can check for themselves in a unit
//! Nodal made: `git status` says nothing, `git worktree list` names one checkout, two
//! units cannot be told apart by the files in them, and the second unit on a branch is
//! refused by name rather than by a constraint violation.
//!
//! The last test is the one that needs a second process. What makes an interruption
//! different from a failure is that no code of ours runs afterwards, so the account the
//! next invocation reads has to have been written down before the kill. The test runs
//! the real binary, waits until the operation has reached a point inside the home, sends
//! `SIGKILL`, and then runs `nodal new` again — which is what a person would do — and
//! asserts that the killed run left nothing at all.

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// How long a test waits for a killed run to reach the home it is building.
const REACH_TIMEOUT: Duration = Duration::from_secs(60);

/// How often it looks. A create of a small project is over in milliseconds, so the
/// poll is as tight as it can be and the project the kill test uses is not small.
const POLL: Duration = Duration::from_millis(1);

/// How many files the kill test's project carries in its dependency directory.
///
/// They are what makes the clone long enough to be killed part-way through. They are
/// ignored by Git and kept by the exclusion policy, which is the shape of every real
/// project this tool is for: most of the bytes are installed dependencies.
const BULK_FILES: usize = 12_000;

/// A project to make units in, and the state directory they go in.
struct Workspace {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The project's repository.
    source: PathBuf,
    /// Nodal's state directory: the registry, and every home.
    state: PathBuf,
}

impl Workspace {
    /// A one-commit repository and an empty state directory beside it.
    fn new() -> Self {
        Self::build(0)
    }

    /// The same, with a dependency directory of `bulk` ignored files in it.
    fn with_bulk() -> Self {
        Self::build(BULK_FILES)
    }

    /// A repository, its one commit, and however many ignored files were asked for.
    fn build(bulk: usize) -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "node_modules/\n").unwrap();
        let modules = source.join("node_modules");
        std::fs::create_dir_all(&modules).unwrap();
        for index in 0..bulk {
            std::fs::write(modules.join(format!("{index}.js")), "module.exports = {};\n").unwrap();
        }
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "unit@example.invalid"],
            vec!["config", "user.name", "Test"],
            vec!["add", "-A"],
            vec!["commit", "-qm", "first"],
        ] {
            let status = Command::new("git").args(&args).current_dir(&source).status().unwrap();
            assert!(status.success(), "git {args:?}");
        }
        Self { _root: root, source, state }
    }

    /// `nodal` with this workspace's state directory, run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The same invocation, not yet run.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nodal"));
        command.args(args).current_dir(&self.source).env("NODAL_HOME", &self.state);
        // The per-machine secrets file is shared by every unit on a machine, and a test
        // must never read or create the one belonging to whoever is running it.
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command
    }

    /// Where this project's homes are, whatever the project ended up being called, and
    /// `None` until the first create has made the directory.
    fn homes(&self) -> Option<PathBuf> {
        std::fs::read_dir(&self.state)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join("e"))
            .find(|path| path.is_dir())
    }

    /// Every home this project has, in name order.
    fn units(&self) -> Vec<PathBuf> {
        let mut homes: Vec<PathBuf> = self
            .homes()
            .into_iter()
            .flat_map(|directory| std::fs::read_dir(directory).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect();
        homes.sort();
        homes
    }
}

/// Standard output as text, with the command insisted upon.
fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// `git` in a directory, as text.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_new_unit_has_a_clean_status_and_a_checkout_of_its_own() {
    let workspace = Workspace::new();
    let report = stdout(&workspace.nodal(&["new", "worker import: missing supervisor"]));
    assert!(report.contains("worker-import"), "{report}");
    assert!(report.contains("nodal/worker-import"), "{report}");

    let home = workspace.units().pop().unwrap();
    assert_eq!(
        git(&home, &["status", "--porcelain", "--untracked-files=all"]),
        "",
        "the files nodal writes are hidden, so a unit is clean the moment it is made"
    );
    assert_eq!(git(&home, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(), "nodal/worker-import");

    let worktrees = git(&home, &["worktree", "list"]);
    assert_eq!(worktrees.lines().count(), 1, "a unit is its own repository: {worktrees}");
    assert!(worktrees.contains(home.to_str().unwrap()), "{worktrees}");

    assert!(home.join(".nodal").join("id").is_file(), "the home carries its marker");
    assert!(home.join(".nodal").join("manifest.toml").is_file());
    assert!(home.join(".envrc").is_file());
}

#[test]
fn a_second_unit_on_one_branch_is_refused_and_the_holder_is_named() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));

    let refused = workspace.nodal(&["new", "--name", "worker-import"]);
    assert!(!refused.status.success(), "the second create must fail");
    let message = String::from_utf8(refused.stderr).unwrap();
    assert!(message.contains("nodal/worker-import"), "{message}");
    assert!(message.contains("held by the open unit worker-import"), "{message}");
    assert_eq!(workspace.units().len(), 1, "the refusal left no home behind");
}

#[test]
fn two_units_are_two_working_copies_that_cannot_see_each_other() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "worker import"]));
    stdout(&workspace.nodal(&["new", "payroll export"]));

    let homes = workspace.units();
    assert_eq!(homes.len(), 2, "{homes:?}");
    std::fs::write(homes[0].join("app").join("only-here.txt"), "one\n").unwrap();
    assert!(homes[0].join("app").join("only-here.txt").is_file());
    assert!(!homes[1].join("app").join("only-here.txt").exists(), "a write in one is not in two");
    assert!(!workspace.source.join("app").join("only-here.txt").exists(), "nor in the project");

    let branches: Vec<String> =
        homes.iter().map(|home| git(home, &["rev-parse", "--abbrev-ref", "HEAD"])).collect();
    assert_ne!(branches[0], branches[1], "each unit owns its own branch");
}

#[test]
fn a_home_that_would_sit_inside_the_project_is_refused() {
    let workspace = Workspace::new();
    let inside = workspace.source.join("state");
    let refused =
        workspace.command(&["new", "inside"]).env("NODAL_HOME", &inside).output().unwrap();
    assert!(!refused.status.success());
    let message = String::from_utf8(refused.stderr).unwrap();
    assert!(message.contains("the tree it would be cloned from"), "{message}");
}

#[test]
fn a_killed_create_leaves_nothing_once_the_next_invocation_resolves_it() {
    let workspace = Workspace::with_bulk();
    let mut child = workspace
        .command(&["new", "worker import"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let half_made = wait_for_a_home(&workspace, &mut child);
    kill(&mut child);
    assert!(half_made.exists(), "the killed run left the home it was building");

    // What a person does next. The preamble resolves the interrupted run before this
    // create starts, so what is left afterwards is this unit's home and nothing else.
    let second = workspace.command(&["new", "payroll export"]).output().unwrap();
    let told = String::from_utf8(second.stderr.clone()).unwrap();
    assert!(stdout(&second).contains("payroll-export"), "{told}");
    assert!(told.contains("was interrupted and rolled back"), "the person is told: {told}");

    let homes = workspace.units();
    assert_eq!(homes.len(), 1, "the killed run's home is gone: {homes:?}");
    assert!(!half_made.exists());
    assert!(homes[0].join(".nodal").join("id").is_file());
}

/// Wait until the killed run has made a home, and answer with it.
///
/// The kill lands while the operation is inside that directory — cloning it, scrubbing
/// it, or writing its files — which is the state the next invocation has to cope with.
fn wait_for_a_home(workspace: &Workspace, child: &mut Child) -> PathBuf {
    let deadline = Instant::now() + REACH_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(home) = workspace.units().pop() {
            return home;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the create finished before it could be killed: {status}");
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    panic!("no home appeared within {REACH_TIMEOUT:?}");
}

/// `SIGKILL`, so that nothing of the child's own runs afterwards.
fn kill(child: &mut Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}
