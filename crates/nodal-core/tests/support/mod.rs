//! The world the behaviour locks are run against, and the expected-failure gate.
//!
//! One [`World`] is one machine: a checkout, a base built from it, a state directory
//! and the registry inside it. Every identifier the operations use is fixed here rather
//! than generated, because a plan built twice has to be the same plan, and because two
//! runs of the same operation against two worlds have to produce rows a test can
//! compare.
//!
//! [`Snapshot`] is what is compared. It holds the registry rows and the events an
//! operation wrote, rendered with everything that cannot be equal between two runs
//! taken out: the clock readings, the identifiers the runtime mints, and the temporary
//! directory each world sits in.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    reason = "tests fail by panicking, and each test file uses part of this module"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

use nodal_core::git::Oid;
use nodal_core::lifecycle::journal::{self, StepRecord, StepState};
use nodal_core::lifecycle::ops::{adopt, merge, new, reclaim};
use nodal_core::lifecycle::owner::{Liveness, Owner};
use nodal_core::lifecycle::{self, Action, Plan, Recovery};
use nodal_core::model::{
    Actor, ActorKind, Base, BranchName, Digest, EnvState, Environment, OperationId, Platform,
    PortBlock, Ports, Project, ProjectName, Recipe, Session, Slug, Timestamp, Trashed, Unit,
    UnitStatus, WorkspaceFp,
};
use nodal_core::runtime::stop::{Stopped, Target};
use nodal_core::store::{Store, bases, environments, events, projects, sessions, trash, units};
use nodal_core::workspace::tracked;
use nodal_core::{Error, lifecycle::ops};
use tempfile::TempDir;

/// The environment variable that turns the two reproductions into enforced tests.
const ENFORCE: &str = "NODAL_ENFORCE_STEP_OUTPUTS";

/// The instant every fixture row is stamped with.
const AT: &str = "2026-09-07T09:00:00Z";

/// The cache directory a create's relocation sweep removes, which is what makes the
/// relocation event exist at all.
///
/// It is nested under a package rather than at the root of the tree, because the
/// exclusion list is anchored at the root and would drop a cache there before the clone
/// carried it. A cache under a package is exactly what the sweep exists for.
const CACHE: &str = "packages/web/__pycache__";

// ---------------------------------------------------------------------------
// The expected-failure gate.
// ---------------------------------------------------------------------------

/// Assert a divergence that a named change will remove.
///
/// This is an expected failure rather than a skipped test. The comparison is made in
/// full on every run, and the outcome is read two ways:
///
/// - `finished` and `resumed` still differ: the reproduction is printed and the test
///   passes. A known bug does not turn the suite red and hide a real regression.
/// - they are equal: the test **fails**, because the change has landed and the gate has
///   to go, or the property will be unprotected from the next commit onwards.
///
/// Setting `NODAL_ENFORCE_STEP_OUTPUTS=1` swaps the two, which is what CI does the day
/// the change lands.
pub fn expected_failure(what: &str, fixed_by: &str, finished: &Snapshot, resumed: &Snapshot) {
    let enforced = std::env::var(ENFORCE).is_ok_and(|value| value == "1");
    let equal = finished == resumed;
    match (enforced, equal) {
        (true, true) | (false, false) => {}
        (true, false) => {
            panic!(
                "{ENFORCE}=1 is set, and the two runs still differ.\n  \
                 expected failure: {what}\n  fixed by: {fixed_by}\n{}",
                difference(finished, resumed)
            );
        }
        (false, true) => {
            panic!(
                "this expected failure now passes on its own, so its gate has to go.\n  \
                 was: {what}\n  fixed by: {fixed_by}\n  \
                 replace the `expected_failure` call with `assert_eq!(finished, resumed)`, \
                 and drop {ENFORCE} from ci/acceptance-behaviour-lock.sh."
            );
        }
    }
    if !enforced {
        eprintln!(
            "expected failure, still reproducing: {what}\n  fixed by: {fixed_by}\n{}",
            difference(finished, resumed)
        );
    }
}

