//! Acceptance test for `nodal adopt`, end to end, against a real repository.
//!
//! Adoption is the command that touches something Nodal did not make, so every
//! assertion here is about what it did *not* do to a person's own directory.
//!
//! The one that matters most is the first: a worktree adopted in place has the same
//! `git status` afterwards, byte for byte, as it had before — and still has it after a
//! `nodal show` has written the unit's memory into it. That is the whole promise of the
//! in-place form, and it is checked against the text a person would read rather than
//! against a list of files, because the text is what they would notice changing.
//!
//! The second is the one the feature exists for. A nested worktree an agent tool made
//! says nothing about itself; the tool's own session record does, and the unit comes out
//! of the adoption carrying that intent, marked as recovered rather than stated.
//!
//! The third is the other end of the promise. A root is never trashed: a reclaim gives
//! up the registration, takes Nodal's own files back out, and leaves the directory where
//! it always was, back to the same `git status` again.
//!
//! The fourth is what a person reads when it is over. An adoption closes with a sentence
//! that says what happened to their directory, and anything listed under it is
//! introduced by that sentence.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::model::{EnvState, Epistemic, UnitStatus};
use nodal_core::store::{Store, environments, projects, trash, units};
use tempfile::TempDir;

/// The prompt the fixture's session record holds, which is what a recovery must find.
const PROMPT: &str = "Fix the token refresh so a session that idles overnight is not logged out";

/// Where the nested worktree goes, which is where the tool that made this mess puts one.
const NESTED: &str = ".claude/worktrees/token";

/// The branch that worktree has checked out.
const NESTED_BRANCH: &str = "feature/token-refresh";

/// A project, a nested worktree inside it, and the records of the session that made it.
struct Workspace {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The project's repository.
    source: PathBuf,
    /// Nodal's state directory.
    state: PathBuf,
    /// Where the agent tool's session records are.
    sessions: PathBuf,
}

impl Workspace {
    /// The same project, carrying a recipe, with its hooks approved.
    fn with_recipe(recipe: &str) -> Self {
        let workspace = Self::new();
        std::fs::write(workspace.source.join("nodal.toml"), recipe).unwrap();
        stdout(&workspace.nodal(&["init", "--force"]));
        workspace
    }

    /// A one-commit project with a nested worktree on a branch of its own.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let source = root.path().join("project");
        let state = root.path().join("state");
        let sessions = root.path().join("agent");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        std::fs::write(source.join("a.txt"), "one\n").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "unit@example.invalid"],
            vec!["config", "user.name", "Test"],
            vec!["add", "-A"],
            vec!["commit", "-qm", "first"],
        ] {
            git(&source, &args);
        }
        let workspace = Self { _root: root, source, state, sessions };
        git(&workspace.source, &["worktree", "add", "-q", "-b", NESTED_BRANCH, NESTED]);
        workspace
    }

    /// The nested worktree, resolved the way a registry row records it.
    fn nested(&self) -> PathBuf {
        resolved(&self.source.join(NESTED))
    }

    /// Write the record of a session that ran in `directory`, opened with `prompt`.
    ///
    /// The shape is the tool's own: one file of JSON lines under a directory named
    /// after the working directory, with every character that is not a letter, a digit
    /// or a dash replaced by a dash.
    fn session(&self, directory: &Path, prompt: &str) {
        let encoded = nodal_core::doctor::intent::encode(directory);
        let held = self.sessions.join("projects").join(encoded);
        std::fs::create_dir_all(&held).unwrap();
        let line = format!(
            r#"{{"type":"user","isSidechain":false,"cwd":{cwd},"timestamp":"2026-08-19T23:37:11Z","message":{{"role":"user","content":{prompt}}}}}"#,
            cwd = serde_json::to_string(directory).unwrap(),
            prompt = serde_json::to_string(prompt).unwrap(),
        );
        std::fs::write(held.join("a.jsonl"), format!("{line}\n")).unwrap();
    }

    /// `nodal` with this workspace's state directory, run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.nodal_in(&self.source.clone(), args)
    }

    /// The same, run somewhere else.
    fn nodal_in(&self, directory: &Path, args: &[&str]) -> Output {
        let mut command = state::nodal(&self.state);
        command.args(args).current_dir(directory);
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command.env(nodal_core::doctor::intent::CONFIG_VAR, &self.sessions);
        command.output().unwrap()
    }

    /// The registry, opened for reading what a command wrote.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// The unit with this handle, as the registry holds it.
    fn unit(&self, slug: &str) -> nodal_core::model::Unit {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
        units::list(store.conn(), project.id)
            .unwrap()
            .into_iter()
            .find(|unit| unit.slug.as_str() == slug)
            .unwrap_or_else(|| panic!("no unit is called {slug}"))
    }

    /// That unit's newest materialisation.
    fn environment(&self, slug: &str) -> nodal_core::model::Environment {
        let store = self.store();
        environments::latest_for_unit(store.conn(), self.unit(slug).id)
            .unwrap()
            .expect("the unit has a materialisation")
    }
}

