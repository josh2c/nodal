//! Acceptance test for T2.7: the Claude Code integration, exercised the way Claude Code
//! exercises it.
//!
//! Every test here feeds a payload on standard input and reads standard output, because
//! that is the whole of the contract between the two programs. The payloads are the
//! ones the desktop application was measured sending: a `WorktreeCreate` with an empty
//! `transcript_path`, no scratchpad directory and a slug derived from the opening
//! prompt, and a `SessionStart` whose session identifier is not the one the create
//! carried. Nothing here correlates by that identifier, and the test that fires
//! `SessionStart` twice with two different ones is what keeps it that way.
//!
//! The four events are not four of a kind, and the tests are not either:
//!
//! - `WorktreeCreate` is a **provider**. Claude reads a directory from its standard
//!   output and ends the session when it does not get one, so both answers are tested:
//!   the home of a unit that was really made, and the refusal a project with no recipe
//!   gets. The refusal is checked to be a path Claude rejects rather than an empty line,
//!   and the case where `nodal` is not installed at all is run through the command text
//!   that is really written into `.claude/settings.json`, with an empty `PATH`.
//! - `SessionStart` prints a memory inside a unit home and nothing anywhere else.
//! - `Stop` records a handoff where there is a message to record, and is silent where
//!   the desktop application sent none.
//! - `WorktreeRemove` never fired in four measured session lifecycles. It is installed
//!   anyway, and the test says what it must do if it ever fires: nothing that removes
//!   anything.
//!
//! The last two tests are about the file. It goes into a project a person may commit,
//! so it names no path of this machine; and an install followed by an uninstall leaves
//! it byte for byte the file it was, including the hooks somebody else had put in it.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use nodal_core::adapters::claude_code::REFUSED;
use nodal_core::model::{Epistemic, EventKind};
use nodal_core::store::{Store, events, projects, units};
use tempfile::TempDir;

/// The shell a hook command runs in, named by its path.
const SHELL: &str = "/bin/sh";

/// How long the answer to a missing binary may take. It is a refusal, not a wait: the
/// point of the test is that a person meets a message rather than a hung session.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(30);

/// A project, its state directory, and a home directory belonging to nobody.
struct Project {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The project's repository.
    source: PathBuf,
    /// Nodal's state directory.
    state: PathBuf,
    /// What `$HOME` is for every command, so that no test reads the start-up files of
    /// whoever is running it.
    home: PathBuf,
}

