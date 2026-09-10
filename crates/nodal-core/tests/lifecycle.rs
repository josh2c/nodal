//! Acceptance test for the operation framework: an operation killed between two steps
//! is reported and resolved by the next invocation, and nothing it made is left behind.
//!
//! The kill has to be a real one. A test that drops a value or returns an error is
//! testing the error path, not the interruption: what makes an interruption different
//! is that no code of ours runs afterwards, so everything the next invocation knows has
//! to have been written down before the process died. This file therefore runs the
//! operation in a second process — this same test binary, re-executed with the child
//! test's name — waits until it is parked between two steps, sends it `SIGKILL`, and
//! then does what the next `nodal` would do.
//!
//! The operation is a stand-in for `nodal new`, which is not built yet: it
//! creates a home directory, writes the marker file into it, and registers a service —
//! a file under a shared directory, standing in for the container a real run would
//! start, so that "nothing left in services" is something this test can actually check.
//! Its registry write is a project row and a unit row, made at the end in one
//! transaction.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nodal_core::lifecycle::journal::{self, State, StepRecord, StepState};
use nodal_core::lifecycle::owner::{Liveness, Owner};
use nodal_core::lifecycle::{
    Action, Output, Outputs, Plan, Rebuild, Recovery, Resolution, Step, nothing, run,
};
use nodal_core::model::{
    Digest, HostName, Objective, OperationId, Project, ProjectName, Slug, Timestamp, Unit, UnitId,
    UnitStatus,
};
use nodal_core::store::{Store, projects, units};
use nodal_core::{Error, Result, lifecycle};
use serde_json::json;
use tempfile::TempDir;

/// The environment variable the child reads its working directory from.
const ROOT_VAR: &str = "NODAL_T011_ROOT";

/// The name of the child test, as the test harness filters on it.
const CHILD_TEST: &str = "child_runs_the_operation_and_parks_between_two_steps";

/// How long the parent waits for the child to reach the park.
const PARK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often it looks.
const POLL: Duration = Duration::from_millis(20);

// ---------------------------------------------------------------------------
// The operation under test.
// ---------------------------------------------------------------------------

/// What the fixture operation is called in the journal.
const KIND: &str = "fixture-new";

/// The steps, in order, so that a test can name one without repeating a string.
const CREATE_HOME: &str = "create-home";
const WRITE_MARKER: &str = "write-marker";
const START_SERVICE: &str = "start-service";
const PARK: &str = "park";

/// What the service step reports, and what the registry write is expected to record.
/// A resumed run must record this too, having never started the service itself.
const SERVICE_PORT: &str = "20017";

/// Everything the fixture operation was built from. This is the plan's `params`: what
/// the journal keeps, and the only thing a later process has to rebuild the plan from.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Params {
    /// The directory the operation creates.
    home: PathBuf,
    /// The file that stands in for a running service.
    service: PathBuf,
    /// The unit the registry write records.
    unit: UnitId,
    /// Its project.
    project: PathBuf,
    /// Whether the plan ends in a step that never returns, for the kill test.
    park: bool,
    /// Whether the last real step fails, for the failure test.
    fail: bool,
    /// What an interrupted run of this plan should do.
    resume: bool,
}

/// Create a directory, and remove it again with whatever it holds.
struct CreateHome(PathBuf);

impl Step for CreateHome {
    fn key(&self) -> String {
        CREATE_HOME.to_owned()
    }

    fn apply(&self) -> Result<Output> {
        std::fs::create_dir_all(&self.0).map_err(Error::io(&self.0))?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        remove_dir(&self.0)
    }
}

/// Write the marker file that says whose home this is.
struct WriteMarker {
    home: PathBuf,
    unit: UnitId,
}

impl WriteMarker {
    fn path(&self) -> PathBuf {
        self.home.join(".nodal").join("id")
    }
}

impl Step for WriteMarker {
    fn key(&self) -> String {
        WRITE_MARKER.to_owned()
    }

    fn apply(&self) -> Result<Output> {
        let path = self.path();
        let parent = path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&parent).map_err(Error::io(&parent))?;
        std::fs::write(&path, self.unit.to_string()).map_err(Error::io(&path))?;
        Ok(nothing())
    }

    fn undo(&self) -> Result<()> {
        remove_file(&self.path())
    }
}