#[test]
fn a_worktree_adopted_in_place_leaves_git_status_byte_identical() {
    let workspace = Workspace::new();
    let nested = workspace.nested();
    let before = git(&nested, &["status"]);

    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));

    assert!(nested.join(".nodal").join("id").is_file(), "the home is marked");
    assert!(nested.join(".envrc").is_file(), "and activated");
    assert_eq!(git(&nested, &["status"]), before, "the checkout looks exactly as it did");

    // And still, after the command that writes the unit's memory into it.
    stdout(&workspace.nodal(&["show", "token-refresh"]));
    assert!(nested.join("WORKUNIT.md").is_file(), "the memory was written");
    assert_eq!(git(&nested, &["status"]), before, "and it is invisible to Git as well");

    let environment = workspace.environment("token-refresh");
    assert!(!environment.managed, "the checkout is a root, not a home Nodal made");
    assert_eq!(environment.home, nested, "and the home is where it always was");
}

#[test]
fn a_nested_worktree_an_agent_made_is_adopted_with_its_intent_recovered() {
    let workspace = Workspace::new();
    workspace.session(&workspace.nested(), PROMPT);

    let report = stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));
    assert!(report.contains(PROMPT), "the report says what was recovered: {report}");

    let unit = workspace.unit("token-refresh");
    assert_eq!(unit.objective.as_ref().map(ToString::to_string).as_deref(), Some(PROMPT));
    assert_eq!(
        unit.objective_epistemic,
        Some(Epistemic::Observed),
        "recovered, and never recorded as though somebody had stated it"
    );

    let listed = stdout(&workspace.nodal(&["ls"]));
    assert!(listed.contains("(recovered)"), "the list says how it is known: {listed}");
}

/// The reported defect: the report ended with a column of env names and nothing that
/// said what they were or what had just happened to the checkout.
///
/// The last line of the answer is now a sentence. Everything under it is a name that
/// sentence counted, so a person reading the bottom of the output is never reading a
/// list with no heading.
#[test]
fn an_adoption_closes_with_a_sentence_that_says_what_it_did() {
    let workspace = Workspace::new();
    let report = stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));

    let lines: Vec<&str> = report.lines().filter(|line| !line.trim().is_empty()).collect();
    let summary = lines
        .iter()
        .position(|line| line.contains("adopted token-refresh in place"))
        .unwrap_or_else(|| panic!("no summary line in:\n{report}"));
    assert!(
        lines[summary].contains("env name"),
        "the summary accounts for the names: {}",
        lines[summary]
    );
    // Nothing after the summary that the summary did not introduce.
    let fixture_names: Vec<&str> = lines[summary + 1..].to_vec();
    for name in &fixture_names {
        assert!(
            name.trim().chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
            "a line under the summary that is not one of the names it counted: {name}"
        );
    }
    assert!(!report.contains("no value"), "the unlabelled column is gone:\n{report}");
}

