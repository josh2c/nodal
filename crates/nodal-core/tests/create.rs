//! The create operation as a plan: what its steps do, what they undo, and what happens
//! to a run of it that a process never finished.
//!
//! The end-to-end test lives in `nodal-cli` (`tests/new.rs`), where a real binary is
//! killed with `SIGKILL`. This one is the same property without the race: the world a
//! killed run leaves is built here by hand, from the journal downwards, so that the
//! rebuild-and-roll-back path is exercised the same way on every machine.
//!
//! The base the plan clones is made here by hand for the same reason every identifier
//! is: a plan built twice has to be the same plan, and resolving a base is the other
//! operation's work, tested in `tests/substrate.rs`. It is made the way the substrate
//! makes one — a clone of the checkout, detached at the commit — so what a step reads
//! here is what a step reads in the product.
//!
//! The per-machine secrets file activation reads is `secrets.env` in the state
//! directory, and every fixture here has a temporary one, so no test in this file reads
//! or writes the file belonging to whoever is running it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

use nodal_core::lifecycle::journal::{self, State, StepRecord, StepState};
use nodal_core::lifecycle::ops::new::{self, Params};
use nodal_core::lifecycle::owner::{Liveness, Owner};
use nodal_core::lifecycle::{Action, guard, marker, ops};
use nodal_core::model::{
    Base, BranchName, Digest, EnvState, Environment, OperationId, Platform, Ports, Project,
    ProjectName, Recipe, Slug, Timestamp, Unit, UnitStatus, WorkspaceFp,
};
use nodal_core::store::{Store, bases, environments, projects, units};
use nodal_core::{Error, lifecycle};
use tempfile::TempDir;

/// A project, the base built from it, a state directory, and the registry in it.
struct Fixture {
    /// Kept so that the temporary directory outlives the test.
    _root: TempDir,
    /// The person's checkout. Read to decide the base; never cloned into a home.
    source: PathBuf,
    /// Nodal's state directory.
    state: PathBuf,
    /// The base a home is cloned from.
    base: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("README.md"), "a project\n").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "unit@example.invalid"],
            vec!["config", "user.name", "Test"],
            vec!["add", "-A"],
            vec!["commit", "-qm", "first"],
        ] {
            assert!(
                Command::new("git").args(&args).current_dir(&source).status().unwrap().success()
            );
        }
        let state = root.path().join("state");
        let base = state.join("project").join("b").join("00000004");
        clone_to_base(&source, &base);
        Self { _root: root, source, state, base }
    }

    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// The base row the environment names, which its foreign key requires.
    fn base_row(&self) -> Base {
        let at = Timestamp::parse("2026-09-07T09:00:00Z").unwrap();
        Base {
            id: id('4'),
            project_id: id('1'),
            ws_fingerprint: WorkspaceFp(Digest::parse("00000000000000000000000000000004").unwrap()),
            platform: Platform::parse("x86_64-unknown-linux-gnu").unwrap(),
            commit: git(&self.source, &["rev-parse", "HEAD"]).parse().unwrap(),
            path: self.base.clone(),
            built_at: at,
            last_used: at,
        }
    }

    /// The parameters of a create, with every identifier fixed so that two builds of
    /// the plan are the same plan.
    fn params(&self) -> Params {
        let at = Timestamp::parse("2026-09-07T09:00:00Z").unwrap();
        let project = Project {
            id: id('1'),
            root: self.source.clone(),
            name: ProjectName::parse("project").unwrap(),
            recipe_hash: Digest::parse("abc123").unwrap(),
            created_at: at,
        };
        let unit = Unit {
            id: id('2'),
            project_id: project.id,
            slug: Slug::parse("worker-import").unwrap(),
            objective: None,
            objective_epistemic: None,
            branch: BranchName::parse("nodal/worker-import").unwrap(),
            parent_branch: None,
            status: UnitStatus::Open,
            created_at: at,
            updated_at: at,
        };
        let environment = Environment {
            id: id('3'),
            unit_id: unit.id,
            attempt: 1,
            home: self.state.join("project").join("e").join("00000001"),
            managed: true,
            base_id: Some(id('4')),
            ws_fp_materialized: None,
            schema_fp_materialized: None,
            host: nodal_core::lifecycle::owner::current_host(),
            db_name: None,
            ports: Ports::default(),
            fixed_port: None,
            state: EnvState::Stopped,
            created_at: at,
            last_active: at,
        };
        Params {
            project,
            recipe: Recipe::default(),
            base_path: self.base.clone(),
            state_dir: self.state.clone(),
            unit,
            environment,
            block: nodal_core::model::PortBlock {
                project_id: id('1'),
                first: 20_000,
                last: 20_099,
            },
            ports: vec!["app".parse().unwrap()],
        }
    }
}

fn id<T: std::str::FromStr<Err = Error>>(last: char) -> T {
    format!("01J8Z6H000000000000000000{last}").parse().unwrap()
}

/// Make the base the plan clones, the way the substrate makes one: a clone of the
/// checkout, detached at its commit, so the base holds what is committed and no more.
fn clone_to_base(source: &Path, base: &Path) {
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    let parent = base.parent().unwrap();
    assert!(
        Command::new("git")
            .args(["clone", "--quiet", "--"])
            .arg(source)
            .arg(base)
            .current_dir(parent)
            .status()
            .unwrap()
            .success()
    );
    let head = git(source, &["rev-parse", "HEAD"]);
    assert!(
        Command::new("git")
            .args(["checkout", "--force", "--detach", &head])
            .current_dir(base)
            .status()
            .unwrap()
            .success()
    );
}