/// Register a service. A file, so that a test can say it is gone.
struct StartService(PathBuf);

impl Step for StartService {
    fn key(&self) -> String {
        START_SERVICE.to_owned()
    }

    /// Produces a value the registry write needs and no rebuild of the plan can work
    /// out for itself: which port the service came up on. The number is read from the
    /// file when there is one, so a second apply answers what the first did.
    fn apply(&self) -> Result<Output> {
        if let Some(parent) = self.0.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        let port = match std::fs::read_to_string(&self.0) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let port = SERVICE_PORT.to_string();
                std::fs::write(&self.0, &port).map_err(Error::io(&self.0))?;
                port
            }
            Err(error) => return Err(Error::io(&self.0)(error)),
        };
        Ok(json!({ "port": port }))
    }

    fn undo(&self) -> Result<()> {
        remove_file(&self.0)
    }
}

/// What [`StartService`] tells the registry write.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Started {
    /// The port the service came up on.
    port: String,
}

/// A step that fails, so the failure path can be tested without a broken machine.
struct Explode;

impl Step for Explode {
    fn key(&self) -> String {
        "explode".to_owned()
    }

    fn apply(&self) -> Result<Output> {
        Err(Error::InvalidValue { kind: "fixture step", value: String::from("boom") })
    }

    fn undo(&self) -> Result<()> {
        Ok(())
    }
}

/// A step that says it has arrived and then never returns, so the parent can kill the
/// child at a point of its choosing. It changes nothing, so the world at that moment is
/// exactly the world between the step before it and the step after it.
struct Park(PathBuf);

impl Step for Park {
    fn key(&self) -> String {
        PARK.to_owned()
    }

    fn apply(&self) -> Result<Output> {
        std::fs::write(&self.0, "parked").map_err(Error::io(&self.0))?;
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    fn undo(&self) -> Result<()> {
        remove_file(&self.0)
    }
}

/// Removing something that is not there is not a failure: undo is idempotent, and it
/// runs against a world that may never have been changed.
fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// As [`remove_file`], for a directory and everything under it.
fn remove_dir(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// Build the fixture operation's plan. Pure, and the same in both processes: this is
/// what makes a run started by one of them resolvable by the other.
fn plan(params: &Params) -> Plan {
    let (unit, project_root) = (params.unit, params.project.clone());
    // The registry write records what the service step found, which is the whole point:
    // the process that resolves an interrupted run never ran that step and has to read
    // the answer out of the journal.
    let commit =
        Box::new(move |tx: &rusqlite::Transaction<'_>, outputs: &Outputs| -> Result<Output> {
            let project = fixture_project(&project_root);
            projects::insert(tx, &project)?;
            let started: Option<Started> = outputs.read(START_SERVICE)?;
            let objective = started.map(|started| Objective::parse(&started.port).unwrap());
            units::insert(tx, &Unit { objective, ..fixture_unit(unit, project.id) })?;
            Ok(json!({ "wrote": "unit" }))
        });
    let mut plan = Plan::new(KIND, String::from("fix-worker-import"), json!(params), commit)
        .then(CreateHome(params.home.clone()))
        .then(WriteMarker { home: params.home.clone(), unit: params.unit })
        .then(StartService(params.service.clone()));
    if params.fail {
        plan = plan.then(Explode);
    }
    if params.park {
        plan = plan.then(Park(park_marker(&params.home)));
    }
    if params.resume {
        plan = plan.recovering(Recovery::Resume);
    }
    plan
}

/// The file the parked child writes to say it has arrived. It sits beside the home
/// rather than inside it, so that removing the home does not remove the evidence.
fn park_marker(home: &Path) -> PathBuf {
    let mut marker = home.as_os_str().to_owned();
    marker.push(".parked");
    PathBuf::from(marker)
}

/// The table [`lifecycle::resolve`] is given: one entry per operation this build knows.
struct FixtureNew;

impl Rebuild for FixtureNew {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn rebuild(&self, record: &journal::Operation) -> Result<Plan> {
        let params: Params = serde_json::from_value(record.params.clone()).map_err(|_| {
            Error::InvalidValue { kind: "fixture params", value: record.kind.clone() }
        })?;
        Ok(plan(&params))
    }
}

/// Fixed values for the registry rows, so a test can look for them by name.
fn fixture_project(root: &Path) -> Project {
    Project {
        id: id('P'),
        root: root.to_path_buf(),
        name: ProjectName::parse("fixture").unwrap(),
        recipe_hash: Digest::parse("abc123").unwrap(),
        created_at: at(),
        remote_url: None,
    }
}

fn fixture_unit(id: UnitId, project_id: nodal_core::model::ProjectId) -> Unit {
    Unit {
        id,
        project_id,
        slug: Slug::parse("fix-worker-import").unwrap(),
        objective: None,
        objective_epistemic: None,
        branch: "nodal/fix-worker-import".parse().unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: at(),
        updated_at: at(),
    }
}

fn at() -> Timestamp {
    Timestamp::parse("2026-09-06T10:11:12Z").unwrap()
}

fn id<T: std::str::FromStr<Err = Error>>(last: char) -> T {
    format!("01J8Z6H000000000000000000{last}").parse().unwrap()
}

// ---------------------------------------------------------------------------
// A workspace both processes agree on.
// ---------------------------------------------------------------------------

/// The paths the fixture operation uses, derived from one root so that the child can be
/// told where to work with a single environment variable.
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn registry(&self) -> PathBuf {
        self.root.join("registry.db")
    }

