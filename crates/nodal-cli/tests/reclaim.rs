//! Acceptance test for T1.12: `nodal reclaim` and `nodal gc`, end to end, against a
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

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::lifecycle::journal;
use nodal_core::model::{
    BranchName, EnvId, EnvState, Environment, Ports, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::store::{Store, environments, projects, trash, units};
use tempfile::TempDir;

/// The identifiers the adopted checkout's rows are written with. Fixed rather than
/// generated, so the fixture needs nothing the product does not already depend on.
const ADOPTED_UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";
const ADOPTED_ENV: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA2";

/// How long a test waits for a killed run to reach the step it is being killed in.
const REACH_TIMEOUT: Duration = Duration::from_secs(60);

/// How long it waits for a stopped process to actually go.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// How often either wait looks.
const POLL: Duration = Duration::from_millis(5);

/// A project to make units in, and the state directory they go in.
struct Workspace {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The project's repository.
    source: PathBuf,
    /// Nodal's state directory: the registry, every home, and the trash.
    state: PathBuf,
}

impl Workspace {
    /// A one-commit repository and an empty state directory beside it.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "node_modules/\nbuilt/\n").unwrap();
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

    /// The same, with a recipe this test wrote and this machine has approved.
    ///
    /// Approving is what `nodal init` does, so the fixture runs it rather than writing
    /// the approvals file: an approval a test made by hand would not be evidence that
    /// the command a person runs makes one.
    fn with_recipe(recipe: &str) -> Self {
        let workspace = Self::new();
        workspace.write_recipe(recipe);
        stdout(&workspace.nodal(&["init", "--force"]));
        workspace
    }

    /// Put a recipe in the project, without approving anything.
    fn write_recipe(&self, recipe: &str) {
        std::fs::write(self.source.join("nodal.toml"), recipe).unwrap();
    }

    /// `nodal` with this workspace's state directory, run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The same invocation, not yet run.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nodal"));
        command.args(args).current_dir(&self.source).env("NODAL_HOME", &self.state);
        // Both files are shared by every unit on a machine, and a test must never read
        // or create the ones belonging to whoever is running it.
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command
    }

    /// The registry, opened for reading what a command wrote.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
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

    /// Every live home this project has, in name order.
    fn units(&self) -> Vec<PathBuf> {
        entries(self.segment("e"))
    }

    /// Everything in this project's trash, in name order.
    fn trashed(&self) -> Vec<PathBuf> {
        entries(self.segment("trash"))
    }

    /// The one unit this project has, and its home.
    fn one_unit(&self) -> (String, PathBuf) {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
        let unit = units::list(store.conn(), project.id).unwrap().pop().expect("a unit was made");
        (unit.id.to_string(), self.units().pop().expect("it has a home"))
    }
}

impl Workspace {
    /// Register a checkout of this project as a unit adopted in place.
    ///
    /// `nodal adopt` is a later task, so the rows are written here the way it will
    /// write them: a unit, and an environment whose home is the person's own directory
    /// and whose `managed` is false. The directory is a clone of the project, because
    /// that is what an adopted checkout is — a tree whose commits the project already
    /// has.
    fn adopt_in_place(&self, slug: &str) -> PathBuf {
        let root = self.state.parent().unwrap().join(slug);
        git(&self.source, &["clone", "-q", "--", ".", root.to_str().unwrap()]);
        let store = self.store();
        let project =
            projects::list(store.conn()).unwrap().pop().expect("the project is known by now");
        let now = Timestamp::now();
        let unit = Unit {
            id: UnitId::parse(ADOPTED_UNIT).unwrap(),
            project_id: project.id,
            slug: Slug::parse(slug).unwrap(),
            objective: None,
            branch: BranchName::parse(format!("nodal/{slug}")).unwrap(),
            parent_branch: None,
            status: UnitStatus::Open,
            created_at: now,
            updated_at: now,
        };
        let environment = Environment {
            id: EnvId::parse(ADOPTED_ENV).unwrap(),
            unit_id: unit.id,
            attempt: 1,
            home: root.clone(),
            managed: false,
            base_id: None,
            ws_fp_materialized: None,
            schema_fp_materialized: None,
            host: nodal_core::lifecycle::owner::current_host(),
            db_name: None,
            ports: Ports::default(),
            fixed_port: None,
            state: EnvState::Stopped,
            created_at: now,
            last_active: now,
        };
        units::insert(store.conn(), &unit).unwrap();
        environments::insert(store.conn(), &environment).unwrap();
        nodal_core::lifecycle::marker::write(&root, unit.id).unwrap();
        // Adoption hides what Nodal writes, so that `git status` in a person's own
        // checkout says exactly what it said before.
        nodal_core::env::files::hide(&root.join(".git")).unwrap();
        root
    }
}

