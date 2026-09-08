//! Acceptance test for T1.3 and T1.11a: `nodal new`, end to end, against a real
//! repository.
//!
//! Every assertion here is about a property a person can check for themselves in a unit
//! Nodal made: `git status` says nothing, `git worktree list` names one checkout, two
//! units cannot be told apart by the files in them, and the second unit on a branch is
//! refused by name rather than by a constraint violation.
//!
//! Three of them are about where a home comes from. A home is a clone of a base, so
//! what the person had uncommitted in their checkout when they asked is in no unit; the
//! second unit of a workspace finds the base warm and neither clones nor installs
//! anything; and the base the first unit paid for is held against eviction while that
//! unit exists.
//!
//! Two more are about what a home does with the caches the base's build left in it. A
//! cache that records the path it was made at records the base's path, so a unit
//! carries the caches that move and none of the caches that do not, and the removal is
//! in the unit's log rather than only in the code that did it.
//!
//! The last two are the ones that need a second process. What makes an interruption
//! different from a failure is that no code of ours runs afterwards, so the account the
//! next invocation reads has to have been written down before the kill. Each test runs
//! the real binary, waits until the operation has reached the part being tested, sends
//! `SIGKILL`, and then runs `nodal new` again — which is what a person would do. A
//! create killed while it makes a home leaves nothing, because a home no registry row
//! knows about is the worst outcome there is. A create killed while it builds its base
//! keeps the base, because the base is a prerequisite with a plan of its own and
//! throwing away a clone and an install is the cost that plan exists to avoid.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::model::{Epistemic, Event, EventKind};
use nodal_core::store::{Store, events, projects, units};
use tempfile::TempDir;

/// How long a test waits for a killed run to reach the home it is building.
const REACH_TIMEOUT: Duration = Duration::from_secs(60);

/// How often it looks. A create of a small project is over in milliseconds, so the
/// poll is as tight as it can be and the project the kill test uses is not small.
const POLL: Duration = Duration::from_millis(1);

/// How many files the kill tests work with in bulk.
///
/// They are what makes an operation long enough to be killed part-way through. Where
/// they are put says which operation is being killed: in the base's dependency
/// directory they slow the clone a create makes, and committed to the repository they
/// slow the clone the base build makes.
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

    /// The same, with `BULK_FILES` committed files in it, so that cloning it is long
    /// enough to be interrupted.
    fn with_bulk() -> Self {
        Self::build(BULK_FILES)
    }

    /// A two-package repository whose base has been built and then left holding the
    /// caches a build leaves behind.
    ///
    /// The caches go in the base, not in the checkout, because that is where they come
    /// from: the install and the build that wrote them ran there, and every path they
    /// recorded is the base's. A checkout's copy would never reach a unit at all, since
    /// a base is a clone of the committed objects and nothing else.
    fn with_a_base_that_holds_caches() -> Self {
        let workspace = Self::build(0);
        workspace.commit_source();
        stdout(&workspace.nodal(&["base", "build"]));
        let base = workspace.bases().pop().expect("the base was built");
        for (relative, content) in [
            (".next/BUILD_ID", "one"),
            (".next/cache/webpack/0.pack", "/old/path/of/the/base"),
            ("apps/web/.next/BUILD_ID", "two"),
            ("apps/web/.next/cache/webpack/0.pack", "/old/path/of/the/base"),
            ("apps/web/.turbo/turbo-build.log", "cached"),
            ("apps/web/scripts/__pycache__/tool.cpython-312.pyc", "compiled"),
            ("node_modules/react/index.js", "module.exports = {};"),
        ] {
            let path = base.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        workspace
    }

    /// Commit the second package and the ignore rules its build output needs, so that
    /// the base's clone carries the source and the caches are the only thing planted.
    fn commit_source(&self) {
        std::fs::create_dir_all(self.source.join("apps/web/src")).unwrap();
        std::fs::write(self.source.join("apps/web/src/page.tsx"), "export default null;\n")
            .unwrap();
        std::fs::write(
            self.source.join(".gitignore"),
            "node_modules/\n.next/\n.turbo/\n__pycache__/\n",
        )
        .unwrap();
        git(&self.source, &["add", "-A"]);
        git(&self.source, &["commit", "-qm", "the web package"]);
    }

    /// The registry, opened for reading what a command wrote.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// Every event of the one unit this project has.
    fn events(&self) -> Vec<Event> {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
        let unit = units::list(store.conn(), project.id).unwrap().pop().expect("a unit was made");
        events::list_for_unit(store.conn(), unit.id).unwrap()
    }

    /// A repository, its one commit, and however many bulk files were asked for.
    fn build(bulk: usize) -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "node_modules/\n").unwrap();
        write_bulk(&source.join("vendor"), bulk);
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
        let mut command = state::nodal(&self.state);
        command.args(args).current_dir(&self.source);
        // The per-machine secrets file is shared by every unit on a machine, and a test
        // must never read or create the one belonging to whoever is running it.
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command
    }

    /// Where this project's homes are, whatever the project ended up being called, and
    /// `None` until the first create has made the directory.
    fn homes(&self) -> Option<PathBuf> {
        self.segment("e")
    }

    /// The same for the project's bases, which sit under their own segment so that no
    /// walk of the homes can reach one.
    fn base_directory(&self) -> Option<PathBuf> {
        self.segment("b")
    }

    /// One segment of this project's directory, `None` until something has made it.
    fn segment(&self, name: &str) -> Option<PathBuf> {
        std::fs::read_dir(&self.state)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join(name))
            .find(|path| path.is_dir())
    }

    /// Every home this project has, in name order.
    fn units(&self) -> Vec<PathBuf> {
        Self::entries(self.homes())
    }

    /// Every base this project has, in name order. A directory still being assembled
    /// carries a suffix and is not one yet.
    fn bases(&self) -> Vec<PathBuf> {
        Self::entries(self.base_directory())
            .into_iter()
            .filter(|path| !path.to_string_lossy().ends_with(".partial"))
            .collect()
    }

    /// What is directly inside a directory, in name order, and nothing when there is
    /// no such directory yet.
    fn entries(directory: Option<PathBuf>) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = directory
            .into_iter()
            .flat_map(|path| std::fs::read_dir(path).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect();
        found.sort();
        found
    }
}