    fn home(&self) -> PathBuf {
        self.root.join("homes").join("e-01")
    }

    /// Where a real run would have a container. Nothing else writes here, so what is
    /// under it after a rollback is the whole of "left in services".
    fn services(&self) -> PathBuf {
        self.root.join("services")
    }

    fn service(&self) -> PathBuf {
        self.services().join("db-01")
    }

    fn params(&self) -> Params {
        Params {
            home: self.home(),
            service: self.service(),
            unit: id('V'),
            project: self.root.join("project"),
            park: false,
            fail: false,
            resume: false,
        }
    }

    fn store(&self) -> Store {
        Store::open(self.registry()).unwrap()
    }

    /// Whether the operation left anything at all: a home on disk, a service, or a row.
    fn leftovers(&self) -> Vec<String> {
        let mut found = Vec::new();
        if self.home().exists() {
            found.push(format!("home {}", self.home().display()));
        }
        for entry in std::fs::read_dir(self.services()).into_iter().flatten().flatten() {
            found.push(format!("service {}", entry.path().display()));
        }
        let store = self.store();
        let project = fixture_project(&self.root.join("project"));
        if projects::get(store.conn(), project.id).unwrap().is_some() {
            found.push(String::from("project row"));
        }
        if units::get(store.conn(), id::<UnitId>('V')).unwrap().is_some() {
            found.push(String::from("unit row"));
        }
        found
    }
}

/// A temporary root that is removed when the test ends.
fn workspace() -> (TempDir, Workspace) {
    let dir = TempDir::new().unwrap();
    let workspace = Workspace::at(dir.path());
    std::fs::create_dir_all(workspace.services()).unwrap();
    (dir, workspace)
}

/// Resolve as the next `nodal` would, with the one operation this build knows.
fn next_invocation(workspace: &Workspace) -> Vec<Resolution> {
    let mut store = workspace.store();
    lifecycle::resolve(&mut store, &[&FixtureNew]).unwrap()
}

// ---------------------------------------------------------------------------
// The acceptance test.
// ---------------------------------------------------------------------------

/// The child half: run the operation for real, and park in the gap between the service
/// step and the end. Ignored, so it only runs when the parent asks for it by name.
#[test]
#[ignore = "run by the parent test, in a process it kills"]
fn child_runs_the_operation_and_parks_between_two_steps() {
    let root = std::env::var(ROOT_VAR).expect("the parent sets the root");
    let workspace = Workspace::at(root);
    let params = Params { park: true, ..workspace.params() };
    let mut store = workspace.store();
    // Never returns: the parent kills this process while the park step is on the stack.
    let _ = run(&mut store, &plan(&params));
    unreachable!("the parent kills the child before the plan can finish");
}