/// `git` in a directory, as trimmed text, with the call insisted upon.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn the_plan_is_the_same_plan_however_it_is_built() {
    let fixture = Fixture::new();
    let params = fixture.params();
    let built = new::plan(&params, &Arc::new(OnceLock::new())).unwrap();
    assert_eq!(
        built.keys(),
        [
            "home.materialize",
            "home.relocate",
            "git.scrub",
            "git.branch",
            "git.hide",
            "home.marker",
            "env.activate",
        ]
    );

    let record = journalled(&fixture, &params);
    let rebuilt = ops::rebuilders()
        .iter()
        .find(|entry| entry.kind() == new::KIND)
        .expect("this build knows how to rebuild a create")
        .rebuild(&record)
        .unwrap();
    assert_eq!(rebuilt.keys(), built.keys(), "a rebuilt plan lines up with the run's journal");
    assert_eq!(rebuilt.subject, built.subject);
}

#[test]
fn applying_the_steps_twice_changes_nothing_and_undoing_them_leaves_nothing() {
    let fixture = Fixture::new();
    let params = fixture.params();
    let home = params.environment.home.clone();

    let plan = new::plan(&params, &Arc::new(OnceLock::new())).unwrap();
    for step in &plan.steps {
        step.apply().unwrap();
        step.apply().unwrap();
    }
    assert_eq!(marker::read(&home).unwrap(), Some(params.unit.id));
    assert!(home.join(".envrc").is_file());

    for step in plan.steps.iter().rev() {
        step.undo().unwrap();
        step.undo().unwrap();
    }
    assert!(!home.exists(), "the home is gone, and undoing again is not a failure");
}

#[test]
fn a_run_whose_process_is_gone_is_rebuilt_and_rolled_back_to_nothing() {
    let fixture = Fixture::new();
    let params = fixture.params();
    let home = params.environment.home.clone();
    let record = journalled(&fixture, &params);

    // The world as a create killed inside its sixth step would have left it.
    let plan = new::plan(&params, &Arc::new(OnceLock::new())).unwrap();
    let store = fixture.store();
    for (position, step) in plan.steps.iter().enumerate().take(5) {
        step.apply().unwrap();
        mark(&store, record.id, position, &step.key(), StepState::Applied);
    }
    assert!(home.join(".git").is_dir(), "the killed run left a home behind");
    drop(store);

    let mut store = fixture.store();
    let reported = lifecycle::resolve(&mut store, &ops::rebuilders()).unwrap();
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(
        reported[0].action,
        Action::RolledBack {
            undone: vec![
                String::from("git.hide"),
                String::from("git.branch"),
                String::from("git.scrub"),
                String::from("home.relocate"),
                String::from("home.materialize"),
            ]
        },
        "every step the journal saw is undone, in reverse"
    );

    assert!(!home.exists(), "nothing of the killed run is left on disk");
    assert!(
        units::get(store.conn(), params.unit.id).unwrap().is_none(),
        "and none in the registry"
    );
    assert!(environments::get(store.conn(), params.environment.id).unwrap().is_none());
    assert_eq!(journal::get(store.conn(), record.id).unwrap().unwrap().state, State::RolledBack);
    assert_eq!(lifecycle::resolve(&mut store, &ops::rebuilders()).unwrap(), Vec::new());
}

#[test]
fn a_home_is_refused_inside_a_project_or_another_units_home() {
    let fixture = Fixture::new();
    let params = fixture.params();
    let store = fixture.store();
    projects::insert(store.conn(), &params.project).unwrap();
    bases::insert(store.conn(), &fixture.base_row()).unwrap();
    units::insert(store.conn(), &params.unit).unwrap();
    environments::insert(store.conn(), &params.environment).unwrap();

    guard::placement(store.conn(), &fixture.state.join("elsewhere"), &fixture.source).unwrap();

    let inside_project = fixture.source.join("nested");
    let refused = guard::placement(store.conn(), &inside_project, &fixture.source).unwrap_err();
    assert!(refused.to_string().contains(guard::SOURCE), "{refused}");

    let inside_home = params.environment.home.join("packages").join("web");
    let refused = guard::placement(store.conn(), &inside_home, Path::new("/nowhere")).unwrap_err();
    assert!(refused.to_string().contains(guard::HOME), "{refused}");
}

/// Journal a run of this plan as started by a process that is no longer there.
fn journalled(fixture: &Fixture, params: &Params) -> journal::Operation {
    let store = fixture.store();
    let id = OperationId::from_ulid(ulid::Ulid::new());
    let gone = Owner { host: Owner::current().host, pid: dead_pid() };
    let at = Timestamp::parse("2026-09-07T09:00:00Z").unwrap();
    journal::start(
        store.conn(),
        id,
        &new::plan(params, &Arc::new(OnceLock::new())).unwrap(),
        &gone,
        at,
    )
    .unwrap();
    journal::get(store.conn(), id).unwrap().expect("the run is journalled")
}

/// Write down that a step of a run was applied.
fn mark(store: &Store, id: OperationId, position: usize, key: &str, state: StepState) {
    let record = StepRecord {
        position: u32::try_from(position).unwrap(),
        key: key.to_owned(),
        state,
        updated_at: Timestamp::now(),
    };
    journal::mark_step(store.conn(), id, &record).unwrap();
}

/// A process identifier that is certainly not running on this host.
fn dead_pid() -> u32 {
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