/// What is directly inside a directory, in name order, and nothing when there is no
/// such directory yet.
fn entries(directory: Option<PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = directory
        .into_iter()
        .flat_map(|path| std::fs::read_dir(path).into_iter().flatten().flatten())
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// Standard output as text, with the command insisted upon.
fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// Standard error as text.
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// A path with every symbolic link on the way to it resolved, which is what a process
/// asked for its own working directory answers.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `git` in a directory, as text.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
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

/// Insist that the process signal behaved the way this host can behave.
///
/// On Linux Nodal reads `/proc`, so a reclaim must carry no note about the process
/// table. Everywhere else the scan is not implemented, and the reclaim must say so
/// rather than report an empty table as an empty machine
/// (`nodal_core::runtime::processes`). This is asserted rather than skipped, because
/// "the host degrades to a note" is the behaviour, not the absence of behaviour.
fn assert_process_signal(report: &serde_json::Value) {
    let notes = report["notes"].as_array().expect("a report carries its notes");
    let scan = notes.iter().find(|note| note["signal"] == "environment");
    if cfg!(target_os = "linux") {
        assert!(scan.is_none(), "linux reads the process table: {report}");
    } else {
        let note = scan.unwrap_or_else(|| panic!("no note about the process table: {report}"));
        assert!(
            note["why"].as_str().unwrap_or_default().contains("process scan"),
            "the note says which signal went unread: {report}"
        );
    }
}

/// Whether this host can see the processes a unit is running.
fn can_see_processes() -> bool {
    cfg!(target_os = "linux")
}

// ---------------------------------------------------------------------------
// The check, the trash and the verification.
// ---------------------------------------------------------------------------

#[test]
fn a_clean_unit_is_reclaimed_and_nothing_of_it_is_left_but_the_trash_entry() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();

    let report = stdout(&workspace.nodal(&["reclaim", "worker-import"]));
    assert!(report.contains("nothing that is only here"), "{report}");
    assert_nothing_left(&report);

    assert!(!home.exists(), "the home is not where it was");
    assert!(workspace.units().is_empty(), "and no live home is left: {:?}", workspace.units());
    let trashed = workspace.trashed();
    assert_eq!(trashed.len(), 1, "the trash holds it, and only it: {trashed:?}");
    assert_eq!(trashed[0].file_name(), home.file_name(), "under the name it had");
    assert!(trashed[0].join("app").join("main.txt").is_file(), "with its content");

    let store = workspace.store();
    let entry = trash::list(store.conn()).unwrap().pop().expect("the move was recorded");
    assert_eq!(entry.path, trashed[0]);
    assert_eq!(entry.home, home);
    assert!(entry.snapshot.is_none(), "a clean home needed nothing preserved");
}

#[test]
fn a_unit_with_work_that_is_only_there_is_refused_and_told_exactly_what() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();
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
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();
    std::fs::write(home.join("app").join("main.txt"), "committed here\n").unwrap();
    // A home is a clone and carries no identity of its own, as a person's would from
    // their global configuration.
    git(&home, &["config", "user.email", "unit@example.invalid"]);
    git(&home, &["config", "user.name", "Test"]);
    git(&home, &["add", "-A"]);
    git(&home, &["commit", "-qm", "work only this home has"]);

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success());
    let told = stderr(&refused);
    assert!(told.contains("commits on no remote (1)"), "{told}");

    // The commits the home inherited are in the person's own checkout, so they are not
    // work that is only here. Without that, a project with no remote could never have a
    // unit reclaimed at all.
    git(&workspace.source, &["fetch", "-q", home.to_str().unwrap(), "HEAD"]);
    let accepted = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(accepted.status.success(), "{}", stderr(&accepted));
}