#[test]
fn a_killed_operation_is_reported_and_rolled_back_by_the_next_invocation() {
    let (_dir, workspace) = workspace();
    let mut child = spawn_child(&workspace);
    wait_for_park(&workspace, &mut child);
    kill(&mut child);

    assert_the_killed_run_left_its_work(&workspace);

    let reported = next_invocation(&workspace);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_every_step_was_undone_in_reverse(&reported[0]);

    // Nothing on disk, nothing in services, nothing in the registry.
    assert_eq!(workspace.leftovers(), Vec::<String>::new());

    // And it is resolved once, not reported again every time.
    assert_eq!(next_invocation(&workspace), Vec::new());
    assert_the_journal_closed_the_run(&workspace);
}

/// What a run killed between two steps leaves: everything up to the kill, and no
/// registry rows, because the registry write is the last thing an operation does.
fn assert_the_killed_run_left_its_work(workspace: &Workspace) {
    assert!(workspace.home().join(".nodal").join("id").exists(), "the marker was written");
    assert!(workspace.service().exists(), "the service was started");
    assert!(
        units::get(workspace.store().conn(), id::<UnitId>('V')).unwrap().is_none(),
        "the registry write is the last step and never ran"
    );
}

fn assert_every_step_was_undone_in_reverse(resolution: &Resolution) {
    assert_eq!(resolution.kind, KIND);
    assert_eq!(
        resolution.action,
        Action::RolledBack {
            undone: vec![
                PARK.to_owned(),
                START_SERVICE.to_owned(),
                WRITE_MARKER.to_owned(),
                CREATE_HOME.to_owned(),
            ]
        },
        "every step the journal saw is undone, in reverse"
    );
    assert!(
        resolution.to_string().contains("was interrupted and rolled back"),
        "the report says what happened: {resolution}"
    );
}

fn assert_the_journal_closed_the_run(workspace: &Workspace) {
    let store = workspace.store();
    let done = journal::recent(store.conn(), 10).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].state, State::RolledBack);
    assert!(done[0].ended_at.is_some());
}

