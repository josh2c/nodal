//! Acceptance test for the Claude Code integration, exercised the way Claude Code
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
use nodal_core::store::events;
use nodal_safety::{InState as _, Workspace, git};

/// The shell a hook command runs in, named by its path.
const SHELL: &str = "/bin/sh";

/// How long the answer to a missing binary may take. It is a refusal, not a wait: the
/// point of the test is that a person meets a message rather than a hung session.
const REFUSAL_TIMEOUT: Duration = Duration::from_secs(30);

/// A project, its state directory, and a home directory belonging to nobody.
///
/// The home directory is the reason this suite gives its commands one more variable
/// than the shared fixture does: the hooks this suite installs are read out of a user's
/// own settings, and no test may read the ones belonging to whoever is running it.
fn project() -> Workspace {
    let workspace = Workspace::new(state::BINARY);
    let home = workspace.root().join("home");
    std::fs::create_dir_all(&home).unwrap();
    workspace.with_env("HOME", &home).with_env("USERPROFILE", &home)
}

/// The same, with a recipe written and the hooks installed.
fn initialised_project() -> Workspace {
    let project = project();
    succeed(&project.nodal(&["init", "--claude-hooks"]));
    project
}

/// The readings this suite needs beyond the ones the shared fixture has.
trait Hooked {
    /// Answer one hook, with `payload` on standard input, as Claude Code does.
    fn hook(&self, event: &str, payload: &str) -> Output;

    /// The settings file this project's hooks are in.
    fn settings(&self) -> PathBuf;

    /// The one unit this project has.
    fn unit_row(&self) -> nodal_core::model::Unit;
}

impl Hooked for Workspace {
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

    fn settings(&self) -> PathBuf {
        self.source.join(".claude").join("settings.json")
    }