#[test]
fn a_forced_reclaim_commits_the_work_before_it_moves_the_home() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (id, home) = workspace.one_unit();
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

#[test]
fn reclaiming_from_inside_the_home_does_not_stop_the_shell_that_asked() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();

    // The command stands in the home it is about, which is where a person runs it from.
    // Its own process is attributed to the unit by the directory it is in, and stopping
    // it would be stopping the reclaim.
    let mut command = workspace.command(&["reclaim", "worker-import", "--json"]);
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&command.current_dir(&home).output().unwrap())).unwrap();

    assert_process_signal(&report);
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    assert!(report["stopped"]["killed"].as_array().unwrap().is_empty(), "{report}");
    let spared = report["stopped"]["spared"].as_array().unwrap().len();
    if can_see_processes() {
        assert_eq!(spared, 1, "the command's own process was left alone: {report}");
    } else {
        // A host that cannot read the table cannot find the caller either, so there is
        // nothing to spare. The property it does have is the one that matters: the
        // command that asked is still running when the reclaim answers.
        assert_eq!(spared, 0, "{report}");
    }
    assert!(!home.exists(), "and the home still went");
}

#[test]
fn a_reclaimed_unit_is_listed_as_archived_with_no_home_and_no_complaint() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    stdout(&workspace.nodal(&["reclaim", "worker-import"]));

    // The unit stays on the list, because the list is the ledger. What must not stay is
    // its home: asking Git about a directory a reclaim moved away on purpose would put
    // a note under every list from now on.
    let listed = stdout(&workspace.nodal(&["ls"]));
    assert!(listed.contains("worker-import"), "{listed}");
    assert!(listed.contains("archived"), "{listed}");
    assert!(!listed.contains("No such file or directory"), "{listed}");
    assert!(!listed.contains("git status"), "the list has nothing to complain about: {listed}");
}

#[test]
fn a_unit_that_has_been_reclaimed_is_not_reclaimed_again() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    stdout(&workspace.nodal(&["reclaim", "worker-import"]));

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("was reclaimed already"), "{}", stderr(&refused));
    assert_eq!(workspace.trashed().len(), 1, "and nothing was moved twice");
}

// ---------------------------------------------------------------------------
// The runtime.
// ---------------------------------------------------------------------------

/// A planted process is stopped where Nodal can read a process table, and reported as
/// unread where it cannot.
///
/// Both halves are asserted. The Linux half is the acceptance criterion: the process
/// goes and its port comes back. The other half is the behaviour a host without `/proc`
/// has to have — the reclaim finishes, the port still comes back, the plant is still
/// running, and the report says the process table went unread instead of implying the
/// machine was empty.
#[test]
fn a_process_planted_in_a_unit_is_stopped_where_nodal_can_see_it() {
    let workspace = Workspace::new();
    let created = json(&workspace.nodal(&["new", "--name", "worker-import", "--json"]));
    let (id, home) = workspace.one_unit();
    let port = created["unit"]["environment"]["ports"]["app"].as_u64().expect("a port was granted");
    let planted = plant(&home, &id, "sleep 300");

    let report = json(&workspace.nodal(&["reclaim", "worker-import", "--json"]));
    assert_process_signal(&report);
    // The port is given back in the transaction that records the reclaim, so it comes
    // back on every host whether or not anything could be stopped.
    assert_eq!(report["released"]["allocated"], serde_json::json!([port]));
    let store = workspace.store();
    assert!(
        nodal_core::store::port_allocations::get(store.conn(), u16::try_from(port).unwrap())
            .unwrap()
            .is_none(),
        "the port is free for the next unit"
    );
    drop(store);

    if can_see_processes() {
        assert_eq!(report["stopped"]["asked"].as_array().unwrap().len(), 1, "{report}");
        assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
        wait_until_gone(planted);
    } else {
        assert!(report["stopped"]["asked"].as_array().unwrap().is_empty(), "{report}");
        assert!(is_running(planted), "a host that cannot see a process cannot stop one");
        assert!(report["leftovers"].as_array().unwrap().is_empty(), "an unread signal is a note");
        end(planted);
    }
    assert_eq!(workspace.trashed().len(), 1, "the home went either way");
}