impl Project {
    /// A one-commit repository, with no recipe yet.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
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
        Self { _root: root, source, state, home }
    }

    /// The same, with a recipe written and the hooks installed.
    fn initialised() -> Self {
        let project = Self::new();
        succeed(&project.nodal(&["init", "--claude-hooks"]));
        project
    }

    /// `nodal` with this project's state directory, run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The same invocation, not yet run.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = state::nodal(&self.state);
        command.args(args).current_dir(&self.source);
        command.env("HOME", &self.home).env("USERPROFILE", &self.home);
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command
    }

    /// Answer one hook, with `payload` on standard input, as Claude Code does.
    fn hook(&self, event: &str, payload: &str) -> Output {
        let mut child = self
            .command(&["claude-code", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    /// The settings file this project's hooks are in.
    fn settings(&self) -> PathBuf {
        self.source.join(".claude").join("settings.json")
    }

    /// The registry, opened for reading what a hook wrote.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// The one unit this project has.
    fn unit(&self) -> nodal_core::model::Unit {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
        units::list(store.conn(), project.id).unwrap().pop().expect("a unit was made")
    }
}

/// The payload the desktop application sends when it makes a worktree.
fn create_payload(cwd: &Path) -> String {
    format!(
        "{{\"session_id\":\"65c3d11f-676b-40d3-962a-d4f001021ba4\",\"transcript_path\":\"\",\
         \"cwd\":\"{}\",\"hook_event_name\":\"WorktreeCreate\",\"name\":\"say-hi-6fac65\"}}",
        cwd.display()
    )
}

/// The payload a session start sends. `session` is the identifier, which differs from
/// the create's and from the other start's, and which nothing reads.
fn start_payload(cwd: &Path, session: &str) -> String {
    format!(
        "{{\"session_id\":\"{session}\",\"transcript_path\":\"\",\"cwd\":\"{}\",\
         \"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}}",
        cwd.display()
    )
}

/// Standard output as text, with the command insisted upon.
fn succeed(output: &Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// Standard error as text.
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn the_provider_makes_a_unit_and_answers_with_a_home_claude_would_accept() {
    let project = Project::initialised();
    let answered = succeed(&project.hook("worktree-create", &create_payload(&project.source)));

    let lines: Vec<&str> = answered.lines().collect();
    assert_eq!(lines.len(), 1, "the provider printed more than the one path it may print");
    let home = Path::new(lines[0]);
    assert!(home.is_absolute(), "{home:?} is not absolute, so the session would end");
    assert!(
        !answered.contains("/./") && !answered.contains("/../"),
        "{answered:?} carries a dot segment"
    );
    assert!(home.is_dir(), "{home:?} was printed and is not there");
    assert!(home.join(".nodal").join("id").is_file(), "{home:?} is not a unit home");
}

#[test]
fn the_slug_claude_derived_becomes_the_unit_and_is_marked_recovered() {
    let project = Project::initialised();
    succeed(&project.hook("worktree-create", &create_payload(&project.source)));

    let unit = project.unit();
    assert_eq!(unit.slug.as_str(), "say-hi-6fac65", "the payload's own slug was not used");
    assert_eq!(
        unit.objective.as_ref().map(nodal_core::model::Objective::as_str),
        Some("say-hi-6fac65")
    );
    assert_eq!(
        unit.objective_epistemic,
        Some(Epistemic::Observed),
        "a slug Claude derived from a prompt is a reading of an intent, not a statement of one"
    );
}

#[test]
fn a_project_with_no_recipe_is_refused_with_a_path_claude_would_reject() {
    let project = Project::new();
    let refused = project.hook("worktree-create", &create_payload(&project.source));

    assert!(!refused.status.success(), "a project with no recipe made a unit anyway");
    assert_eq!(
        String::from_utf8(refused.stdout.clone()).unwrap().trim(),
        REFUSED,
        "the refusal was not the one Claude Code rejects"
    );
    let said = stderr(&refused);
    assert!(
        said.contains("nodal.toml"),
        "the reason did not name the file that is missing: {said}"
    );
    assert!(said.contains("nodal init"), "the reason did not say what to do: {said}");
}

#[test]
fn a_machine_with_no_nodal_refuses_at_once_rather_than_hanging() {
    let project = Project::initialised();
    let installed = std::fs::read_to_string(project.settings()).unwrap();
    let command = provider_command(&installed);

    let started = Instant::now();
    let output = without_nodal(&command, &project.source);
    let took = started.elapsed();

    assert!(took < REFUSAL_TIMEOUT, "the hook took {took:?}, which is a session hanging");
    assert!(!output.status.success(), "a missing binary reported success");
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), REFUSED);
    assert!(
        !String::from_utf8(output.stderr).unwrap().is_empty(),
        "nothing was said about why the session could not have a unit"
    );
}

#[test]
fn an_observer_on_a_machine_with_no_nodal_says_nothing_and_succeeds() {
    let project = Project::initialised();
    let installed = std::fs::read_to_string(project.settings()).unwrap();
    let command = observer_command(&installed, "session-start");

    let output = without_nodal(&command, &project.source);

    assert!(output.status.success(), "a session start failed because Nodal was not installed");
    assert!(
        output.stdout.is_empty(),
        "it injected something into a session it knows nothing about"
    );
}

#[test]
fn the_memory_is_printed_inside_a_unit_home_and_nowhere_else() {
    let project = Project::initialised();
    let home = succeed(&project.hook("worktree-create", &create_payload(&project.source)));
    let home = PathBuf::from(home.trim());

    let inside = succeed(&project.hook("session-start", &start_payload(&home, "a-session")));
    assert!(inside.contains("say-hi-6fac65"), "the memory did not name the unit: {inside}");
    assert_eq!(
        inside,
        std::fs::read_to_string(home.join("WORKUNIT.md")).unwrap(),
        "what was injected is not the file"
    );

    let outside =
        succeed(&project.hook("session-start", &start_payload(&project.source, "a-session")));
    assert!(outside.is_empty(), "a session in the checkout was given a unit's memory: {outside}");
}

#[test]
fn two_session_identifiers_over_one_path_are_one_answer() {
    let project = Project::initialised();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let first = succeed(&project.hook("session-start", &start_payload(&home, "8e18d129-root")));
    let second =
        succeed(&project.hook("session-start", &start_payload(&home, "b7c41f02-worktree")));
    assert_eq!(first, second, "the answer depended on the session identifier");
    assert!(!first.is_empty(), "neither firing said anything");
}

#[test]
fn a_stop_that_carries_a_message_states_a_handoff() {
    let project = Project::initialised();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let payload = format!(
        "{{\"session_id\":\"a\",\"transcript_path\":\"\",\"cwd\":\"{}\",\
         \"hook_event_name\":\"Stop\",\
         \"last_assistant_message\":\"the parser still fails on two-digit years\"}}",
        home.display()
    );

    let said = succeed(&project.hook("stop", &payload));
    assert!(said.is_empty(), "the stop hook printed into a session that was ending: {said}");

    let store = project.store();
    let handoff = events::list_for_unit(store.conn(), project.unit().id)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == EventKind::Handoff)
        .expect("no handoff was recorded");
    assert_eq!(
        handoff.epistemic,
        Epistemic::Stated,
        "an agent's closing claim was recorded as fact"
    );
    assert_eq!(handoff.body, "the parser still fails on two-digit years");
    assert_eq!(handoff.actor.name.as_str(), "claude-code");
}