    fn unit_row(&self) -> nodal_core::model::Unit {
        self.one_unit()
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
    let project = initialised_project();
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
    let project = initialised_project();
    succeed(&project.hook("worktree-create", &create_payload(&project.source)));

    let unit = project.unit_row();
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
    let project = project();
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
    let project = initialised_project();
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
    let project = initialised_project();
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
    let project = initialised_project();
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
    let project = initialised_project();
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
    let project = initialised_project();
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
    let handoff = events::list_for_unit(store.conn(), project.unit_row().id)
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
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let before =
        events::list_for_unit(project.store().conn(), project.unit_row().id).unwrap().len();
    let payload = format!(
        "{{\"session_id\":\"a\",\"transcript_path\":\"\",\"cwd\":\"{}\",\
         \"hook_event_name\":\"Stop\"}}",
        home.display()
    );

    let output = project.hook("stop", &payload);
    assert!(output.status.success(), "a desktop session ending was reported as a failure");
    assert!(output.stdout.is_empty() && output.stderr.is_empty(), "it said something: {output:?}");
    assert_eq!(
        events::list_for_unit(project.store().conn(), project.unit_row().id).unwrap().len(),
        before,
        "silence was recorded as a handoff"
    );
}

#[test]
fn a_worktree_removal_removes_nothing_and_the_unit_outlives_the_session() {
    let project = initialised_project();
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
fn the_home_the_provider_answers_with_carries_the_hooks_the_session_will_run() {
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let carried = home.join(".claude").join("settings.json");
    assert!(
        carried.is_file(),
        "the session moves into {home:?}, and the file that declares the hooks stayed behind"
    );
    let installed = std::fs::read_to_string(&carried).unwrap();
    assert_eq!(
        installed,
        std::fs::read_to_string(project.settings()).unwrap(),
        "the home was given hooks the project does not declare"
    );
}

#[test]
fn the_home_is_given_the_projects_own_settings_and_not_a_regenerated_four() {
    let project = project();
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n";
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();
    succeed(&project.nodal(&["init", "--claude-hooks"]));
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let carried = std::fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    assert_eq!(
        carried,
        std::fs::read_to_string(project.settings()).unwrap(),
        "the home was given settings the project does not declare"
    );
    assert!(
        carried.contains("Bash(rm:*)"),
        "the project's deny rule stopped applying the moment the session moved: {carried}"
    );
}

#[test]
fn a_settings_file_the_project_commits_is_left_exactly_as_the_clone_carried_it() {
    let project = project();
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n";
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();
    git(&project.source, &["add", "--", ".claude/settings.json"]);
    git(&project.source, &["commit", "--quiet", "--message", "the project's own settings"]);
    succeed(&project.nodal(&["init", "--claude-hooks"]));

    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let carried = home.join(".claude").join("settings.json");
    assert_eq!(
        std::fs::read_to_string(&carried).unwrap(),
        theirs,
        "a file git tracks was rewritten, so the home is in git status before the session starts"
    );
    assert!(
        git(&home, &["status", "--porcelain"]).is_empty(),
        "the home is dirty the moment the session was given it"
    );
}

/// The advice a hookless tracked file gets has to be advice that works. `nodal init
/// --claude-hooks` writes the project's working file, which the home's copy came from a
/// commit of: running it changes nothing for this unit or the next one cloned. The two
/// things that do work are committing the hooks and putting them in the person's own
/// settings, and the note says both.
#[test]
fn a_tracked_settings_file_with_no_hooks_in_it_is_one_note_saying_what_to_do() {
    let project = project();
    let home = tracked_settings(
        &project,
        "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n",
    );
    let said = notes(&project).join("\n");

    assert!(
        said.contains(".claude/settings.json"),
        "nothing in the log says why this unit will record nothing: {said}"
    );
    assert!(
        said.contains("Commit the hooks") && said.contains("your own settings"),
        "the note does not say what would work: {said}"
    );
    assert!(
        !said.contains("nodal init --claude-hooks"),
        "the note sends a person to a command that is a no-op in this state: {said}"
    );
    assert!(home.is_dir());
}

/// The same note reaches the person watching the session start, not only the log they
/// may never open.
#[test]
fn a_hookless_settings_file_is_said_on_standard_error_as_well_as_recorded() {
    let project = project();
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n";
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();
    git(&project.source, &["add", "--", ".claude/settings.json"]);
    git(&project.source, &["commit", "--quiet", "--message", "the project's own settings"]);
    succeed(&project.nodal(&["init", "--claude-hooks"]));

    let created = project.hook("worktree-create", &create_payload(&project.source));
    assert!(created.status.success(), "{}", stderr(&created));
    let said = stderr(&created);
    assert!(
        said.contains(".claude/settings.json") && said.contains("Commit the hooks"),
        "the person who started the session was told nothing: {said}"
    );
}

/// Commit a settings file, install the hooks, and make one unit of the project.
fn tracked_settings(project: &Workspace, theirs: &str) -> PathBuf {
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();
    git(&project.source, &["add", "--", ".claude/settings.json"]);
    git(&project.source, &["commit", "--quiet", "--message", "the project's own settings"]);
    succeed(&project.nodal(&["init", "--claude-hooks"]));
    PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    )
}

/// Every note event this project's one unit carries.
fn notes(project: &Workspace) -> Vec<String> {
    let store = project.store();
    events::list_for_unit(store.conn(), project.unit_row().id)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == EventKind::Note)
        .map(|event| event.body)
        .collect()
}

#[test]
fn an_uninstall_empties_the_hook_region_of_every_home_as_well_as_the_project() {
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let carried = home.join(".claude").join("settings.json");
    assert!(carried.is_file(), "the home was given no hooks to lose");

    succeed(&project.nodal(&["uninstall", "--yes"]));

    let left = std::fs::read_to_string(&carried).unwrap_or_default();
    assert!(
        !left.contains("nodal claude-code"),
        "a home was left running a hook for a binary the person removed: {left}"
    );
}

#[test]
fn a_request_from_inside_a_home_answers_that_home_and_registers_no_project() {
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let again = succeed(&project.hook("worktree-create", &create_payload(&home)));

    assert_eq!(Path::new(again.trim()), home, "the session was sent out of the home it is in");
    assert_eq!(project.homes().len(), 1, "a unit of a unit was cloned: {:?}", project.homes());
}