/// Start a detached process inside a home, carrying that unit's identifier, and answer
/// with its process id.
///
/// Detached on purpose. A process this test started and has not waited for stays in the
/// table as a zombie after it is stopped, and "does this process exist" then answers yes
/// for something that is not running — which would report a stop that worked as a
/// process left behind. Starting it from a shell that then exits hands it to the system,
/// which reaps it properly.
fn plant(home: &Path, unit: &str, command: &str) -> u32 {
    let line = format!("{command} >/dev/null 2>&1 & printf %s \"$!\"");
    let output = Command::new("sh")
        .arg("-c")
        .arg(line)
        .current_dir(home)
        .env("NODAL_ID", unit)
        .output()
        .unwrap();
    let pid: u32 = String::from_utf8(output.stdout).unwrap().trim().parse().unwrap();
    assert!(is_running(pid), "the plant is running");
    pid
}

/// Whether a process is still there. `kill -0` rather than `/proc`, because this file
/// runs on a host that has no `/proc` and still has to answer the question.
fn is_running(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Stop a plant the test itself has to clean up, on a host that could not stop it.
fn end(pid: u32) {
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

/// Wait for a stopped process to leave the table, and insist that it does.
fn wait_until_gone(pid: u32) {
    let deadline = Instant::now() + STOP_TIMEOUT;
    while Instant::now() < deadline {
        if !is_running(pid) {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("process {pid} was still running {STOP_TIMEOUT:?} after the reclaim");
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
    let workspace = Workspace::with_recipe(HOOKS);
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();
    // Resolved now, while the directory is still there. A path that has been moved
    // cannot be resolved, and asking afterwards would quietly answer with the
    // unresolved name and compare it against the resolved one the shell reported.
    let stood_in = resolved(&home);
    stdout(&workspace.nodal(&["reclaim", "worker-import"]));

    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    let phases: Vec<&str> = log.lines().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(phases, ["pre_new", "post_new", "pre_reclaim", "post_reclaim"], "{log}");
    assert!(log.contains("pre_new worker-import"), "{log}");
    assert!(log.contains(&format!("post_new {}", home.display())), "{log}");
    // `$PWD` is what the shell got from the kernel, so it is the resolved path. On a
    // host whose temporary directory is reached through a symbolic link — macOS reaches
    // `/var` through `/private/var` — that is not the text the registry holds, and the
    // two are the same directory.
    assert!(
        log.contains(&format!("pre_reclaim {}", stood_in.display())),
        "pre_reclaim runs in the home it is about: {log}"
    );
    let trashed = workspace.trashed().pop().unwrap();
    assert!(
        log.contains(&format!("post_reclaim {}", trashed.display())),
        "post_reclaim is told where the home went: {log}"
    );
}

#[test]
fn a_hook_command_nobody_approved_refuses_to_run() {
    let workspace = Workspace::with_recipe(HOOKS);
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    // The recipe changes after it was approved, which is what arrives with a pull.
    workspace.write_recipe(&HOOKS.replace("pre_reclaim %s", "SOMETHING ELSE %s"));

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success(), "an unapproved command does not run");
    let told = stderr(&refused);
    assert!(told.contains("pre_reclaim hook"), "{told}");
    assert!(told.contains("is not approved"), "{told}");
    assert!(told.contains("SOMETHING ELSE"), "the message shows what it would have run: {told}");

    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    assert!(!log.contains("SOMETHING ELSE"), "and it did not run: {log}");
    assert!(workspace.trashed().is_empty(), "the refusal happened before anything moved");

    // Approving is what `nodal init` does, and the same reclaim then works.
    stdout(&workspace.nodal(&["init", "--force"]));
    stdout(&workspace.nodal(&["reclaim", "worker-import"]));
    let log = std::fs::read_to_string(workspace.source.join("hooks.log")).unwrap();
    assert!(log.contains("SOMETHING ELSE"), "{log}");
}

#[test]
fn a_hook_that_fails_stops_the_reclaim_before_anything_moves() {
    let workspace = Workspace::with_recipe("[hooks]\npre_reclaim = \"exit 3\"\n");
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();

    let refused = workspace.nodal(&["reclaim", "worker-import"]);
    assert!(!refused.status.success(), "a hook that fails is a reclaim that does not happen");
    let told = stderr(&refused);
    assert!(told.contains("pre_reclaim hook failed"), "{told}");
    assert!(told.contains("exited 3"), "the message says how: {told}");
    assert!(home.is_dir(), "and the home is where it was");
    assert!(workspace.trashed().is_empty());
}

#[test]
fn no_hooks_runs_none_of_them_without_needing_an_approval() {
    let workspace = Workspace::new();
    workspace.write_recipe(HOOKS);
    stdout(&workspace.nodal(&["--no-hooks", "new", "--name", "worker-import"]));
    stdout(&workspace.nodal(&["--no-hooks", "reclaim", "worker-import"]));
    assert!(!workspace.source.join("hooks.log").exists(), "no hook ran");
    assert_eq!(workspace.trashed().len(), 1, "and the unit was still reclaimed");
}

// ---------------------------------------------------------------------------
// The sweep.
// ---------------------------------------------------------------------------

#[test]
fn gc_removes_a_trashed_home_once_its_retention_has_run_out_and_not_before() {
    let workspace = Workspace::with_recipe("[reclaim]\ntrash_retention = 14\n");
    stdout(&workspace.nodal(&["new", "--name", "kept"]));
    stdout(&workspace.nodal(&["reclaim", "kept"]));
    let kept = workspace.trashed().pop().expect("the home is in the trash");

    let held = stdout(&workspace.nodal(&["gc"]));
    assert!(held.contains("0 homes"), "nothing goes before its retention is up: {held}");
    assert!(kept.is_dir(), "and the directory is still there");

    // A project that keeps nothing: the same reclaim, the same sweep, the other answer.
    workspace.write_recipe("[reclaim]\ntrash_retention = 0\n");
    stdout(&workspace.nodal(&["init", "--force"]));
    stdout(&workspace.nodal(&["new", "--name", "swept"]));
    stdout(&workspace.nodal(&["reclaim", "swept"]));
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
/// window. Only a host that can read a process table can find that plant, so only there
/// is the kill part of this test. Elsewhere the same fixture asserts what that host does
/// instead — it cannot see the plant, so it stops nothing, says so in a note, and
/// reclaims the home anyway.
#[test]
fn a_reclaim_killed_between_two_steps_is_rolled_back_by_the_next_invocation() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (id, home) = workspace.one_unit();
    let planted = plant(&home, &id, "trap '' TERM; sleep 300");

    if !can_see_processes() {
        let report = json(&workspace.nodal(&["reclaim", "worker-import", "--json"]));
        assert_process_signal(&report);
        assert!(report["stopped"]["asked"].as_array().unwrap().is_empty(), "{report}");
        assert!(is_running(planted), "nothing this host cannot see was signalled");
        assert!(!home.exists(), "and the home was still reclaimed");
        end(planted);
        return;
    }

    let mut child = workspace
        .command(&["reclaim", "worker-import"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for_the_reclaim_to_start(&workspace, &mut child);
    child.kill().unwrap();
    child.wait().unwrap();

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
    wait_until_gone(planted);
}

/// Wait until the reclaim has journalled itself and is inside its first step.
fn wait_for_the_reclaim_to_start(workspace: &Workspace, child: &mut Child) {
    let deadline = Instant::now() + REACH_TIMEOUT;
    while Instant::now() < deadline {
        let store = workspace.store();
        let running = journal::unfinished(store.conn()).unwrap();
        if running.iter().any(|record| record.kind == "reclaim") {
            return;
        }
        drop(store);
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the reclaim finished before it could be killed: {status}");
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    panic!("no reclaim was journalled within {REACH_TIMEOUT:?}");
}

// ---------------------------------------------------------------------------
// The directory Nodal did not make.
// ---------------------------------------------------------------------------

#[test]
fn a_checkout_adopted_in_place_is_unregistered_and_never_trashed() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let root = workspace.adopt_in_place("in-place");

    let report = stdout(&workspace.nodal(&["reclaim", "in-place"]));
    assert!(report.contains("left in place"), "{report}");
    assert!(report.contains(root.to_str().unwrap()), "{report}");
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
    assert_eq!(workspace.units().len(), 1, "{:?}", workspace.units());
}

#[test]
fn a_home_somebody_deleted_by_hand_still_closes_its_rows() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["new", "--name", "worker-import"]));
    let (_, home) = workspace.one_unit();
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