/// Re-execute this test binary, running only the child test.
fn spawn_child(workspace: &Workspace) -> Child {
    let binary = std::env::current_exe().expect("a test binary has a path");
    Command::new(binary)
        .args([CHILD_TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(ROOT_VAR, &workspace.root)
        .spawn()
        .expect("the test binary can be run again")
}

/// Wait until the child says it has reached the park.
fn wait_for_park(workspace: &Workspace, child: &mut Child) {
    let marker = park_marker(&workspace.home());
    let deadline = Instant::now() + PARK_TIMEOUT;
    while Instant::now() < deadline {
        if marker.exists() {
            return;
        }
        if let Some(status) = child.try_wait().expect("the child can be polled") {
            panic!("the child exited before it parked: {status}");
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    panic!("the child did not reach the park within {PARK_TIMEOUT:?}");
}

/// `SIGKILL`, so that nothing of the child's runs afterwards. Anything the next
/// invocation knows, the child had already written down.
fn kill(child: &mut Child) {
    child.kill().expect("the child can be killed");
    child.wait().expect("the child can be reaped");
}

// ---------------------------------------------------------------------------
// The framework, without a second process.
// ---------------------------------------------------------------------------

#[test]
fn a_plan_that_finishes_writes_the_registry_once_and_leaves_no_work_to_resolve() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    let mut store = workspace.store();
    let done = run(&mut store, &plan(&params)).unwrap();
    let id = done.id;

    assert!(workspace.home().exists());
    assert!(workspace.service().exists());
    assert!(units::get(store.conn(), params.unit).unwrap().is_some(), "the registry write ran");

    let record = journal::get(store.conn(), id).unwrap().expect("the run is journalled");
    assert_eq!(record.state, State::Committed);
    let steps = journal::steps(store.conn(), id).unwrap();
    assert_eq!(
        steps.iter().map(|step| step.key.as_str()).collect::<Vec<_>>(),
        [CREATE_HOME, WRITE_MARKER, START_SERVICE]
    );
    assert!(steps.iter().all(|step| step.state == StepState::Applied));
    assert_eq!(recorded_port(&store, params.unit), Some(String::from(SERVICE_PORT)));
    assert_eq!(next_invocation(&workspace), Vec::new());
}

/// What a step produced outlives the process that produced it.
///
/// The registry write of this plan records the port the service step reported, and the
/// service step is the one an interrupted run here has already applied. So the process
/// that finishes the run never starts the service, has nothing in memory to write, and
/// must read what the first process found out of the journal. Without that column it
/// writes a unit row a first run would not have written, which is the divergence the
/// step-output change removes.
#[test]
fn a_resumed_run_commits_what_the_steps_of_the_first_run_produced() {
    let (_dir, workspace) = workspace();
    let params = Params { resume: true, ..workspace.params() };
    let id = interrupt_after_every_step(&workspace, &params);

    let reported = next_invocation(&workspace);
    assert_eq!(
        reported.first().map(|resolution| &resolution.action),
        Some(&Action::Resumed { applied: Vec::new() }),
        "every step was already applied, so the take-over is the registry write alone: \
         {reported:?}"
    );

    let store = workspace.store();
    assert_eq!(journal::get(store.conn(), id).unwrap().unwrap().state, State::Committed);
    assert_eq!(
        recorded_port(&store, params.unit),
        Some(String::from(SERVICE_PORT)),
        "the write of the run that was finished says what the run that was killed found"
    );
}

/// The port the registry write recorded for a unit, which is what the service step
/// reported to it.
fn recorded_port(store: &Store, unit: UnitId) -> Option<String> {
    units::get(store.conn(), unit)
        .unwrap()
        .expect("the registry write ran")
        .objective
        .map(|objective| objective.to_string())
}

#[test]
fn a_step_that_fails_undoes_the_steps_before_it_and_writes_nothing() {
    let (_dir, workspace) = workspace();
    let params = Params { fail: true, ..workspace.params() };
    let mut store = workspace.store();
    let error = run(&mut store, &plan(&params)).unwrap_err();

    assert!(matches!(&error, Error::OperationStep { key, .. } if key == "explode"), "{error}");
    assert_eq!(workspace.leftovers(), Vec::<String>::new());
    let recent = journal::recent(store.conn(), 10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].state, State::RolledBack);
    assert_eq!(next_invocation(&workspace), Vec::new(), "nothing is left for the next command");
}

#[test]
fn an_interrupted_plan_that_asks_to_resume_is_finished_rather_than_undone() {
    let (_dir, workspace) = workspace();
    let params = Params { resume: true, ..workspace.params() };
    let id = interrupt_after_first_step(&workspace, &params);

    let reported = next_invocation(&workspace);
    assert_eq!(
        reported.first().map(|resolution| &resolution.action),
        Some(&Action::Resumed { applied: vec![WRITE_MARKER.to_owned(), START_SERVICE.to_owned()] }),
        "{reported:?}"
    );
    let store = workspace.store();
    assert_eq!(journal::get(store.conn(), id).unwrap().unwrap().state, State::Committed);
    assert!(units::get(store.conn(), params.unit).unwrap().is_some(), "the registry write ran");
    assert!(workspace.service().exists());
}

#[test]
fn a_step_interrupted_part_way_through_is_undone_like_one_that_finished() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    journal::start(store.conn(), id, &plan(&params), &gone, at()).unwrap();

    // The world as a process killed inside `create-home` would have left it: the
    // directory is there, and the journal says only that the step was being attempted.
    std::fs::create_dir_all(workspace.home()).unwrap();
    let record = StepRecord {
        position: 0,
        key: CREATE_HOME.to_owned(),
        state: StepState::Applying,
        output: None,
        updated_at: at(),
    };
    journal::mark_step(store.conn(), id, &record).unwrap();
    drop(store);

    let reported = next_invocation(&workspace);
    assert_eq!(
        reported.first().map(|resolution| &resolution.action),
        Some(&Action::RolledBack { undone: vec![CREATE_HOME.to_owned()] }),
        "{reported:?}"
    );
    assert_eq!(workspace.leftovers(), Vec::<String>::new());
}

#[test]
fn an_operation_belonging_to_another_machine_is_reported_and_left_alone() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let elsewhere = Owner { host: HostName::parse("some-other-machine").unwrap(), pid: dead_pid() };
    journal::start(store.conn(), id, &plan(&params), &elsewhere, at()).unwrap();
    drop(store);

    let reported = next_invocation(&workspace);
    assert_eq!(reported.first().map(|resolution| &resolution.action), Some(&Action::Elsewhere));
    let store = workspace.store();
    assert_eq!(
        journal::get(store.conn(), id).unwrap().unwrap().state,
        State::Running,
        "another host's run is not touched, and is reported again next time"
    );
}