/// `--json` is not changed by the sentence: it gains the one fact it never carried,
/// which is how the unit came to be, and keeps everything it had.
#[test]
fn the_json_answer_gains_how_the_unit_arrived_and_loses_nothing() {
    let workspace = Workspace::new();
    let output = workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place", "--json"]);
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json is one document");
    assert_eq!(document["arrival"], "adopted_in_place");
    assert_eq!(document["unit"]["slug"], "token-refresh");
    assert!(document["missing"].is_array(), "the names are still there");
    assert!(document["now"].is_string());
}

/// The reported defect: a session opened with dispatch boilerplate gave the unit
/// "IDENTITY CHECK: confirm you are a FRESH session" as its objective.
///
/// The preamble is stepped over and the task is what the unit carries, still marked
/// recovered — and still text out of the record, so a person who opens the session file
/// finds the sentence they are shown.
#[test]
fn a_session_opened_with_dispatch_boilerplate_recovers_the_task_and_not_the_preamble() {
    let workspace = Workspace::new();
    let boilerplate = format!(
        "IDENTITY CHECK: confirm you are a FRESH session, not a continuation.\n\n{PROMPT}."
    );
    workspace.session(&workspace.nested(), &boilerplate);

    let report = stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));
    assert!(report.contains(PROMPT), "the task is the objective: {report}");
    assert!(!report.contains("IDENTITY CHECK"), "the preamble is not: {report}");

    let unit = workspace.unit("token-refresh");
    assert_eq!(unit.objective.as_ref().map(ToString::to_string).as_deref(), Some(PROMPT));
    assert_eq!(unit.objective_epistemic, Some(Epistemic::Observed), "still recovered, not stated");
    assert!(boilerplate.contains(PROMPT), "and every word of it came out of the record");
}

/// A prompt a person typed wins over one read out of a record, because a statement of
/// intent is worth more than a reading of one.
#[test]
fn a_stated_objective_is_not_replaced_by_a_recovered_one() {
    let workspace = Workspace::new();
    workspace.session(&workspace.nested(), PROMPT);

    stdout(&workspace.nodal(&[
        "adopt",
        NESTED_BRANCH,
        "--in-place",
        "-m",
        "audit the key rotation",
    ]));

    let unit = workspace.unit("token-refresh");
    assert_eq!(
        unit.objective.as_ref().map(ToString::to_string).as_deref(),
        Some("audit the key rotation")
    );
    assert_eq!(unit.objective_epistemic, Some(Epistemic::Stated));
}

#[test]
fn an_adopted_root_refuses_trashing_but_gives_up_its_registration() {
    let workspace = Workspace::new();
    let nested = workspace.nested();
    let before = git(&nested, &["status"]);
    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));
    stdout(&workspace.nodal(&["show", "token-refresh"]));

    let report = stdout(&workspace.nodal(&["reclaim", "token-refresh"]));
    assert!(report.contains("left in place"), "{report}");

    assert!(nested.is_dir(), "the person's own directory is where it was");
    assert!(nested.join("a.txt").is_file(), "with the work in it");
    let store = workspace.store();
    assert!(trash::list(store.conn()).unwrap().is_empty(), "nothing of it was trashed");

    let unit = workspace.unit("token-refresh");
    assert_eq!(unit.status, UnitStatus::Archived, "it is unregistered, not forgotten");
    assert_eq!(workspace.environment("token-refresh").state, EnvState::Absent);

    assert!(!nested.join(".nodal").exists(), "and Nodal's own files are out of it again");
    assert!(!nested.join(".envrc").exists());
    assert!(!nested.join("WORKUNIT.md").exists());
    assert_eq!(git(&nested, &["status"]), before, "back to what it said before any of this");
}