#[test]
fn the_memory_answers_the_command_the_home_itself_declares() {
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let installed = std::fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    let command = observer_command(&installed, "session-start");

    let payload = start_payload(&home, "a-session");
    let answered = run_hook(&project, &command, &home, &payload);

    assert!(answered.status.success(), "{}", stderr(&answered));
    assert!(
        String::from_utf8(answered.stdout).unwrap().contains("say-hi-6fac65"),
        "the command the home declares injected no memory"
    );
}

#[test]
fn a_session_that_took_a_home_is_recorded_as_attached_to_its_unit() {
    let project = initialised_project();
    succeed(&project.hook("worktree-create", &create_payload(&project.source)));

    let store = project.store();
    let unit = project.unit_row();
    let attached = events::list_for_unit(store.conn(), unit.id)
        .unwrap()
        .into_iter()
        .find(|event| event.kind == EventKind::Attached)
        .expect("the session that made the unit was not recorded as attached to it");
    assert_eq!(attached.actor.name.as_str(), "claude-code");
    assert_eq!(
        attached.epistemic,
        Epistemic::Observed,
        "nodal watched the hook make this home, so the record is not a claim"
    );
}

#[test]
fn the_settings_file_names_no_directory_of_this_machine() {
    let project = initialised_project();
    let installed = std::fs::read_to_string(project.settings()).unwrap();

    for absent in [project.source.display().to_string(), project.state.display().to_string()] {
        assert!(!installed.contains(&absent), "{absent} is in a file a person may commit");
    }
    assert!(installed.contains("nodal claude-code"), "{installed}");
    assert_eq!(installed.matches("\"type\": \"command\"").count(), 4, "{installed}");
}

#[test]
fn the_hooks_go_in_and_come_out_and_leave_the_file_they_found() {
    let project = project();
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
    let project = initialised_project();
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

/// A home whose settings file the project commits is the project's file, not Nodal's.
/// An uninstall that rewrites or removes it leaves every home of that project modified
/// from birth, and ships the removal in the pull request the unit opens. The project's
/// own copy is still cleaned, one directory up.
#[test]
fn an_uninstall_leaves_a_home_whose_settings_the_project_commits_alone() {
    let project = initialised_project();
    git(&project.source, &["add", "--", ".claude/settings.json"]);
    git(&project.source, &["commit", "--quiet", "--message", "commit the hooks"]);
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let carried = home.join(".claude").join("settings.json");
    let before = std::fs::read_to_string(&carried).unwrap();
    assert!(before.contains("nodal claude-code"), "the clone carried no hooks to lose");

    succeed(&project.nodal(&["uninstall", "--yes"]));

    assert_eq!(
        std::fs::read_to_string(&carried).unwrap(),
        before,
        "a file git tracks was rewritten inside a home, so the home is dirty for ever"
    );
    let status = git(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "the uninstall left the home dirty: {status}");
}

/// The survey visits every project and every home on the machine. One file it cannot
/// read is one line saying so, not the end of the whole answer: a person still gets to
/// remove everything else.
#[test]
fn a_settings_file_that_cannot_be_read_is_one_note_and_not_the_end_of_the_survey() {
    let project = initialised_project();
    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );
    let carried = home.join(".claude").join("settings.json");
    std::fs::remove_file(&carried).unwrap();
    std::fs::create_dir(&carried).unwrap();

    let done = project.nodal(&["uninstall", "--yes"]);

    assert!(
        done.status.success(),
        "one unreadable file ended the whole uninstall: {}",
        stderr(&done)
    );
    let installed = std::fs::read_to_string(project.settings()).unwrap_or_default();
    assert!(
        !installed.contains("nodal claude-code"),
        "the project kept its hooks because one home could not be read: {installed}"
    );
    let said = format!("{}{}", succeed(&done), stderr(&done));
    assert!(
        said.contains("could not be read"),
        "nothing said which file was not looked at: {said}"
    );
}

