//! Acceptance for attribution (T1.10): what is running, and whose it is.
//!
//! Five claims, against this machine rather than against a table a test wrote:
//!
//! 1. A process started with a home's environment — what a `nodal shell` or an
//!    activated terminal gives it — is attributed to that unit as `certain`.
//! 2. A process started in a plain terminal that merely stands in the home, carrying no
//!    Nodal variable at all, is attributed to that unit as `probable`, by its directory.
//! 3. A listener on a port the registry granted to a home is attributed to that unit.
//! 4. A Docker daemon that is not there degrades to a note, and the rest of the answer
//!    is unaffected. Where a daemon is there, a container Nodal labelled is attributed
//!    to its unit as `certain`; where it is not, that check says it was skipped and why.
//! 5. A host whose process table cannot be read says so, for both signals that need
//!    one, and still answers with what the other signals saw. That is the macOS
//!    condition, exercised here with a seam that reports it.
//!
//! The two process claims are Linux claims: they are what `/proc` publishes. On a host
//! without it each reports itself as skipped rather than failing, and claim 5 is what
//! covers that host's behaviour.

#![allow(clippy::unwrap_used)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use nodal_core::model::{
    BranchName, Digest, EnvId, EnvState, Environment, HostName, PortName, Ports, Project,
    ProjectId, ProjectName, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::output::view::Ps;
use nodal_core::runtime::attribute::{Attributed, Confidence, Kind, Source};
use nodal_core::runtime::processes::{Live, Processes, Running};
use nodal_core::runtime::ps;
use nodal_core::services::docker::{self, Docker, Output, UNIT_LABEL};
use nodal_core::store::{Store, environments, projects, units};

/// The unit every row in this file belongs to.
const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// Its materialisation.
const ENVIRONMENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

/// A registry that holds one unit, materialised at `home` on this host, holding `ports`.
fn registry(root: &Path, home: &Path, ports: Ports) -> Store {
    let now = Timestamp::now();
    let unit = Unit {
        id: UnitId::parse(UNIT).unwrap(),
        project_id: ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        slug: Slug::parse("worker-import").unwrap(),
        objective: None,
        objective_epistemic: None,
        branch: BranchName::parse("nodal/worker-import").unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    };
    let environment = Environment {
        id: EnvId::parse(ENVIRONMENT).unwrap(),
        unit_id: unit.id,
        attempt: 1,
        home: home.to_path_buf(),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: host(),
        db_name: None,
        ports,
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: now,
        last_active: now,
    };
    let project = Project {
        id: unit.project_id,
        root: root.join("checkout"),
        name: ProjectName::parse("fixture").unwrap(),
        recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
        created_at: now,
    };
    let store = Store::open(root.join("registry.db")).unwrap();
    projects::insert(store.conn(), &project).unwrap();
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(store.conn(), &environment).unwrap();
    store
}

fn host() -> HostName {
    nodal_core::lifecycle::owner::current_host()
}

/// A home the kernel names the same way this test does, so a directory read back from
/// `/proc` compares equal to the one in the registry.
fn make_home(root: &Path) -> PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    home.canonicalize().unwrap()
}

/// A Docker that is not installed, which is the shape of every machine without one.
struct NoDocker;

impl Docker for NoDocker {
    fn run(&self, _args: &[&str]) -> nodal_core::Result<Output> {
        Err(nodal_core::Error::ToolSpawn {
            program: String::from("docker"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })
    }
}

/// A host whose process table Nodal cannot read, which is macOS until `sysctl` arrives.
struct NoTable;

impl Processes for NoTable {
    fn scan(&self) -> nodal_core::Result<Vec<Running>> {
        Err(nodal_core::Error::ProcessScanUnsupported { host: "macos" })
    }
}

/// Read this machine, with no Docker.
fn observe(store: &Store) -> Ps {
    ps::observe(store.conn(), &Live, &NoDocker, &host(), Timestamp::now()).unwrap()
}

/// The row about one process, when there is one.
fn about(answer: &Ps, pid: u32) -> Option<&Attributed> {
    answer.rows.iter().find(|row| row.pid == Some(pid) && row.kind == Kind::Process)
}

/// Whether this host publishes a process table, and a word about it when it does not.
fn has_proc(claim: &str) -> bool {
    if cfg!(target_os = "linux") {
        return true;
    }
    eprintln!("skipped ({claim}): a process scan reads /proc, which this host does not have");
    false
}

/// A child that is killed when the test ends, whichever way it ends.
struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_process_started_inside_the_homes_environment_is_certain() {
    if !has_proc("certain by environment") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    // What `nodal shell`, the prompt hook and direnv all give a process.
    let child = Sleeper(
        Command::new("sleep")
            .arg("30")
            .env("NODAL_ID", UNIT)
            .env("NODAL_ROOT", &home)
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();

    let answer = observe(&store);
    let row = about(&answer, pid).unwrap_or_else(|| panic!("no row for {pid}: {:?}", answer.rows));
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.confidence, Confidence::Certain);
    assert_eq!(row.signal, Source::Environment);
    assert_eq!(row.what, "sleep 30");
    assert!(answer.notes.iter().any(|note| note.signal == Source::Docker));
}

#[test]
fn a_process_started_in_a_plain_terminal_in_the_home_is_probable_by_its_directory() {
    if !has_proc("probable by directory") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    // A terminal with no integration and no direnv: it carries nothing, it just stands
    // in the home.
    let child = Sleeper(
        Command::new("sleep")
            .arg("30")
            .current_dir(&home)
            .env_remove("NODAL_ID")
            .env_remove("NODAL_ROOT")
            .spawn()
            .unwrap(),
    );
    let pid = child.0.id();

    let answer = observe(&store);
    let row = about(&answer, pid).unwrap_or_else(|| panic!("no row for {pid}: {:?}", answer.rows));
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.confidence, Confidence::Probable);
    assert_eq!(row.signal, Source::Cwd);
}

