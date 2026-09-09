//! Acceptance test for T1.2: substrate bases.
//!
//! Four claims, one file.
//!
//! * Two bases coexist when two workspaces key differently, and the first of them is a
//!   fresh clone of the remote rather than a copy of the checkout that asked for it —
//!   which is checked by putting a file in the checkout that was never committed and
//!   asserting no base has it.
//! * `nodal base gc` refuses a base a unit still holds, and a sweep steps over it.
//! * A base built from the nearest neighbour is faster than a cold one, because the
//!   neighbour's installed state comes across with the copy. The numbers are printed.
//! * A build killed between two steps is finished by the next invocation rather than
//!   started again, which is the reason a base build resumes where every other
//!   operation rolls back.
//!
//! Nothing here needs a package manager on the machine. The project's build command is
//! a script this test writes, and it behaves the way a real install behaves: slow when
//! it finds nothing installed, and quick when the copy brought the installed state
//! with it. What a real package manager would be invoked as is a unit test beside
//! `substrate::build`.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nodal_core::lifecycle::ops::new::ensure_project;
use nodal_core::lifecycle::{Action, Rebuild};
use nodal_core::model::{
    EnvState, Environment, HostName, Ports, Project, Slug, Timestamp, Unit, UnitStatus,
};
use nodal_core::store::{Store, environments, units};
use nodal_core::substrate::progress::{Collector, Reporter};
use nodal_core::substrate::{self, BaseBuild, Origin, Request};
use nodal_core::{Error, lifecycle, recipe};
use nodal_safety::git::{git, identity};
use tempfile::TempDir;

/// The environment variable the child reads its root from.
const ROOT_VAR: &str = "NODAL_T12_ROOT";

/// The environment variable that tells the build script to park instead of working.
const PARK_VAR: &str = "NODAL_T12_PARK";

/// The name of the child test, as the harness filters on it.
const CHILD_TEST: &str = "child_builds_a_base_and_parks_inside_a_step";

/// How long the parent waits for the child to reach the park.
const PARK_TIMEOUT: Duration = Duration::from_secs(120);

/// How often it looks.
const POLL: Duration = Duration::from_millis(20);

/// How long the build script pretends a cold install takes. Long enough that the
/// difference between a cold build and a neighbour build is not a matter of noise.
const COLD_SECONDS: &str = "1";

// ---------------------------------------------------------------------------
// The world the tests run in.
// ---------------------------------------------------------------------------

/// A remote, a checkout of it, a Nodal home, and a registry.
struct World {
    /// The root everything is under, so the child can be told about it with one
    /// variable.
    root: PathBuf,
}

impl World {
    fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The repository bases are cloned from.
    fn remote(&self) -> PathBuf {
        self.root.join("remote")
    }

    /// The user's own working copy, which no base is ever a copy of.
    fn checkout(&self) -> PathBuf {
        self.root.join("checkout")
    }

    /// This machine's Nodal home, as a test gets to choose it.
    fn home(&self) -> PathBuf {
        self.root.join("nodal-home")
    }

    fn registry(&self) -> PathBuf {
        self.root.join("registry.db")
    }

    /// The script standing in for a package manager's install.
    fn build_script(&self) -> PathBuf {
        self.root.join("warm-build")
    }

    fn store(&self) -> Store {
        Store::open(self.registry()).unwrap()
    }

    fn project(&self, store: &mut Store) -> Project {
        ensure_project(store, &self.checkout(), &self.recipe()).unwrap()
    }

    fn recipe(&self) -> nodal_core::model::recipe::Recipe {
        recipe::load(self.checkout()).unwrap().recipe
    }

    fn request(&self, project: &Project) -> Request {
        Request {
            project: project.clone(),
            source: self.checkout(),
            recipe: self.recipe(),
            state_dir: self.home(),
            warm: true,
        }
    }
}