#[test]
fn a_branch_no_checkout_holds_gets_a_home_of_its_own() {
    let workspace = Workspace::new();
    git(&workspace.source, &["switch", "-q", "-c", "feature/orphan"]);
    std::fs::write(workspace.source.join("only-here.txt"), "work\n").unwrap();
    git(&workspace.source, &["add", "-A"]);
    git(&workspace.source, &["commit", "-qm", "work on a branch nothing has out"]);
    git(&workspace.source, &["switch", "-q", "main"]);

    stdout(&workspace.nodal(&["adopt", "feature/orphan"]));

    let environment = workspace.environment("orphan");
    assert!(environment.managed, "a home Nodal made is one Nodal may reclaim");
    assert!(environment.base_id.is_some(), "and it is a clone of a base");
    assert_ne!(environment.home, workspace.source, "it is not the project");
    assert!(
        environment.home.join("only-here.txt").is_file(),
        "the home carries the branch's own commits, which the base has never had"
    );
    assert_eq!(
        git(&environment.home, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "feature/orphan"
    );
    assert_eq!(git(&environment.home, &["status", "--porcelain"]), "", "and it is clean");
}

/// Three ways of asking for something Nodal will not do, and what each is told instead.
#[test]
fn a_target_that_cannot_become_a_unit_is_refused_by_name() {
    let workspace = Workspace::new();

    let refused = workspace.nodal(&["adopt", NESTED]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("--in-place"), "{}", stderr(&refused));

    let refused = workspace.nodal(&["adopt", NESTED_BRANCH]);
    assert!(!refused.status.success());
    let told = stderr(&refused);
    assert!(told.contains("is checked out at"), "{told}");
    assert!(told.contains(NESTED), "it names the checkout whose work would be left: {told}");

    let refused = workspace.nodal(&["adopt", ".", "--in-place"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("the project's own checkout"), "{}", stderr(&refused));

    let refused = workspace.nodal(&["adopt", "no/such/branch"]);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("no branch"), "{}", stderr(&refused));
}

/// A directory that is already a unit's home is not adopted twice.
#[test]
fn a_checkout_that_is_already_a_unit_is_refused_and_names_the_unit() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));

    let refused = workspace.nodal(&["adopt", NESTED, "--in-place"]);
    assert!(!refused.status.success());
    let told = stderr(&refused);
    assert!(told.contains("already the home of unit"), "{told}");
    assert!(told.contains(&workspace.unit("token-refresh").id.to_string()), "{told}");
}

/// The two origins, each explained in the terms of what actually happened to it.
#[test]
fn explain_says_where_each_home_came_from() {
    let workspace = Workspace::new();
    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));

    let adopted = stdout(&workspace.nodal(&["explain", "token-refresh"]));
    assert!(adopted.contains("adopted where it stands"), "{adopted}");
    assert!(adopted.contains("nothing was copied"), "{adopted}");
    assert!(adopted.contains("the block"), "it says where the ports came from: {adopted}");

    git(&workspace.source, &["branch", "feature/orphan"]);
    stdout(&workspace.nodal(&["adopt", "feature/orphan"]));

    let cloned = stdout(&workspace.nodal(&["explain", "orphan"]));
    assert!(cloned.contains("a clone of base"), "{cloned}");
    assert!(cloned.contains("warm for this workspace"), "it says why that base: {cloned}");
    assert!(cloned.contains(".claude/worktrees"), "and what the clone left out: {cloned}");
}