/// The first field of two snapshots that is not equal, rendered for a report.
fn difference(finished: &Snapshot, resumed: &Snapshot) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (name, left, right) in [
        ("units", &finished.units, &resumed.units),
        ("environments", &finished.environments, &resumed.environments),
        ("sessions", &finished.sessions, &resumed.sessions),
        ("trash", &finished.trash, &resumed.trash),
        ("events", &finished.events, &resumed.events),
    ] {
        if left != right {
            let _ = writeln!(
                out,
                "  {name}:\n    run to completion: {left:?}\n    killed and resumed: {right:?}"
            );
        }
    }
    if out.is_empty() {
        out.push_str("  (no difference)\n");
    }
    out
}

// ---------------------------------------------------------------------------
// What is compared.
// ---------------------------------------------------------------------------

/// The registry rows and events one run of an operation left behind.
///
/// Every field is rendered text rather than the row itself, because two runs cannot
/// share a clock reading or a minted identifier and those differences say nothing about
/// the operation. What is left is what the operation decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// One line per unit row: its handle, branch and status.
    pub units: Vec<String>,
    /// One line per environment row: its state, whether it is managed, and its ports.
    pub environments: Vec<String>,
    /// One line per session row of the unit's environment, and whether it is open.
    pub sessions: Vec<String>,
    /// One line per trash entry.
    pub trash: Vec<String>,
    /// One line per event: its kind and its body, with the references it carries.
    pub events: Vec<String>,
}

impl Snapshot {
    /// Read everything the operation on `unit` wrote.
    fn take(store: &Store, world: &World) -> Self {
        let conn = store.conn();
        let unit = world.unit_id;
        let environment = world.environment_id;
        let units = units::get(conn, unit)
            .unwrap()
            .map(|row| format!("{} {} {:?}", row.slug, row.branch, row.status))
            .into_iter()
            .collect();
        let environments = environments::list_for_unit(conn, unit)
            .unwrap()
            .iter()
            .map(|row| {
                format!(
                    "attempt {} {:?} managed={} ports={:?}",
                    row.attempt, row.state, row.managed, row.ports
                )
            })
            .collect();
        let sessions = sessions::list_for_environment(conn, environment)
            .unwrap()
            .iter()
            .map(|row| {
                format!("{:?} pgid={:?} open={}", row.actor.name, row.pgid, row.ended_at.is_none())
            })
            .collect();
        let trash = trash::get(conn, environment)
            .unwrap()
            .map(|row| format!("{} at {}", row.slug, world.scrub(&row.path)))
            .into_iter()
            .collect();
        let events = events::list_for_unit(conn, unit)
            .unwrap()
            .iter()
            .map(|row| {
                let refs: Vec<String> = row
                    .refs
                    .iter()
                    .map(|(name, value)| format!("{name}={}", world.scrub(Path::new(value))))
                    .collect();
                format!("{:?} {} {:?}", row.kind, world.scrub(Path::new(&row.body)), refs)
            })
            .collect();
        Self { units, environments, sessions, trash, events }
    }
}

// ---------------------------------------------------------------------------
// The world.
// ---------------------------------------------------------------------------

/// A checkout, a base, a state directory and the registry in it.
pub struct World {
    /// Kept so that the temporary directory outlives the test.
    root: TempDir,
    /// The path of `root` with its links resolved, which is what the registry records.
    resolved: PathBuf,
    /// The person's checkout.
    pub source: PathBuf,
    /// Nodal's state directory.
    pub state: PathBuf,
    /// The base a home is cloned from.
    pub base: PathBuf,
    /// The unit every operation here acts on.
    pub unit_id: nodal_core::model::UnitId,
    /// Its materialisation.
    pub environment_id: nodal_core::model::EnvId,
}

impl World {
    /// A checkout with one commit, and a base cloned from it.
    pub fn new() -> Self {
        let root = TempDir::new().unwrap();
        let resolved = root.path().canonicalize().unwrap();
        let source = resolved.join("project");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "a project\n").unwrap();
        git_ok(&source, &["init", "-q", "-b", "main"]);
        git_ok(&source, &["config", "user.email", "unit@example.invalid"]);
        git_ok(&source, &["config", "user.name", "Test"]);
        git_ok(&source, &["add", "-A"]);
        git_ok(&source, &["commit", "-qm", "first"]);

        let state = resolved.join("state");
        let base = state.join("project").join("b").join("00000004");
        clone_to_base(&source, &base);
        // The sweep removes a cache that records the path it was made at. Without one
        // the create's relocation report is empty and writes no event, which is the
        // very thing one of the locks is about.
        std::fs::create_dir_all(base.join(CACHE)).unwrap();
        std::fs::write(base.join(CACHE).join("module.pyc"), base.display().to_string()).unwrap();
        assert!(base.join(CACHE).is_dir(), "the base carries a cache for the sweep to find");

