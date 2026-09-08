//! Acceptance test for the tether: `nodal run --tether`, and what reclaim does with it.
//!
//! Every assertion here is about a process a person could look for themselves with
//! `ps`. The unit under test is a real `nodal run --tether`, started against a real
//! repository, and the processes it leaves behind are real processes.
//!
//! Four things are asserted, and each is one of the reasons the flag exists.
//!
//! A tethered command that starts a child which starts another child is **dead whole**
//! after the unit is reclaimed. Not the leader — the group. That is the shape of every
//! development server: a supervisor, a compiler, a watcher, and a leader that has
//! usually replaced itself by the time anybody looks.
//!
//! The signals go **in order, and no further than they have to**. A tethered command
//! that stops when it is interrupted is never sent `SIGTERM`, and the file it writes
//! from its own signal handler is what says so.
//!
//! A tether whose `nodal run` was **killed with `SIGKILL`** is still stopped. The
//! registry row is the record of the group, not the process that made the row, so
//! losing the parent loses nothing.
//!
//! An **untethered** process in the same home is left to attribution, which was already
//! stopping it before this flag existed. The tether path adds a target; it does not take
//! one over.
//!
//! A process group is addressed with `kill`, which every host answers for. So unlike the
//! attribution half of a reclaim, none of this stands aside on a host with no process
//! table: these tests run everywhere the binary does.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::model::{EnvId, EnvState, Session, Timestamp};
use nodal_core::store::{Store, environments, projects, sessions, units};
use tempfile::TempDir;

/// How long a test waits for something it has started to reach the state it needs.
const TIMEOUT: Duration = Duration::from_secs(30);

/// How often a wait looks.
const POLL: Duration = Duration::from_millis(10);

// ---------------------------------------------------------------------------
// The workspace.
// ---------------------------------------------------------------------------

/// A one-commit project, the state directory its units go in, and a scratch directory
/// the tethered commands write to.
///
/// The scratch directory is outside every home on purpose. A file a test wrote inside a
/// unit's home would be untracked work, and the uniqueness check would refuse to reclaim
/// the unit — which is the check doing its job, and nothing to do with tethers.
struct Workspace {
    _root: TempDir,
    source: PathBuf,
    state: PathBuf,
    scratch: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        let scratch = root.path().join("scratch");
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        std::fs::write(source.join(".gitignore"), "node_modules/\n").unwrap();
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
        Self { _root: root, source, state, scratch }
    }

    /// `nodal` with this workspace's state directory, not yet run.
    fn command(&self, args: &[&str], cwd: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nodal"));
        command.args(args).current_dir(cwd).env("NODAL_HOME", &self.state);
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command
    }

    /// `nodal` run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args, &self.source).output().unwrap()
    }

    /// A `nodal run` whose output goes nowhere, not yet run.
    ///
    /// A tethered command keeps whatever it was given for standard output, which is
    /// what a person wants from a development server. It hands that on to every process
    /// it starts, so a test that collected the output would wait for the last of them to
    /// exit — which is the thing the test is about to reclaim. These tests read the
    /// registry and the process table instead, and let the output go.
    fn quiet(&self, args: &[&str], cwd: &Path) -> Command {
        let mut command = self.command(args, cwd);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        command
    }

    /// The registry, opened for reading what a command wrote.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// Make one unit and answer with its home and its materialisation.
    fn unit(&self, name: &str) -> (PathBuf, EnvId) {
        assert_ok(&self.nodal(&["new", "--name", name]));
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
        let unit = units::list(store.conn(), project.id)
            .unwrap()
            .into_iter()
            .find(|unit| unit.slug.as_str() == name)
            .expect("the unit was made");
        let environment =
            environments::latest_for_unit(store.conn(), unit.id).unwrap().expect("it has a home");
        (environment.home.clone(), environment.id)
    }

    /// A path in the scratch directory, which no home contains.
    fn scratch(&self, name: &str) -> PathBuf {
        self.scratch.join(name)
    }

    /// The tethers of one materialisation that are still open.
    fn tethers(&self, environment: EnvId) -> Vec<Session> {
        sessions::list_open_tethers(self.store().conn(), environment).unwrap()
    }
}

// ---------------------------------------------------------------------------
// Processes, as a person would look at them.
// ---------------------------------------------------------------------------

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

/// Wait for something to become true, and insist that it does.
fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        if ready() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("{what} did not happen within {TIMEOUT:?}");
}

/// Standard output as text, with the command insisted upon.
fn assert_ok(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// The JSON a `--json` command answered with.
fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&assert_ok(output)).unwrap()
}

/// The identifiers of one kind of target in one list of the stop report.
fn targets(report: &serde_json::Value, list: &str, kind: &str) -> Vec<u64> {
    report["stopped"][list]
        .as_array()
        .unwrap_or_else(|| panic!("no {list} in {report}"))
        .iter()
        .filter_map(|target| target.get(kind))
        .filter_map(serde_json::Value::as_u64)
        .collect()
}

/// The process identifiers a file holds, one to a line.
fn pids(path: &Path) -> Vec<u32> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