/// Everything a test starts from: a remote with one commit, a checkout of it, and a
/// build script that is slow the first time and quick afterwards.
fn world() -> (TempDir, World) {
    let dir = TempDir::new().unwrap();
    let world = World::at(dir.path());
    write_build_script(&world.build_script());
    make_remote(&world);
    git(dir.path(), &["clone", "--quiet", "--", text(&world.remote()), text(&world.checkout())]);
    // A file the user has and no commit does, kept out of the index the way a person's
    // scratch file is. No base may ever hold it.
    write(&world.checkout().join(".git/info/exclude"), "LOCAL-ONLY.txt\n");
    write(&world.checkout().join("LOCAL-ONLY.txt"), "not committed anywhere\n");
    (dir, world)
}

/// Create the remote and put one commit in it.
///
/// Bare, as a real remote is: a checkout can push to it, which is what the tests that
/// move the workspace key on do before they ask for the base that key names.
fn make_remote(world: &World) {
    let seed = world.root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "--quiet", "--initial-branch=main"]);
    write(&seed.join("package.json"), "{\n  \"name\": \"fixture\"\n}\n");
    write(&seed.join("src.js"), "console.log(1)\n");
    write(
        &seed.join("nodal.toml"),
        &format!("[commands]\nbuild = \"{}\"\n", text(&world.build_script())),
    );
    commit(&seed, "first");
    git(&world.root, &["clone", "--quiet", "--bare", "--", text(&seed), text(&world.remote())]);
}

/// The script the recipe names as the project's build command.
///
/// It is what makes a neighbour build measurably cheaper than a cold one, and it is so
/// for the same reason a real one is: the work is proportional to what is not installed
/// yet, and a copy of the nearest base brings the installed state with it.
fn write_build_script(path: &Path) {
    let script = format!(
        "#!/bin/sh\n\
         set -e\n\
         if [ -n \"${PARK_VAR}\" ]; then : > \"${PARK_VAR}\"; while true; do sleep 1; done; fi\n\
         if [ -d node_modules ]; then echo incremental > .warm-log; exit 0; fi\n\
         mkdir -p node_modules\n\
         i=0\n\
         while [ $i -lt 200 ]; do echo x > \"node_modules/dep-$i.js\"; i=$((i + 1)); done\n\
         sleep {COLD_SECONDS}\n\
         echo cold > .warm-log\n"
    );
    write(path, &script);
    executable(path);
}