        Self { root, resolved, source, state, base, unit_id: id('2'), environment_id: id('3') }
    }

    /// The registry of this world, opened afresh.
    pub fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// A path with this world's temporary directory taken out of it, so that two worlds
    /// render the same path the same way.
    pub fn scrub(&self, path: &Path) -> String {
        path.display().to_string().replace(&self.resolved.display().to_string(), "<world>")
    }

    /// What this world holds after an operation.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::take(&self.store(), self)
    }

    // -----------------------------------------------------------------------
    // Rows.
    // -----------------------------------------------------------------------

    /// The project row.
    pub fn project(&self) -> Project {
        Project {
            id: id('1'),
            root: self.source.clone(),
            name: ProjectName::parse("project").unwrap(),
            recipe_hash: Digest::parse("abc123").unwrap(),
            created_at: at(),
        }
    }

    /// The base row the environment names, which its foreign key requires.
    pub fn base_row(&self) -> Base {
        Base {
            id: id('4'),
            project_id: id('1'),
            ws_fingerprint: WorkspaceFp(Digest::parse("00000000000000000000000000000004").unwrap()),
            platform: Platform::parse("x86_64-unknown-linux-gnu").unwrap(),
            commit: git(&self.source, &["rev-parse", "HEAD"]).parse().unwrap(),
            path: self.base.clone(),
            built_at: at(),
            last_used: at(),
        }
    }

    /// The unit row.
    pub fn unit(&self) -> Unit {
        Unit {
            id: self.unit_id,
            project_id: id('1'),
            slug: Slug::parse("worker-import").unwrap(),
            objective: None,
            objective_epistemic: None,
            branch: BranchName::parse("nodal/worker-import").unwrap(),
            parent_branch: None,
            status: UnitStatus::Open,
            created_at: at(),
            updated_at: at(),
        }
    }

    /// Where the unit's home is.
    pub fn home(&self) -> PathBuf {
        self.state.join("project").join("e").join("00000001")
    }

    /// The environment row.
    pub fn environment(&self) -> Environment {
        Environment {
            id: self.environment_id,
            unit_id: self.unit_id,
            attempt: 1,
            home: self.home(),
            managed: true,
            base_id: Some(id('4')),
            ws_fp_materialized: None,
            schema_fp_materialized: None,
            host: nodal_core::lifecycle::owner::current_host(),
            db_name: None,
            ports: Ports::default(),
            fixed_port: None,
            state: EnvState::Stopped,
            created_at: at(),
            last_active: at(),
        }
    }

    /// Put the project, base, unit and environment rows in the registry.
    ///
    /// This is the world an operation that acts on an existing unit starts from. A
    /// create writes these rows itself and must not find them already there.
    pub fn insert_unit(&self) {
        let store = self.store();
        projects::insert(store.conn(), &self.project()).unwrap();
        bases::insert(store.conn(), &self.base_row()).unwrap();
        units::insert(store.conn(), &self.unit()).unwrap();
        environments::insert(store.conn(), &self.environment()).unwrap();
    }
}

/// An empty sink for the exclusion rows a create's clone left standing.
///
/// A plan takes one so that the caller can read what the copy decided. Nothing here
/// reads it: the locks compare the rows and events an operation wrote, and this value
/// reaches the person who ran the command rather than the registry. It is the shape the
/// step-output column removes.
pub fn sink() -> Arc<OnceLock<Vec<tracked::Kept>>> {
    Arc::new(OnceLock::new())
}

// ---------------------------------------------------------------------------
// Journalling a run.
// ---------------------------------------------------------------------------

/// Journal a run of `plan` as one a process that is no longer there started.
///
/// The recovery mode the journal keeps is the plan's own, so a caller that wants a run
/// the next invocation finishes rather than takes back sets `plan.recovery` first.
pub fn journal_of(world: &World, plan: &Plan) -> journal::Operation {
    let store = world.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    journal::start(store.conn(), id, plan, &dead_owner(), at()).unwrap();
    journal::get(store.conn(), id).unwrap().expect("the run is journalled")
}