// ---------------------------------------------------------------------------
// The group dies whole.
// ---------------------------------------------------------------------------

/// A tethered command that starts a child which starts another child leaves both behind
/// when it exits, and the reclaim kills the whole group rather than the leader.
///
/// This is the development server, reduced to what makes it hard: the process
/// `nodal run` waited for is gone long before anybody reclaims anything, and the things
/// still running are two generations below it.
#[test]
fn a_tethered_group_is_dead_whole_after_the_unit_is_reclaimed() {
    let workspace = Workspace::new();
    let (home, environment) = workspace.unit("dev-server");
    let children = workspace.scratch("children");
    let script = workspace.scratch("server.sh");
    std::fs::write(
        &script,
        format!(
            "sleep 600 & printf '%s\\n' \"$!\" >> {file}\n\
             sh -c 'sleep 600 & printf \"%s\\n\" \"$!\" >> {file}' &\n\
             sleep 0.5\n",
            file = children.display()
        ),
    )
    .unwrap();

    let ran = workspace
        .quiet(&["run", "--tether", "sh", script.to_str().unwrap()], &home)
        .status()
        .unwrap();
    assert!(ran.success(), "the tethered command ran");

    let planted = pids(&children);
    assert_eq!(planted.len(), 2, "the script started a child and a grandchild");
    assert!(planted.iter().all(|pid| is_running(*pid)), "both outlived the nodal run");

    let open = workspace.tethers(environment);
    assert_eq!(open.len(), 1, "the unit holds one tether");
    let group = open[0].pgid.expect("the row records a process group");

    let report = json(&workspace.nodal(&["reclaim", "dev-server", "--json"]));
    assert_eq!(targets(&report, "asked", "group"), vec![u64::from(group)], "{report}");
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    for pid in planted {
        wait_for("the whole group to go", || !is_running(pid));
    }
    assert!(workspace.tethers(environment).is_empty(), "the row was given up with the unit");
}

// ---------------------------------------------------------------------------
// The ladder.
// ---------------------------------------------------------------------------

/// A tethered command that stops when it is interrupted is never sent `SIGTERM`.
///
/// The command writes the name of each signal it is sent from its own handler, so the
/// file is written by the process under test rather than inferred from the outside.
#[test]
fn a_tether_that_stops_on_the_interrupt_is_never_sent_the_next_signal() {
    let workspace = Workspace::new();
    let (home, environment) = workspace.unit("polite-server");
    let signals = workspace.scratch("signals");
    let ready = workspace.scratch("ready");
    let script = workspace.scratch("polite.sh");
    std::fs::write(
        &script,
        format!(
            "trap 'printf INT >> {log}; exit 0' INT\n\
             trap 'printf TERM >> {log}' TERM\n\
             : > {ready}\n\
             while : ; do sleep 0.1 ; done\n",
            log = signals.display(),
            ready = ready.display()
        ),
    )
    .unwrap();

    let mut run = workspace
        .quiet(&["run", "--tether", "sh", script.to_str().unwrap()], &home)
        .spawn()
        .unwrap();
    wait_for("the tethered command to start", || ready.exists());
    wait_for("the tether to be recorded", || !workspace.tethers(environment).is_empty());

    let report = json(&workspace.nodal(&["reclaim", "polite-server", "--json"]));
    assert_eq!(targets(&report, "asked", "group").len(), 1, "{report}");
    assert!(targets(&report, "killed", "group").is_empty(), "it was never killed: {report}");
    run.wait().unwrap();

    let seen = std::fs::read_to_string(&signals).unwrap();
    assert_eq!(seen, "INT", "the tether saw the interrupt and nothing after it");
}

// ---------------------------------------------------------------------------
// The parent is not the record.
// ---------------------------------------------------------------------------

/// A tether whose `nodal run` was killed outright is still found and still stopped.
///
/// The registry row is what a reclaim reads. Nothing asks the parent, because in the
/// case that matters there is no parent left to ask.
#[test]
fn a_tether_outlives_the_nodal_run_that_started_it_and_is_still_stopped() {
    let workspace = Workspace::new();
    let (home, environment) = workspace.unit("orphan-server");
    let mut run = workspace.quiet(&["run", "--tether", "sleep", "600"], &home).spawn().unwrap();
    wait_for("the tether to be recorded", || !workspace.tethers(environment).is_empty());
    let group = workspace.tethers(environment)[0].pgid.expect("the row records a group");

    kill_hard(&mut run);
    assert!(is_running(group), "the tethered command outlived its parent");

    let report = json(&workspace.nodal(&["reclaim", "orphan-server", "--json"]));
    assert_eq!(targets(&report, "asked", "group"), vec![u64::from(group)], "{report}");
    assert!(report["leftovers"].as_array().unwrap().is_empty(), "{report}");
    wait_for("the orphaned tether to go", || !is_running(group));
}

/// Kill a `nodal run` outright and wait for it to leave the table.
fn kill_hard(run: &mut Child) {
    let pid = run.id();
    assert!(
        Command::new("kill").args(["-9", &pid.to_string()]).status().unwrap().success(),
        "the parent was killed"
    );
    run.wait().unwrap();
}