/// `nodal show` is the command that keeps a unit's memory current, so what it wrote has
/// to be the answer it printed — including which of the two kinds of objective it is.
///
/// The file is the context compiler's, not this task's: the wording asserted here is
/// the compiler's own (`context::render`), and the one thing adoption contributes to it
/// is that a recovered objective says so.
#[test]
fn show_writes_the_memory_it_reports() {
    let workspace = Workspace::new();
    workspace.session(&workspace.nested(), PROMPT);
    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));

    let shown = stdout(&workspace.nodal(&["show", "token-refresh"]));
    assert!(shown.contains("(recovered)"), "{shown}");

    let memory = std::fs::read_to_string(workspace.nested().join("WORKUNIT.md")).unwrap();
    assert!(memory.contains(PROMPT), "{memory}");
    assert!(memory.contains(&format!("- objective: {PROMPT} (recovered)")), "{memory}");
    assert!(memory.contains("- home: "), "{memory}");
    assert!(memory.contains("## Project ledger"), "{memory}");
}

/// A recipe whose two create hooks each append their own name to one file.
const HOOKS: &str = r#"
[hooks]
pre_new = "printf 'pre_new %s\\n' \"$NODAL_UNIT\" >> \"$NODAL_SOURCE/hooks.log\""
post_new = "printf 'post_new %s\\n' \"$NODAL_ROOT\" >> \"$NODAL_SOURCE/hooks.log\""
"#;

/// DL-042, both sentences, in one test because the ruling is a comparison.
///
/// The two forms of adoption did different things, so they run different hooks. Nothing
/// was created in place: the directory was the person's before the command and is theirs
/// after it, and running a project's own commands inside a live checkout on the strength
/// of registering it is not what registering it asked for — so not one of the six runs.
/// The materialised form made a home from a base, which is exactly what `nodal new`
/// does, so `post_new` runs in it and is told that home. Neither form runs `pre_new`:
/// that is the hook for the moment before a home exists, and neither form has one.
#[test]
fn in_place_runs_no_hook_and_a_materialised_adoption_runs_post_new() {
    let workspace = Workspace::with_recipe(HOOKS);
    let log = workspace.source.join("hooks.log");

    stdout(&workspace.nodal(&["adopt", NESTED_BRANCH, "--in-place"]));
    assert!(!log.exists(), "adoption in place ran a hook: {:?}", std::fs::read_to_string(&log));

    git(&workspace.source, &["branch", "feature/orphan"]);
    stdout(&workspace.nodal(&["adopt", "feature/orphan"]));

    let ran = std::fs::read_to_string(&log).expect("the materialised form ran post_new");
    let phases: Vec<&str> = ran.lines().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(phases, ["post_new"], "post_new and nothing else: {ran}");
    let home = resolved(&workspace.environment("orphan").home);
    assert!(ran.contains(&format!("post_new {}", home.display())), "{ran}");
}

/// `--no-hooks` is the person's own answer, and it reaches the form that has a hook.
#[test]
fn no_hooks_stops_the_one_hook_a_materialised_adoption_would_run() {
    let workspace = Workspace::with_recipe(HOOKS);
    git(&workspace.source, &["branch", "feature/orphan"]);

    stdout(&workspace.nodal(&["--no-hooks", "adopt", "feature/orphan"]));

    assert!(!workspace.source.join("hooks.log").exists(), "--no-hooks ran a hook");
}

/// Which project a unit belongs to is a question the target answers, not the directory
/// the person happened to be standing in.
#[test]
fn a_checkout_named_by_path_is_adopted_from_outside_any_repository() {
    let workspace = Workspace::new();
    let elsewhere = workspace.state.parent().unwrap().to_path_buf();

    let nested = workspace.nested();
    stdout(&workspace.nodal_in(&elsewhere, &["adopt", nested.to_str().unwrap(), "--in-place"]));

    assert_eq!(workspace.environment("token-refresh").home, nested);
    let store = workspace.store();
    let project = projects::list(store.conn()).unwrap().pop().expect("the project is known");
    assert_eq!(
        project.root,
        resolved(&workspace.source),
        "the project is the one the checkout is of"
    );
}

/// A path with every symbolic link on the way to it resolved, which is the form a
/// registry row records.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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

/// `git` in a directory, as text.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
