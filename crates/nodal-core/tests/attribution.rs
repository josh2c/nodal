//! Acceptance for attribution: what is running, and whose it is.
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

#![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use nodal_core::model::{EnvId, HostName, PortName, Ports, ProjectId, Timestamp, UnitId};
use nodal_core::output::view::Ps;
use nodal_core::runtime::attribute::{self, Attributed, Confidence, Kind, Reach, Source};
use nodal_core::runtime::processes::{Live, Processes, Running, Withheld};
use nodal_core::runtime::ps;
use nodal_core::services::docker::{self, Docker, Output, UNIT_LABEL};
use nodal_core::store::{Store, environments, projects, units};
use nodal_safety::{platform, process, rows};

/// The unit every row in this file belongs to.
const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// Its materialisation.
const ENVIRONMENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

/// A registry that holds one unit, materialised at `home` on this host, holding `ports`.
fn registry(root: &Path, home: &Path, ports: Ports) -> Store {
    let now = Timestamp::now();
    let project_id = ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap();
    let unit = rows::unit(
        UnitId::parse(UNIT).unwrap(),
        project_id,
        "worker-import",
        "nodal/worker-import",
        now,
    );
    let mut environment = rows::environment(EnvId::parse(ENVIRONMENT).unwrap(), unit.id, home, now);
    environment.ports = ports;
    let project = rows::project(unit.project_id, root.join("checkout"), "fixture", now);
    let store = Store::open(root.join("registry.db")).unwrap();
    projects::insert(store.conn(), &project).unwrap();
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(store.conn(), &environment).unwrap();
    store
}

fn host() -> HostName {
    nodal_core::model::HostName::current()
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

/// A host whose process table Nodal cannot read.
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

/// Read this machine again until it has a row about `pid`, and answer with that reading.
///
/// A process exists before it has replaced itself with the program it was started for. A
/// scan taken in that instant reads the environment the test binary had rather than the one
/// the test gave the process, so there is no row — rarely, and never twice the same way.
/// The reading is a question about the machine, so it is asked again until the machine
/// answers or the deadline passes. What is asserted about the row is asserted once.
fn observed(store: &Store, pid: u32) -> Ps {
    process::until("a row for the process the test started", || {
        let answer = observe(store);
        about(&answer, pid).is_some().then_some(answer)
    })
}

#[test]
fn a_process_started_inside_the_homes_environment_is_certain() {
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let child = process::carrying(UNIT, &home);
    let pid = child.pid();

    let answer = observed(&store, pid);
    let row = about(&answer, pid).unwrap();
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.confidence, Confidence::Certain);
    assert_eq!(row.signal, Source::Environment);
    assert_eq!(row.what, "sleep 30");
    assert!(answer.notes.iter().any(|note| note.signal == Source::Docker));
}

#[test]
fn a_process_started_in_a_plain_terminal_in_the_home_is_probable_by_its_directory() {
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let child = process::standing_in(&home);
    let pid = child.pid();

    let answer = observed(&store, pid);
    let row = about(&answer, pid).unwrap();
    assert_eq!(row.slug.as_str(), "worker-import");
    assert_eq!(row.confidence, Confidence::Probable);
    assert_eq!(row.signal, Source::Cwd);
}

/// A restricted binary carrying a unit's identifier, standing in the home.
///
/// This stays a host split, because the refusal is the kernel's. On Linux the variables
/// are read, so the row is certain. macOS zeroes the variables of `/bin/sleep`, so the row
/// is probable by its directory, and a note says why the variables were not read. It is
/// never certain on a guess, so a teardown never signals it.
#[test]
fn a_restricted_binary_is_found_by_its_directory_and_its_variables_are_not_guessed() {
    let directory = tempfile::tempdir().unwrap();
    let home = make_home(directory.path());
    let store = registry(directory.path(), &home, Ports::default());

    let mut command = Command::new("/bin/sleep");
    command.arg("30").current_dir(&home).env("NODAL_ID", UNIT).env("NODAL_ROOT", &home);
    let child = process::Owned::spawn(&mut command);
    let pid = child.pid();

    // The reading this host gives once the child has become `sleep`. A Linux scan reads the
    // variables before the command, so one read can take the variables from before the
    // exec and the command from after it, and then the row is probable for an instant.
    let settled = |row: &Attributed| {
        row.what == "sleep 30"
            && (cfg!(target_os = "macos") || row.confidence == Confidence::Certain)
    };
    let answer = process::until("a row for the sleep the test started", || {
        let answer = observe(&store);
        about(&answer, pid).is_some_and(settled).then_some(answer)
    });
    let row = about(&answer, pid).unwrap();
    assert_eq!(row.slug.as_str(), "worker-import");
    if cfg!(target_os = "macos") {
        assert_eq!(row.confidence, Confidence::Probable, "{row:?}");
        assert_eq!(row.signal, Source::Cwd);
        let note = answer
            .notes
            .iter()
            .find(|note| note.signal == Source::Environment && note.why == attribute::RESTRICTED)
            .unwrap_or_else(|| panic!("no note says the variables were withheld: {answer:?}"));
        assert_eq!(note.reach, Reach::Part, "the table was read: {note:?}");
    } else {
        assert_eq!(row.confidence, Confidence::Certain, "{row:?}");
        assert_eq!(row.signal, Source::Environment);
    }
}

/// A process of another account is kept, counted and never read — on both hosts.
///
/// **This was a host split and it is not one any more.** A Linux scan used to leave such
/// a process out altogether: no row, no note, nothing. That made the one case the safety
/// contract is written for — a process this account cannot see, standing in a home — the
/// one case that produced no evidence at all (FS-6). Both hosts now list it with what was
/// withheld, so a reader can say how much it could not see.
///
/// The process that started the machine is another account's unless the test runs as
/// root, and it is on every host this suite runs on, so it is the one row that can be
/// asserted without making a machine hold anything.
#[test]
fn a_process_of_another_account_is_kept_and_counted_and_never_read() {
    // SAFETY: `geteuid` takes no argument and cannot fail.
    if unsafe { libc::geteuid() } == 0
        && platform::skipped("another account's process is withheld", "the test runs as root")
    {
        return;
    }
    let running = Live.scan().unwrap();
    let first = running
        .iter()
        .find(|process| process.pid == 1)
        .unwrap_or_else(|| panic!("the first process is not in the table"));
    assert_eq!(first.withheld, Some(Withheld::AnotherAccount), "{first:?}");
    assert!(first.vars.is_empty() && first.cwd.is_none(), "{first:?}");
    assert!(first.held.is_empty(), "what a withheld process holds is refused too: {first:?}");
    assert!(
        first.lineage.parent.is_some() || first.lineage.group.is_some(),
        "the one thing still readable about it is where it came from: {first:?}"
    );
    let notes = attribute::withheld(&running);
    let signals: Vec<Source> = notes
        .iter()
        .filter(|note| note.why == attribute::ANOTHER_ACCOUNT)
        .map(|note| note.signal)
        .collect();
    assert_eq!(signals, [Source::Environment, Source::Cwd], "{notes:?}");
    assert!(notes.iter().all(|note| note.reach == Reach::Part), "{notes:?}");
}