// ---------------------------------------------------------------------------
// What the tether does not touch.
// ---------------------------------------------------------------------------

/// An untethered process in the home is left to attribution, which stops it as itself.
///
/// Both are running in the same home and both are stopped, but they are stopped as
/// different kinds of target: the tether as a group, because the registry recorded one,
/// and the other as one process, because a scan is all there is to go on. The untethered
/// process never gets a row, and never joins anybody's group.
#[test]
fn an_untethered_process_in_the_home_is_stopped_by_attribution_and_not_by_the_tether() {
    let workspace = Workspace::new();
    let (home, environment) = workspace.unit("mixed");
    let plain = workspace.scratch("plain.pid");
    let script = workspace.scratch("plain.sh");
    std::fs::write(&script, format!("sleep 600 & printf '%s\\n' \"$!\" > {}\n", plain.display()))
        .unwrap();

    // Untethered: it runs in this test's own process group, and only the identifier it
    // carries says which unit it is in.
    let ran = workspace.quiet(&["run", "sh", script.to_str().unwrap()], &home).status().unwrap();
    assert!(ran.success(), "the untethered command ran");
    let untethered = pids(&plain);
    assert_eq!(untethered.len(), 1);
    assert!(is_running(untethered[0]));

    let mut run = workspace.quiet(&["run", "--tether", "sleep", "600"], &home).spawn().unwrap();
    wait_for("the tether to be recorded", || !workspace.tethers(environment).is_empty());
    let open = workspace.tethers(environment);
    assert_eq!(open.len(), 1, "only the tethered run wrote a row");
    let group = open[0].pgid.expect("the row records a group");
    assert_ne!(group, untethered[0], "the untethered process is in nobody's tether");

    let report = json(&workspace.nodal(&["reclaim", "mixed", "--json"]));
    run.wait().unwrap();
    assert_eq!(targets(&report, "asked", "group"), vec![u64::from(group)], "{report}");
    if cfg!(target_os = "linux") {
        assert!(
            targets(&report, "asked", "process").contains(&u64::from(untethered[0])),
            "attribution stopped it as one process: {report}"
        );
        wait_for("the untethered process to go", || !is_running(untethered[0]));
    } else {
        // No process table to attribute with. The tether is still stopped by identifier,
        // and the process the host could not see is the host's to answer for.
        let _ = Command::new("kill").args(["-9", &untethered[0].to_string()]).status();
    }
    wait_for("the tether to go", || !is_running(group));
}

/// A tether that outlived the reclaim of its unit is stopped by the next `nodal gc`.
///
/// This is the second half of the promise. A reclaim that could not read the machine,
/// or one that was killed before it got there, leaves a group with an open row and an
/// environment that is gone. `gc` stops exactly those, and never the tether of a unit
/// somebody is still working in.
///
/// The state is written by hand, because the operation that would produce it is the one
/// that fails: a reclaim on this host stops the tether itself, so the only way to give
/// `gc` something to do is to put the registry in the state a failed reclaim leaves.
#[test]
fn a_tether_left_behind_by_a_reclaimed_unit_is_stopped_by_the_next_sweep() {
    let workspace = Workspace::new();
    let (home, environment) = workspace.unit("stale");
    let mut run = workspace.quiet(&["run", "--tether", "sleep", "600"], &home).spawn().unwrap();
    wait_for("the tether to be recorded", || !workspace.tethers(environment).is_empty());
    let group = workspace.tethers(environment)[0].pgid.expect("the row records a group");
    kill_hard(&mut run);

    // What a reclaim that never reached its stop leaves: the home gone as far as the
    // registry is concerned, and the tether's row still open.
    let store = workspace.store();
    environments::update_state(store.conn(), environment, EnvState::Absent, Timestamp::now())
        .unwrap();
    drop(store);
    assert!(is_running(group), "the tether is still there for the sweep to find");

    let swept = json(&workspace.nodal(&["gc", "--json"]));
    assert_eq!(targets(&swept, "asked", "group"), vec![u64::from(group)], "{swept}");
    wait_for("the stale tether to go", || !is_running(group));
    assert!(workspace.tethers(environment).is_empty(), "the sweep gave the row up as well");
}

/// A tether is refused in a home the registry does not know, because the row is the only
/// record a group would ever have.
#[test]
fn a_tether_is_refused_where_nothing_could_record_the_group() {
    let workspace = Workspace::new();
    let (home, _) = workspace.unit("unknown");
    let empty = workspace.scratch("other-registry.db");

    let mut command = workspace.command(&["run", "--tether", "sleep", "600"], &home);
    command.env("NODAL_STORE", &empty);
    let refused = command.output().unwrap();

    assert!(!refused.status.success(), "a tether nothing records is refused");
    let why = String::from_utf8(refused.stderr).unwrap();
    assert!(why.contains("--tether"), "{why}");
    assert!(why.contains("record"), "{why}");

    // The same command without the flag still runs, and still loses only its event.
    let mut plain = workspace.command(&["run", "true"], &home);
    plain.env("NODAL_STORE", &empty);
    assert!(plain.output().unwrap().status.success(), "an untethered run carries on");
}