#[test]
fn an_operation_this_build_does_not_know_is_reported_rather_than_guessed_at() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let mut unknown = plan(&params);
    unknown.kind = "from-a-later-version";
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    journal::start(store.conn(), id, &unknown, &gone, at()).unwrap();
    drop(store);

    let reported = next_invocation(&workspace);
    assert_eq!(reported.first().map(|resolution| &resolution.action), Some(&Action::Unknown));
}

#[test]
fn an_operation_whose_record_cannot_be_read_is_reported_rather_than_half_undone() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let mut unreadable = plan(&params);
    unreadable.params = json!({ "written_by": "a later version" });
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    journal::start(store.conn(), id, &unreadable, &gone, at()).unwrap();
    drop(store);

    let reported = next_invocation(&workspace);
    assert!(
        matches!(
            reported.first().map(|resolution| &resolution.action),
            Some(Action::Unresolved { .. })
        ),
        "{reported:?}"
    );
    let store = workspace.store();
    assert_eq!(
        journal::get(store.conn(), id).unwrap().unwrap().state,
        State::Running,
        "a run whose plan cannot be rebuilt is left alone, not partly undone"
    );
}

#[test]
fn applying_a_plans_steps_twice_changes_nothing() {
    let (_dir, workspace) = workspace();
    let params = workspace.params();
    for step in &plan(&params).steps {
        step.apply().unwrap();
        step.apply().unwrap();
    }
    assert!(workspace.service().exists());
    for step in plan(&params).steps.iter().rev() {
        step.undo().unwrap();
        step.undo().unwrap();
    }
    assert!(!workspace.home().exists());
    assert!(!workspace.service().exists());
}

/// Journal a run as started, apply every step of it, and interrupt it in the gap
/// before its registry write. The world a process killed there leaves.
fn interrupt_after_every_step(workspace: &Workspace, params: &Params) -> OperationId {
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    let plan = plan(params);
    journal::start(store.conn(), id, &plan, &gone, at()).unwrap();
    for (position, step) in plan.steps.iter().enumerate() {
        let output = step.apply().unwrap();
        let record = StepRecord {
            position: u32::try_from(position).unwrap(),
            key: step.key(),
            state: StepState::Applied,
            output: Some(output).filter(|value| !value.is_null()),
            updated_at: at(),
        };
        journal::mark_step(store.conn(), id, &record).unwrap();
    }
    id
}

/// Journal a run as started, then interrupt it after its first step.
fn interrupt_after_first_step(workspace: &Workspace, params: &Params) -> OperationId {
    let store = workspace.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    let plan = plan(params);
    journal::start(store.conn(), id, &plan, &gone, at()).unwrap();
    plan.steps[0].apply().unwrap();
    let record = StepRecord {
        position: 0,
        key: plan.steps[0].key(),
        state: StepState::Applied,
        output: None,
        updated_at: at(),
    };
    journal::mark_step(store.conn(), id, &record).unwrap();
    id
}

/// A process identifier that is certainly not running on this host.
///
/// A process is started and reaped, and the number it had is then checked rather than
/// assumed: identifiers are reused, and these tests spawn processes of their own.
fn dead_pid() -> u32 {
    let here = Owner::current();
    for _ in 0..16 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the test binary can be run again");
        let pid = child.id();
        child.wait().expect("the child can be reaped");
        let candidate = Owner { host: here.host.clone(), pid };
        if candidate.state(&here) == Liveness::Gone {
            return pid;
        }
    }
    panic!("no process identifier stayed free long enough to use");
}