#[test]
fn a_desktop_stop_that_carries_neither_message_nor_transcript_is_silent() {
    let project = Project::initialised();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let before = events::list_for_unit(project.store().conn(), project.unit().id).unwrap().len();
    let payload = format!(
        "{{\"session_id\":\"a\",\"transcript_path\":\"\",\"cwd\":\"{}\",\
         \"hook_event_name\":\"Stop\"}}",
        home.display()
    );

    let output = project.hook("stop", &payload);
    assert!(output.status.success(), "a desktop session ending was reported as a failure");
    assert!(output.stdout.is_empty() && output.stderr.is_empty(), "it said something: {output:?}");
    assert_eq!(
        events::list_for_unit(project.store().conn(), project.unit().id).unwrap().len(),
        before,
        "silence was recorded as a handoff"
    );
}

#[test]
fn a_worktree_removal_removes_nothing_and_the_unit_outlives_the_session() {
    let project = Project::initialised();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let payload = format!(
        "{{\"session_id\":\"a\",\"cwd\":\"{}\",\"hook_event_name\":\"WorktreeRemove\"}}",
        home.display()
    );

    let output = project.hook("worktree-remove", &payload);
    assert!(output.status.success());
    assert!(home.is_dir(), "the home went when a session let go of it");
    assert!(
        succeed(&project.nodal(&["ls"])).contains("say-hi-6fac65"),
        "the unit stopped being listed when its session ended"
    );
}

#[test]
fn the_settings_file_names_no_directory_of_this_machine() {
    let project = Project::initialised();
    let installed = std::fs::read_to_string(project.settings()).unwrap();

    for absent in [project.source.display().to_string(), project.state.display().to_string()] {
        assert!(!installed.contains(&absent), "{absent} is in a file a person may commit");
    }
    assert!(installed.contains("nodal claude-code"), "{installed}");
    assert_eq!(installed.matches("\"type\": \"command\"").count(), 4, "{installed}");
}

#[test]
fn the_hooks_go_in_and_come_out_and_leave_the_file_they_found() {
    let project = Project::new();
    let theirs = "{\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"hooks\": [\n          \
                  {\n            \"type\": \"command\",\n            \"command\": \"./audit.sh\"\n \
                           }\n        ]\n      }\n    ]\n  }\n}\n";
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();

    succeed(&project.nodal(&["init", "--claude-hooks"]));
    let installed = std::fs::read_to_string(project.settings()).unwrap();
    assert!(installed.contains("./audit.sh"), "somebody else's hook went: {installed}");
    assert!(installed.contains("nodal claude-code"), "{installed}");

    succeed(&project.nodal(&["uninstall", "--yes"]));
    assert_eq!(
        std::fs::read_to_string(project.settings()).unwrap(),
        theirs,
        "the file did not come back byte for byte"
    );
}

#[test]
fn a_second_install_writes_the_same_file_and_a_removal_takes_the_file_it_made() {
    let project = Project::initialised();
    let once = std::fs::read_to_string(project.settings()).unwrap();
    succeed(&project.nodal(&["init", "--force", "--claude-hooks"]));
    assert_eq!(
        std::fs::read_to_string(project.settings()).unwrap(),
        once,
        "a second install wrote again"
    );

    succeed(&project.nodal(&["uninstall", "--yes"]));
    assert!(
        !project.settings().exists(),
        "a settings file that held nothing but nodal's hooks was left behind"
    );
    assert!(!project.source.join(".claude").exists(), "the directory nodal made was left behind");
}

/// Run one of the installed commands on a machine that has no `nodal`.
///
/// The shell is named by its own path, because the `PATH` the command is given has
/// nothing on it: that is the condition being tested, and a shell found on it would not
/// be.
fn without_nodal(command: &str, directory: &Path) -> Output {
    Command::new(SHELL)
        .arg("-c")
        .arg(command)
        .env("PATH", "/nonexistent")
        .current_dir(directory)
        .output()
        .unwrap()
}

/// The command Claude Code would run for the provider event, out of the file itself.
fn provider_command(settings: &str) -> String {
    command_of(settings, "worktree-create")
}

/// The command Claude Code would run for one observer, out of the file itself.
fn observer_command(settings: &str, verb: &str) -> String {
    command_of(settings, verb)
}

/// One command out of the settings file, read as JSON so the test reads what Claude
/// reads rather than what Nodal meant to write.
fn command_of(settings: &str, verb: &str) -> String {
    let document: serde_json::Value = serde_json::from_str(settings).unwrap();
    let hooks = document.get("hooks").and_then(serde_json::Value::as_object).unwrap();
    for groups in hooks.values() {
        for group in groups.as_array().into_iter().flatten() {
            for hook in
                group.get("hooks").and_then(serde_json::Value::as_array).into_iter().flatten()
            {
                let command = hook.get("command").and_then(serde_json::Value::as_str).unwrap_or("");
                if command.contains(verb) {
                    return command.to_owned();
                }
            }
        }
    }
    panic!("the settings file holds no command for {verb}: {settings}");
}