#[test]
fn a_listener_on_a_granted_port_is_attributed_to_its_unit() {
    if !cfg!(target_os = "linux") {
        eprintln!("skipped: a listener scan reads /proc/net/tcp, which this host does not have");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());

    // Bind first, then grant that port, so the test never has to guess a free one.
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    let mut granted = std::collections::BTreeMap::new();
    granted.insert(PortName::parse("app").unwrap(), port);
    let store = registry(directory.path(), &home, Ports(granted));

    let answer = observe(&store);
    let row = answer
        .rows
        .iter()
        .find(|row| row.kind == Kind::Listener)
        .unwrap_or_else(|| panic!("no listener row: {:?}", answer.rows));
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.port, Some(port));
    assert_eq!(row.what, "app");
    assert_eq!(row.confidence, Confidence::Probable);
    drop(socket);
}

#[test]
fn a_docker_that_is_not_there_is_a_note_and_not_a_failure() {
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let answer = observe(&store);
    let note = answer
        .notes
        .iter()
        .find(|note| note.signal == Source::Docker)
        .unwrap_or_else(|| panic!("no note about docker: {:?}", answer.notes));
    assert_eq!(note.why, "docker is not installed");
    // The rest of the answer still happened: a note is not an outage.
    assert!(nodal_core::output::render(&answer, nodal_core::output::Format::Json).is_ok());
}

#[test]
fn a_container_nodal_labelled_is_attributed_to_its_unit() {
    let Some(name) = docker_daemon("labelled container") else { return };
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let started = Command::new("docker")
        .args(["run", "--detach", "--rm", "--name", &name, "--label"])
        .arg(format!("{UNIT_LABEL}={UNIT}"))
        .args(["busybox", "sleep", "60"])
        .output()
        .unwrap();
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));

    let answer = ps::observe(store.conn(), &Live, &docker::Cli, &host(), Timestamp::now()).unwrap();
    let _ = Command::new("docker").args(["rm", "--force", &name]).output();

    let row = answer
        .rows
        .iter()
        .find(|row| row.kind == Kind::Container && row.what == name)
        .unwrap_or_else(|| panic!("no row for {name}: {:?}", answer.rows));
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.confidence, Confidence::Certain);
    assert_eq!(row.signal, Source::Docker);
}

#[test]
fn a_host_with_no_process_table_says_so_and_still_answers() {
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let answer = ps::observe(store.conn(), &NoTable, &NoDocker, &host(), Timestamp::now()).unwrap();
    for signal in [Source::Environment, Source::Cwd] {
        let note = answer
            .notes
            .iter()
            .find(|note| note.signal == signal)
            .unwrap_or_else(|| panic!("no note for {signal:?}: {:?}", answer.notes));
        assert!(note.why.contains("/proc"), "{}", note.why);
    }
    assert!(answer.rows.iter().all(|row| row.kind != Kind::Process));
}

/// A name for a container this test may create, or nothing and a printed reason.
///
/// A machine whose account is not in the `docker` group, and a machine with no Docker at
/// all, both report the check as skipped rather than failing it. CI runs it for real.
fn docker_daemon(claim: &str) -> Option<String> {
    match Command::new("docker").args(["info", "--format", "{{.ServerVersion}}"]).output() {
        Ok(output) if output.status.success() => {
            Some(format!("nodal-t1-10-{}", std::process::id()))
        }
        Ok(output) => {
            let why = String::from_utf8_lossy(&output.stderr);
            eprintln!("skipped ({claim}): {}", why.lines().next().unwrap_or("docker said nothing"));
            None
        }
        Err(error) => {
            eprintln!("skipped ({claim}): docker could not be started: {error}");
            None
        }
    }
}