/// Write down that a step of a run was applied.
pub fn mark_applied(world: &World, id: OperationId, position: usize, key: &str) {
    let store = world.store();
    let record = StepRecord {
        position: u32::try_from(position).unwrap(),
        key: key.to_owned(),
        state: StepState::Applied,
        updated_at: Timestamp::now(),
    };
    journal::mark_step(store.conn(), id, &record).unwrap();
}

/// This host, with a process identifier that is certainly not running on it.
pub fn dead_owner() -> Owner {
    Owner { host: Owner::current().host, pid: dead_pid() }
}

/// A process identifier that is certainly not running on this host.
pub fn dead_pid() -> u32 {
    let here = Owner::current();
    for _ in 0..16 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        if (Owner { host: here.host.clone(), pid }).state(&here) == Liveness::Gone {
            return pid;
        }
    }
    panic!("no process identifier stayed free long enough to use");
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

/// An identifier of the fixed shape, differing only in its last character.
pub fn id<T: std::str::FromStr<Err = Error>>(last: char) -> T {
    format!("01J8Z6H000000000000000000{last}").parse().unwrap()
}

/// The instant every fixture row is stamped with.
pub fn at() -> Timestamp {
    Timestamp::parse(AT).unwrap()
}

/// `git` in a directory, as trimmed text, with the call insisted upon.
pub fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// `git` in a directory, with only its success insisted upon.
pub fn git_ok(directory: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(directory)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// Make the base the plan clones, the way the substrate makes one: a clone of the
/// checkout, detached at its commit, so the base holds what is committed and no more.
fn clone_to_base(source: &Path, base: &Path) {
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    git_ok(
        base.parent().unwrap(),
        &["clone", "--quiet", "--", &source.display().to_string(), &base.display().to_string()],
    );
    let head = git(source, &["rev-parse", "HEAD"]);
    git_ok(base, &["checkout", "--force", "--detach", &head]);
}

// ---------------------------------------------------------------------------
// The parameters of each operation.
// ---------------------------------------------------------------------------

/// The process group the reclaim fixture's teardown signals and cannot stop.
const SURVIVOR: u32 = 424_242;

impl World {
    /// The parameters of a create, with every identifier fixed.
    pub fn create_params(&self) -> new::Params {
        new::Params {
            project: self.project(),
            recipe: Recipe::default(),
            base_path: self.base.clone(),
            state_dir: self.state.clone(),
            unit: self.unit(),
            environment: self.environment(),
            block: PortBlock { project_id: id('1'), first: 20_000, last: 20_099 },
            ports: vec!["app".parse().unwrap()],
        }
    }

    /// The parameters of an adopt that materialises a home, which is a create with the
    /// branch fetched from the project's own checkout.
    pub fn adopt_params(&self) -> adopt::Params {
        adopt::Params {
            project: self.project(),
            recipe: Recipe::default(),
            state_dir: self.state.clone(),
            unit: self.unit(),
            environment: self.environment(),
            block: PortBlock { project_id: id('1'), first: 20_000, last: 20_099 },
            ports: vec!["app".parse().unwrap()],
            source: adopt::Source::Materialized {
                base_path: self.base.clone(),
                from: self.source.clone(),
            },
            recovered: false,
        }
    }

    /// The parameters of an adopt of a checkout that is already there and stays where
    /// it is. This is the shape with no relocation step, so its home is a root.
    pub fn adopt_in_place_params(&self) -> adopt::Params {
        self.build_checkout();
        let mut params = self.adopt_params();
        params.source = adopt::Source::InPlace;
        params.environment.managed = false;
        params.environment.home = self.checkout();
        params
    }

    /// The parameters of a merge.
    pub fn merge_params(&self, stages: merge::Stages, resuming: bool) -> merge::Params {
        self.build_checkout();
        let mut environment = self.environment();
        environment.home = self.checkout();
        merge::Params {
            project: self.project(),
            unit: self.unit(),
            environment,
            target: merge::Target { branch: String::from("main"), oid: oid(&self.source, "main") },
            head: oid(&self.checkout(), "nodal/worker-import"),
            message: String::from("worker import"),
            stages,
            resuming,
        }
    }

    /// The parameters of a reclaim of a managed home that is on disk.
    pub fn reclaim_params(&self) -> reclaim::Params {
        reclaim::Params {
            project: self.project(),
            unit: self.unit(),
            environment: self.environment(),
            entry: Some(self.trashed()),
            tethers: Vec::new(),
        }
    }

    /// Where a reclaimed home goes.
    fn trashed(&self) -> Trashed {
        Trashed {
            environment_id: self.environment_id,
            unit_id: self.unit_id,
            project_id: id('1'),
            slug: Slug::parse("worker-import").unwrap(),
            home: self.home(),
            path: self.state.join("project").join("trash").join("00000001"),
            snapshot: None,
            trashed_at: at(),
            expires_at: at(),
        }
    }

    /// A checkout of the project on the unit's branch, with work on it. This is the
    /// home an adopt finds and a merge acts on.
    pub fn checkout(&self) -> PathBuf {
        self.resolved.join("checkout")
    }

    /// Build the checkout, once.
    pub fn build_checkout(&self) {
        let checkout = self.checkout();
        if checkout.is_dir() {
            return;
        }
        git_ok(
            &self.resolved,
            &[
                "clone",
                "--quiet",
                "--",
                &self.source.display().to_string(),
                &checkout.display().to_string(),
            ],
        );
        git_ok(&checkout, &["config", "user.email", "unit@example.invalid"]);
        git_ok(&checkout, &["config", "user.name", "Test"]);
        git_ok(&checkout, &["checkout", "-q", "-b", "nodal/worker-import"]);
        std::fs::write(checkout.join("worker.txt"), "the work\n").unwrap();
        git_ok(&checkout, &["add", "-A"]);
        git_ok(&checkout, &["commit", "-qm", "work"]);
    }
}

/// The commit a ref stands at, in a repository a world made.
fn oid(repository: &Path, reference: &str) -> Oid {
    git(repository, &["rev-parse", reference]).parse().unwrap()
}

// ---------------------------------------------------------------------------
// Running an operation twice: to completion, and as a run that was taken over.
// ---------------------------------------------------------------------------

impl World {
    /// Apply a plan from the start and commit it, the way a process that lives does.
    fn to_completion(&self, plan: &Plan) -> Snapshot {
        let mut store = self.store();
        lifecycle::run(&mut store, plan).expect("the operation finishes");
        drop(store);
        self.snapshot()
    }

    /// Apply the first `applied` steps of a plan, journal the run as one whose process
    /// then died, and let the next invocation take it over and finish it.
    ///
    /// The journalled recovery mode is [`Recovery::Resume`]. All four lifecycle
    /// operations choose [`Recovery::RollBack`] today, so this is the one line that
    /// separates a run the next invocation finishes from one it takes back. The
    /// property under test is the journal's: a plan rebuilt from what the journal kept
    /// has to be able to finish the run that was interrupted.
    fn taken_over(&self, plan: &mut Plan, applied: usize) -> Snapshot {
        plan.recovery = Recovery::Resume;
        let record = journal_of(self, plan);
        for (position, step) in plan.steps.iter().enumerate().take(applied) {
            step.apply().expect("the killed run got this far");
            mark_applied(self, record.id, position, &step.key());
        }
        let mut store = self.store();
        let reported = lifecycle::resolve(&mut store, &ops::rebuilders()).unwrap();
        match reported.as_slice() {
            [only] => assert!(
                matches!(only.action, Action::Resumed { .. }),
                "the run was taken over and finished: {only:?}"
            ),
            other => {
                panic!("one unfinished run was expected, and there were {}: {other:?}", other.len())
            }
        }
        drop(store);
        self.snapshot()
    }

    // -- create ------------------------------------------------------------

    /// A create that runs from end to end.
    pub fn run_new_to_completion(&self) -> Snapshot {
        self.prepare_create();
        self.to_completion(&new::plan(&self.create_params(), &sink()).unwrap())
    }

    /// A create whose process died after `applied` steps.
    pub fn resume_new_after(&self, applied: usize) -> Snapshot {
        self.prepare_create();
        let mut plan = new::plan(&self.create_params(), &sink()).unwrap();
        self.taken_over(&mut plan, applied)
    }

    /// A create writes the unit and environment rows itself, so only the project and
    /// the base are there before it runs.
    fn prepare_create(&self) {
        let store = self.store();
        projects::insert(store.conn(), &self.project()).unwrap();
        bases::insert(store.conn(), &self.base_row()).unwrap();
    }

    // -- adopt -------------------------------------------------------------

    /// An adopt of a checkout that stays where it is, run from end to end.
    pub fn run_adopt_to_completion(&self) -> Snapshot {
        self.prepare_adopt();
        self.to_completion(&adopt::plan(&self.adopt_in_place_params(), &sink()).unwrap())
    }

    /// The same adopt, whose process died after `applied` steps.
    pub fn resume_adopt_after(&self, applied: usize) -> Snapshot {
        self.prepare_adopt();
        let mut plan = adopt::plan(&self.adopt_in_place_params(), &sink()).unwrap();
        self.taken_over(&mut plan, applied)
    }

    fn prepare_adopt(&self) {
        self.build_checkout();
        self.prepare_create();
    }

    // -- merge -------------------------------------------------------------

    /// A merge that runs from end to end.
    pub fn run_merge_to_completion(&self) -> Snapshot {
        self.prepare_merge();
        let sink = Arc::new(OnceLock::new());
        self.to_completion(
            &merge::plan(&self.merge_params(merge::Stages::all(), false), &sink).unwrap(),
        )
    }

    /// A merge whose process died after `applied` steps.
    pub fn resume_merge_after(&self, applied: usize) -> Snapshot {
        self.prepare_merge();
        let sink = Arc::new(OnceLock::new());
        let mut plan = merge::plan(&self.merge_params(merge::Stages::all(), false), &sink).unwrap();
        self.taken_over(&mut plan, applied)
    }

    fn prepare_merge(&self) {
        self.build_checkout();
        let store = self.store();
        projects::insert(store.conn(), &self.project()).unwrap();
        bases::insert(store.conn(), &self.base_row()).unwrap();
        units::insert(store.conn(), &self.unit()).unwrap();
        let mut environment = self.environment();
        environment.home = self.checkout();
        environments::insert(store.conn(), &environment).unwrap();
    }

    // -- reclaim -----------------------------------------------------------

    /// A reclaim that runs from end to end, of a unit one of whose process groups
    /// survived the teardown.
    pub fn run_reclaim_to_completion(&self) -> Snapshot {
        self.prepare_reclaim();
        let (teardown, released) = surviving_teardown();
        self.to_completion(&reclaim::plan(&self.reclaim_params(), &teardown, &released).unwrap())
    }

    /// The same reclaim, whose process died after `applied` steps.
    pub fn resume_reclaim_after(&self, applied: usize) -> Snapshot {
        self.prepare_reclaim();
        let (teardown, released) = surviving_teardown();
        let mut plan = reclaim::plan(&self.reclaim_params(), &teardown, &released).unwrap();
        self.taken_over(&mut plan, applied)
    }

    /// The unit, its home on disk, and the open session row of the tethered group.
    fn prepare_reclaim(&self) {
        self.insert_unit();
        std::fs::create_dir_all(self.home()).unwrap();
        std::fs::write(self.home().join("README.md"), "a home\n").unwrap();
        let store = self.store();
        sessions::insert(store.conn(), &self.tether()).unwrap();
    }

    /// The open session row of a tethered process group.
    fn tether(&self) -> Session {
        Session {
            id: id('5'),
            environment_id: self.environment_id,
            actor: Actor { kind: ActorKind::Agent, name: "claude-code".parse().unwrap() },
            pid: Some(SURVIVOR),
            pgid: Some(SURVIVOR),
            started_at: at(),
            ended_at: None,
        }
    }
}

/// The sinks of a reclaim whose teardown signalled one process group and could not
/// stop it.
///
/// The teardown is fixed rather than measured, because no test can make a process group
/// survive `SIGKILL`. The value is what a real teardown reports in the one condition the
/// reclaim module doc names: a group that is still running, whose session row is the
/// only record `nodal gc` has of it and must therefore stay open.
fn surviving_teardown()
-> (Arc<OnceLock<reclaim::Teardown>>, Arc<OnceLock<nodal_core::services::ports::Released>>) {
    let teardown = Arc::new(OnceLock::new());
    let _ = teardown.set(reclaim::Teardown {
        stopped: Stopped {
            asked: Vec::new(),
            killed: Vec::new(),
            left: vec![Target::Group(SURVIVOR)],
            spared: Vec::new(),
        },
        containers: Vec::new(),
        notes: Vec::new(),
    });
    (teardown, Arc::new(OnceLock::new()))
}