/// A project file that declares none of Nodal's hooks is still what the home gets: the
/// hooks may be in the person's own settings, which is how the session reached the
/// provider at all. What must not happen is that it arrives silently, because the
/// observers then fire from somewhere this unit's log cannot name.
#[test]
fn an_untracked_project_file_with_no_hooks_is_carried_and_said() {
    let project = project();
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  }\n}\n";
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), theirs).unwrap();
    succeed(&project.nodal(&["init"]));

    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    assert_eq!(
        std::fs::read_to_string(home.join(".claude").join("settings.json")).unwrap(),
        theirs,
        "the home was given settings the project does not declare"
    );
    let said = notes(&project).join("\n");
    assert!(
        said.contains("declares none of nodal's hooks"),
        "a home that observes nothing was given one silently: {said}"
    );
}

/// A settings file holding only whitespace is no settings file. Copied verbatim it is a
/// document Claude Code cannot read, so the home would declare nothing at all.
#[test]
fn a_project_settings_file_holding_only_whitespace_is_treated_as_none() {
    let project = project();
    std::fs::create_dir_all(project.source.join(".claude")).unwrap();
    std::fs::write(project.settings(), "   \n\n").unwrap();
    succeed(&project.nodal(&["init"]));

    let home = PathBuf::from(
        succeed(&project.hook("worktree-create", &create_payload(&project.source)))
            .trim()
            .to_owned(),
    );

    let carried = std::fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    assert!(
        carried.contains("nodal claude-code"),
        "the home was given a document Claude Code cannot read: {carried:?}"
    );
    assert_eq!(carried.matches("\"type\": \"command\"").count(), 4, "{carried}");
}

/// A home Claude Code would not accept ends the session. Nothing durable may be written
/// about a session that never began: no memory, no settings file, and above all no
/// event saying an agent took this home.
#[test]
fn a_home_claude_would_not_accept_is_refused_before_anything_records_it() {
    let project = project();
    let state = format!("{}/./state", project.root().display());
    let project = project.with_env("NODAL_HOME", &state);
    succeed(&project.nodal(&["init", "--claude-hooks"]));

    let refused = project.hook("worktree-create", &create_payload(&project.source));

    assert!(!refused.status.success(), "a home with a dot segment in it was answered with");
    assert_eq!(String::from_utf8(refused.stdout.clone()).unwrap().trim(), REFUSED);
    let attached: Vec<String> =
        events::list_for_unit(project.store().conn(), project.unit_row().id)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == EventKind::Attached)
            .map(|event| event.body)
            .collect();
    assert!(
        attached.is_empty(),
        "a session that never began is recorded as having taken this home: {attached:?}"
    );
    let home = project.homes().into_iter().next().expect("the unit was made");
    assert!(
        !home.join("WORKUNIT.md").exists() && !home.join(".claude").exists(),
        "a home nobody was sent to was furnished for a session"
    );
}

/// Run one installed command in `directory`, with `payload` on standard input.
///
/// It is the same shell Claude Code runs a hook in, and the command is read out of the
/// settings file rather than written by the test, so what is exercised is what a
/// session would really run.
fn run_hook(project: &Workspace, command: &str, directory: &Path, payload: &str) -> Output {
    let mut child = Command::new(SHELL)
        .arg("-c")
        .arg(command)
        .current_dir(directory)
        .env("PATH", on_path(project))
        .env("NODAL_HOME", &project.state)
        .env("HOME", project.root().join("home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// A `PATH` holding one directory, in which the binary under test is called `nodal`.
///
/// The installed command is `command -v nodal`, so a test that runs the real command
/// has to make the real name findable. Nothing else is on the path: what the command
/// finds is the build being tested and no other Nodal on the machine.
fn on_path(project: &Workspace) -> PathBuf {
    let directory = project.root().join("bin");
    std::fs::create_dir_all(&directory).unwrap();
    let link = directory.join("nodal");
    if !link.exists() {
        std::os::unix::fs::symlink(state::BINARY, &link).unwrap();
    }
    directory
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