#[cfg(unix)]
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Commit everything a tree holds, and answer with the commit.
///
/// The identity is written into the repository first. The runner shuts the machine's
/// own Git configuration out, so a repository this test made has no identity until the
/// test gives it one.
fn commit(dir: &Path, message: &str) -> String {
    identity(dir);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "--allow-empty", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

/// Move the checkout's dependency inputs on, which is what moves its workspace key.
fn move_the_workspace_key(world: &World, marker: &str) -> String {
    let checkout = world.checkout();
    write(&checkout.join("package.json"), &format!("{{\n  \"name\": \"{marker}\"\n}}\n"));
    commit(&checkout, marker)
}

fn push(world: &World) {
    git(world.checkout(), &["push", "--quiet", "origin", "HEAD:main"]);
}

/// Build the base the checkout needs, reporting into a collector the test can read.
fn build(world: &World, store: &mut Store, project: &Project) -> (substrate::Outcome, Vec<String>) {
    let collector = Arc::new(Collector::default());
    let progress: Arc<dyn Reporter> = collector.clone();
    let outcome = substrate::ensure(store, &world.request(project), &progress).unwrap();
    (outcome, collector.lines())
}

// ---------------------------------------------------------------------------
// Two bases coexist, and neither is a copy of the checkout.
// ---------------------------------------------------------------------------

#[test]
fn two_bases_coexist_keyed_by_different_fingerprints() {
    let (_dir, world) = world();
    let mut store = world.store();
    let project = world.project(&mut store);

    let (first, _) = build(&world, &mut store, &project);
    assert!(first.built(), "the first call builds");
    assert_origin(&first, "a fresh clone of");

    move_the_workspace_key(&world, "second");
    push(&world);
    let (second, lines) = build(&world, &mut store, &project);
    assert!(second.built());
    assert_origin(&second, "base ");
    assert!(lines.iter().any(|line| line.contains("copied")), "the copy is reported: {lines:?}");

    assert_ne!(first.fingerprint, second.fingerprint, "the two are keyed differently");
    assert_ne!(first.base.id, second.base.id);
    assert_both_are_on_disk_and_neither_is_the_checkout(&[&first, &second]);
    assert_eq!(substrate::list(&store, project.id).unwrap().len(), 2);

    // And asking again for a key that is warm finds it rather than building it.
    let (again, _) = build(&world, &mut store, &project);
    assert!(!again.built());
    assert_eq!(again.base.id, second.base.id);
}

/// The base came from where the test says it should have.
fn assert_origin(outcome: &substrate::Outcome, phrase: &str) {
    let origin = outcome.origin.as_ref().map(Origin::describe).unwrap_or_default();
    assert!(origin.starts_with(phrase), "expected {phrase:?}, got {origin:?}");
}

/// Both bases are real directories, and neither carries what only the checkout has.
fn assert_both_are_on_disk_and_neither_is_the_checkout(outcomes: &[&substrate::Outcome]) {
    for outcome in outcomes {
        let path = &outcome.base.path;
        assert!(path.is_dir(), "{} is on disk", path.display());
        assert!(!path.join("LOCAL-ONLY.txt").exists(), "no base holds what only the checkout has");
    }
}

#[test]
fn a_base_can_be_built_for_a_commit_the_remote_does_not_have() {
    let (_dir, world) = world();
    let mut store = world.store();
    let project = world.project(&mut store);
    move_the_workspace_key(&world, "unpushed");

    let (outcome, lines) = build(&world, &mut store, &project);
    assert!(outcome.built());
    assert!(
        lines.iter().any(|line| line.contains("taking the objects from the checkout")),
        "the fallback says what it did: {lines:?}"
    );
    assert!(!outcome.base.path.join("LOCAL-ONLY.txt").exists(), "objects only, never files");
}

// ---------------------------------------------------------------------------
// A pinned base is refused.
// ---------------------------------------------------------------------------

#[test]
fn gc_refuses_a_pinned_base_and_sweeps_the_idle_ones() {
    let (_dir, world) = world();
    let mut store = world.store();
    let project = world.project(&mut store);

    let (pinned, _) = build(&world, &mut store, &project);
    move_the_workspace_key(&world, "idle");
    push(&world);
    let (idle, _) = build(&world, &mut store, &project);

    pin(&store, &project, &pinned.base.id);
    assert_eq!(substrate::pins(&store, pinned.base.id).unwrap(), 1);

    let refused = substrate::evict(&store, pinned.base.id, &Collector::default()).unwrap_err();
    assert!(
        matches!(&refused, Error::BasePinned { base, pins } if *base == pinned.base.id && *pins == 1),
        "a pinned base is refused: {refused}"
    );
    assert!(pinned.base.path.is_dir(), "and it is still there");

    // A sweep that keeps nothing still steps over the pinned one.
    let removed = substrate::gc(&store, project.id, 0, &Collector::default()).unwrap();
    assert_eq!(removed.iter().map(|base| base.id).collect::<Vec<_>>(), vec![idle.base.id]);
    assert!(!idle.base.path.exists(), "the idle base is gone from disk");
    assert!(pinned.base.path.is_dir(), "the pinned base is untouched");
    assert_eq!(substrate::list(&store, project.id).unwrap().len(), 1);
}

/// Record a unit whose environment was cloned from a base, which is what a pin is.
fn pin(store: &Store, project: &Project, base: &nodal_core::model::BaseId) {
    let unit = Unit {
        id: "01J8Z6H000000000000000000A".parse().unwrap(),
        project_id: project.id,
        slug: Slug::parse("holds-the-base").unwrap(),
        objective: None,
        objective_epistemic: None,
        branch: "nodal/holds-the-base".parse().unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    };
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(
        store.conn(),
        &Environment {
            id: "01J8Z6H000000000000000000B".parse().unwrap(),
            unit_id: unit.id,
            attempt: 1,
            home: PathBuf::from("/nowhere/e/01"),
            managed: true,
            base_id: Some(*base),
            ws_fp_materialized: None,
            schema_fp_materialized: None,
            host: HostName::parse("test").unwrap(),
            db_name: None,
            ports: Ports(BTreeMap::new()),
            fixed_port: None,
            state: EnvState::Stopped,
            created_at: Timestamp::now(),
            last_active: Timestamp::now(),
        },
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// A neighbour build is faster than a cold one.
// ---------------------------------------------------------------------------

#[test]
fn a_neighbour_build_is_faster_than_a_cold_one() {
    let (_dir, world) = world();
    let mut store = world.store();
    let project = world.project(&mut store);

    let cold = Instant::now();
    let (first, _) = build(&world, &mut store, &project);
    let cold = cold.elapsed();

    move_the_workspace_key(&world, "neighbour");
    push(&world);
    let warm = Instant::now();
    let (second, _) = build(&world, &mut store, &project);
    let warm = warm.elapsed();

    println!(
        "cold build {cold:?} (from the remote), neighbour build {warm:?} (from base {base})",
        base = first.base.id
    );
    assert!(matches!(second.origin, Some(Origin::Neighbour { .. })));
    assert_eq!(
        std::fs::read_to_string(second.base.path.join(".warm-log")).unwrap().trim(),
        "incremental",
        "the neighbour brought the installed state with it"
    );
    assert!(warm < cold, "a neighbour build is faster: cold {cold:?}, neighbour {warm:?}");
}

// ---------------------------------------------------------------------------
// A killed build is finished by the next invocation.
// ---------------------------------------------------------------------------

/// The child half: build a base for real and park inside the last step. Ignored, so it
/// runs only when the parent asks for it by name.
#[test]
#[ignore = "run by the parent test, in a process it kills"]
fn child_builds_a_base_and_parks_inside_a_step() {
    let world = World::at(std::env::var(ROOT_VAR).expect("the parent sets the root"));
    let mut store = world.store();
    let project = world.project(&mut store);
    let progress: Arc<dyn Reporter> = Arc::new(Collector::default());
    // Never returns: the parent kills this process while the build step is on the stack.
    let _ = substrate::ensure(&mut store, &world.request(&project), &progress);
    unreachable!("the parent kills the child before the build can finish");
}

#[test]
fn a_build_killed_between_steps_is_finished_by_the_next_invocation() {
    let (_dir, world) = world();
    let marker = world.root.join("parked");
    let mut child = spawn_child(&world, &marker);
    wait_for_park(&marker, &mut child);
    kill(&mut child);

    let mut store = world.store();
    let project = world.project(&mut store);
    assert!(substrate::list(&store, project.id).unwrap().is_empty(), "no row was committed");

    let resolutions = lifecycle::resolve(&mut store, &[&BaseBuild]).unwrap();

    assert_eq!(resolutions.len(), 1, "{resolutions:?}");
    assert_eq!(resolutions[0].kind, BaseBuild.kind());
    assert_eq!(
        resolutions[0].action,
        Action::Resumed { applied: vec![String::from("warm")] },
        "the clone and the checkout stand; only the step it died in is done again"
    );

    let bases = substrate::list(&store, project.id).unwrap();
    assert_eq!(bases.len(), 1, "the resumed build committed its row");
    assert!(bases[0].base.path.join(".warm-log").exists(), "and finished the work");

    // Resolved once, not reported again.
    assert!(lifecycle::resolve(&mut store, &[&BaseBuild]).unwrap().is_empty());
}

/// Re-execute this test binary, running only the child test.
fn spawn_child(world: &World, marker: &Path) -> Child {
    let binary = std::env::current_exe().expect("a test binary has a path");
    Command::new(binary)
        .args([CHILD_TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .env(ROOT_VAR, &world.root)
        .env(PARK_VAR, marker)
        .stdout(Stdio::null())
        .spawn()
        .expect("the test binary can be run again")
}

/// Wait until the child says it has reached the park.
fn wait_for_park(marker: &Path, child: &mut Child) {
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
    kill(child);
    panic!("the child never reached the park");
}

/// Kill the child outright and reap it, so no code of its own runs afterwards.
fn kill(child: &mut Child) {
    child.kill().expect("the child can be killed");
    child.wait().expect("the child can be reaped");
}