/// Write `count` files into a directory, as an install writes a dependency tree.
fn write_bulk(directory: &Path, count: usize) {
    if count == 0 {
        return;
    }
    std::fs::create_dir_all(directory).unwrap();
    for index in 0..count {
        std::fs::write(directory.join(format!("{index}.js")), "module.exports = {};\n").unwrap();
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
    // Git prints the resolved path, and a temporary directory is reached through a link
    // on macOS, so the two names are compared in the one form both tools agree on.
    let resolved = home.canonicalize().unwrap();
    assert!(worktrees.contains(resolved.to_str().unwrap()), "{worktrees}");

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
fn a_unit_carries_nothing_the_checkout_had_not_committed() {
    let workspace = Workspace::new();
    // What a person's checkout looks like at the moment they ask for a unit: an edit
    // in progress, a file they have not added, and a directory Git is ignoring.
    std::fs::write(workspace.source.join("app").join("main.txt"), "half-finished\n").unwrap();
    std::fs::write(workspace.source.join("scratch.txt"), "notes to self\n").unwrap();
    write_bulk(&workspace.source.join("node_modules"), 3);

    stdout(&workspace.nodal(&["new", "worker import"]));
    let home = workspace.units().pop().unwrap();

    assert_eq!(
        std::fs::read_to_string(home.join("app").join("main.txt")).unwrap(),
        "shared\n",
        "the unit has the committed file, not the edit in progress"
    );
    assert!(!home.join("scratch.txt").exists(), "nor the file that was never added");
    assert!(!home.join("node_modules").exists(), "nor the directory Git was ignoring");
    assert_eq!(
        git(&home, &["status", "--porcelain", "--untracked-files=all"]),
        "",
        "so the unit is clean, which a clone of the checkout would not have been"
    );
}

#[test]
fn the_second_unit_of_a_workspace_reuses_the_base_the_first_one_paid_for() {
    let workspace = Workspace::new();
    let first = workspace.nodal(&["new", "worker import"]);
    let told = String::from_utf8(first.stderr.clone()).unwrap();
    stdout(&first);
    assert!(told.contains("building from"), "the first create says it is building: {told}");
    assert_eq!(workspace.bases().len(), 1, "and it built one base");

    let second = workspace.nodal(&["new", "payroll export"]);
    let told = String::from_utf8(second.stderr.clone()).unwrap();
    stdout(&second);
    assert!(told.contains("is warm for this workspace"), "the second finds it: {told}");
    assert!(!told.contains("building from"), "and clones nothing: {told}");
    assert_eq!(workspace.bases().len(), 1, "there is still one base");
    assert_eq!(workspace.units().len(), 2, "and two units on it");

    // Both units hold the base, which is what stops a sweep taking it away.
    let listed = stdout(&workspace.nodal(&["base", "ls", "--json"]));
    assert!(listed.contains("\"pins\": 2"), "{listed}");
}

#[test]
fn a_killed_create_leaves_nothing_once_the_next_invocation_resolves_it() {
    let workspace = Workspace::new();
    // The base first, so that what the kill lands in is the create and not the build.
    // The bulk goes where an install would have put it, which is what the create then
    // has to clone.
    stdout(&workspace.nodal(&["base", "build"]));
    let base = workspace.bases().pop().unwrap();
    write_bulk(&base.join("node_modules"), BULK_FILES);

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
    assert_eq!(workspace.bases().len(), 1, "and the base it was cloning is untouched");
    assert!(
        homes[0].join("node_modules").is_dir(),
        "which is what the unit that did finish was made from"
    );
}

#[test]
fn a_create_killed_while_its_base_builds_keeps_the_base_and_finishes_it_next_time() {
    let workspace = Workspace::with_bulk();
    let mut child = workspace
        .command(&["new", "worker import"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_a_base_directory(&workspace, &mut child);
    kill(&mut child);
    assert!(workspace.units().is_empty(), "the kill landed before any home was made");

    // What a person does next. The base build is resumed rather than taken back, and
    // the create that wanted it is simply asked for again.
    let second = workspace.command(&["new", "payroll export"]).output().unwrap();
    let told = String::from_utf8(second.stderr.clone()).unwrap();
    assert!(stdout(&second).contains("payroll-export"), "{told}");
    assert!(told.contains("was interrupted and resumed"), "the person is told: {told}");

    let bases = workspace.bases();
    assert_eq!(bases.len(), 1, "one base, finished: {bases:?}");
    assert!(bases[0].join("vendor").join("0.js").is_file(), "and complete");
    let homes = workspace.units();
    assert_eq!(homes.len(), 1, "{homes:?}");
    assert!(homes[0].join("vendor").join("0.js").is_file(), "the unit came from it");
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

/// Wait until the killed run has started assembling a base, and answer once it has.
///
/// The directory being waited for is the one the build assembles in, not the one a
/// base ends up under: the rename between them is what makes a base either whole or
/// absent, so waiting for the finished name would be waiting for the step to be over.
fn wait_for_a_base_directory(workspace: &Workspace, child: &mut Child) {
    let deadline = Instant::now() + REACH_TIMEOUT;
    while Instant::now() < deadline {
        if workspace.base_directory().is_some_and(|path| {
            std::fs::read_dir(path).into_iter().flatten().flatten().next().is_some()
        }) {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the create finished before it could be killed: {status}");
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    panic!("no base directory appeared within {REACH_TIMEOUT:?}");
}

/// `SIGKILL`, so that nothing of the child's own runs afterwards.
fn kill(child: &mut Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn a_unit_carries_the_caches_that_move_and_none_of_the_caches_that_do_not() {
    let workspace = Workspace::with_a_base_that_holds_caches();
    stdout(&workspace.nodal(&["new", "--name", "cache-check"]));
    let home = workspace.units().pop().unwrap();

    for gone in [".next/cache", "apps/web/.next/cache", "apps/web/scripts/__pycache__"] {
        assert!(!home.join(gone).exists(), "{gone} records the path the base was built at");
    }
    // The one at the root was left out of the clone, which is what the table's row for
    // it does. The two under the second package are the relocator's work: an exclusion
    // list is read from the root of the tree and reaches neither.
    for kept in [".next/BUILD_ID", "apps/web/.next/BUILD_ID", "apps/web/.turbo/turbo-build.log"] {
        assert!(home.join(kept).is_file(), "{kept} moves without trouble and is kept warm");
    }
    assert!(home.join("apps/web/src/page.tsx").is_file());
    assert!(home.join("node_modules/react/index.js").is_file(), "the dependencies move");
}

#[test]
fn the_removal_of_a_cache_is_one_line_of_the_unit_own_log() {
    let workspace = Workspace::with_a_base_that_holds_caches();
    stdout(&workspace.nodal(&["new", "--name", "cache-check"]));
    let home = workspace.units().pop().unwrap();
    let base = workspace.bases().pop().unwrap();

    let events = workspace.events();
    assert_eq!(events.len(), 1, "the removal is one line of the unit's log: {events:?}");
    let event = &events[0];
    assert_eq!(event.kind, EventKind::Note);
    assert_eq!(event.epistemic, Epistemic::Observed, "nodal watched itself do it");
    assert!(event.body.contains("apps/web/.next/cache"), "{}", event.body);
    assert!(event.body.contains("apps/web/scripts/__pycache__"), "{}", event.body);
    assert!(event.body.contains(home.to_str().unwrap()), "{}", event.body);
    assert_eq!(reference(event, "removed"), Some("2"), "{:?}", event.refs);
    assert_eq!(reference(event, "relocator"), Some("invalidate"));
    assert_eq!(reference(event, "from"), base.to_str(), "the path the caches were made at");
    assert_eq!(reference(event, "to"), home.to_str());
}

#[test]
fn a_unit_made_from_a_base_with_no_stale_cache_has_nothing_to_report() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "nothing-to-say"]));
    assert!(workspace.events().is_empty(), "a log line saying nothing happened is noise");
}

/// One of an event's named references, as text.
fn reference<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event.refs.get(&name.parse().unwrap()).map(String::as_str)
}
