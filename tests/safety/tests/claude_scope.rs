//! Where the Claude Code hooks are installed, and what the provider hook may do in a
//! project that is not Nodal's.
//!
//! The hooks used to go in the project, and only when a person answered a question
//! `nodal init` asked. They go in the person's own `~/.claude/settings.json` now, and
//! only when `--claude-hooks` asks for it. That moves one command's reach from one
//! repository to every project on the machine, so three lines of the never list are
//! about this file rather than about a corner of it.
//!
//! | line | what a break would look like | test |
//! |---|---|---|
//! | never write into a tracked file | an `init` nobody asked leaves a file in a repository, and the next commit ships it | `an_init_that_was_not_asked_writes_no_hook_anywhere`, `the_hooks_go_in_the_persons_own_file_and_never_in_the_project` |
//! | never delete unique work | the provider answers with a directory that already holds a session's work | `a_worktree_that_already_holds_work_is_never_answered_with` |
//! | never an error without its reason | a session in a project with no recipe ends, or is moved, and nothing says why | `a_repository_with_no_recipe_is_told_what_happened_and_what_to_do` |
//! | uninstall leaves plain repositories | a removal leaves bytes in the person's own settings that were not there before | `an_uninstall_leaves_the_persons_own_settings_byte_for_byte` |
//!
//! Every command here is given a home directory of its own. `nodal init --claude-hooks`
//! writes under it and `nodal uninstall` reads the shell start-up files under it, so a
//! test that let either reach the real one would edit the settings and the `.bashrc` of
//! whoever ran the suite.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use nodal_safety::{InState as _, Machine, git, stderr, stdout};
use tempfile::TempDir;

/// The settings file, relative to whichever directory holds it.
const SETTINGS: &str = ".claude/settings.json";

/// Where Claude Code puts the worktrees it makes for itself, under a project root.
const WORKTREES: &str = ".claude/worktrees";

/// The token every command Nodal writes into a settings file carries.
const MARKER: &str = "nodal claude-code";

/// The handle the payloads below carry, which becomes a unit's slug or a worktree's
/// name.
const HANDLE: &str = "say-hi-6fac65";

/// A machine, and a home directory belonging to nobody.
///
/// The two are separate values because the kit's own fixture does not name a home
/// directory: nothing else in the suite writes under one. This suite does, twice, and
/// both writes are the point of it.
struct Person {
    /// The machine under test.
    machine: Machine,
    /// The temporary root the home directory and any other repository are under, kept so
    /// it outlives the test.
    root: TempDir,
    /// The home directory every command here is given.
    home: PathBuf,
}

impl Person {
    /// A machine whose commands write into a home directory of their own.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        Self { machine: Machine::new(), root, home }
    }

    /// One command of the binary, with this person's home directory.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the binary runs")
    }

    /// The invocation itself, not yet run.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = self.machine.command(args);
        command.env("HOME", &self.home).env("USERPROFILE", &self.home);
        command
    }

    /// The person's own settings file, the one Claude Code reads in every project.
    fn settings(&self) -> PathBuf {
        self.home.join(SETTINGS)
    }

    /// The project's settings file.
    fn project_settings(&self) -> PathBuf {
        self.machine.source.join(SETTINGS)
    }

    /// What a file holds, and nothing when it is not there.
    fn held(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// Fire the provider hook about `cwd`, and answer with what it printed.
    fn worktree_create(&self, cwd: &Path) -> Output {
        let payload = format!(
            "{{\"transcript_path\":\"\",\"cwd\":\"{}\",\"hook_event_name\":\"WorktreeCreate\",\
             \"name\":\"{HANDLE}\"}}",
            cwd.display()
        );
        let mut child = self
            .command(&["claude-code", "worktree-create"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    /// A one-commit repository that is not a Nodal project, away from the machine's own
    /// directories.
    fn plain_repository(&self) -> PathBuf {
        let repository = self.root.path().join("theirs");
        std::fs::create_dir_all(&repository).unwrap();
        std::fs::write(repository.join("README.md"), "somebody else's project\n").unwrap();
        git::init(&repository, "main");
        git::commit(&repository, "the project this person actually works on");
        repository
    }
}

/// `nodal init` asks nothing about hooks and installs none. A person who runs it is
/// writing a recipe; a file left in their repository is a file their next commit ships.
#[test]
fn an_init_that_was_not_asked_writes_no_hook_anywhere() {
    let person = Person::new();

    let written = person.nodal(&["init", "--force"]);

    assert!(written.status.success(), "{}", stderr(&written));
    assert!(
        !person.project_settings().exists(),
        "an init nobody asked wrote hooks into the project"
    );
    assert!(
        !Person::held(&person.settings()).contains(MARKER),
        "an init nobody asked wrote hooks into the person's own settings"
    );
    assert!(
        !person.machine.source.join(".claude").exists(),
        "an init nobody asked left a .claude directory in the repository"
    );
}

/// `--claude-hooks` writes the person's own file. Nothing goes into the repository, so
/// there is nothing for a commit to ship and nothing for a clone to carry to a machine
/// that has no `nodal`.
#[test]
fn the_hooks_go_in_the_persons_own_file_and_never_in_the_project() {
    let person = Person::new();

    let written = person.nodal(&["init", "--force", "--claude-hooks"]);

    assert!(written.status.success(), "{}", stderr(&written));
    let mine = Person::held(&person.settings());
    assert!(mine.contains(MARKER), "the person's own settings hold no hook: {mine}");
    assert_eq!(mine.matches("\"type\": \"command\"").count(), 4, "{mine}");
    assert!(!person.project_settings().exists(), "the project was written into as well");
    assert!(
        !person.machine.source.join(".claude").exists(),
        "a user-scope install left a .claude directory in the repository"
    );
    let said = stderr(&written);
    assert!(said.contains("user"), "the scope that was written was not said: {said}");
}

/// Project scope stays available and stays warned about. A clone of that file on a
/// machine with no `nodal` answers `WorktreeCreate` with a refusal, and Claude Code
/// ends the session over it.
#[test]
fn project_scope_says_what_it_costs_before_it_writes() {
    let person = Person::new();

    let written = person.nodal(&["init", "--force", "--claude-hooks=project"]);

    assert!(written.status.success(), "{}", stderr(&written));
    assert!(Person::held(&person.project_settings()).contains(MARKER), "the project got no hook");
    assert!(
        !Person::held(&person.settings()).contains(MARKER),
        "project scope wrote the person's own settings too"
    );
    let said = stderr(&written);
    assert!(said.contains("no nodal"), "the cost of committing the file was not said: {said}");
}

/// The removal is byte for byte, in the person's own file as in a project's. The file
/// holds a person's permissions and somebody else's hooks, and an uninstall that
/// rewrote it would be an uninstall that edited their settings.
#[test]
fn an_uninstall_leaves_the_persons_own_settings_byte_for_byte() {
    let person = Person::new();
    let theirs = "{\n  \"permissions\": {\n    \"deny\": [\"Bash(rm:*)\"]\n  },\n  \
                  \"hooks\": {\n    \"PreToolUse\": []\n  }\n}\n";
    std::fs::write(person.settings(), theirs).unwrap();
    let installed = person.nodal(&["init", "--force", "--claude-hooks"]);
    assert!(installed.status.success(), "{}", stderr(&installed));
    assert!(Person::held(&person.settings()).contains(MARKER), "nothing was installed to remove");

    let removed = person.nodal(&["uninstall", "--yes"]);

    assert!(removed.status.success(), "{}", stderr(&removed));
    assert_eq!(
        Person::held(&person.settings()),
        theirs,
        "the uninstall did not leave the person's own settings the file it found"
    );
}

/// A repository that is not a Nodal project gets the worktree Claude Code would have
/// made for itself. The hooks are in the person's own settings, so this hook fires in
/// every project on the machine; a refusal would end one session per project.
#[test]
fn a_repository_with_no_recipe_gets_claude_codes_own_worktree() {
    let person = Person::new();
    let theirs = person.plain_repository();

    let answered = person.worktree_create(&theirs);

    assert!(answered.status.success(), "a session was ended: {}", stderr(&answered));
    let made = PathBuf::from(stdout(&answered).trim().to_owned());
    assert_eq!(
        made,
        std::fs::canonicalize(theirs.join(WORKTREES)).unwrap().join(HANDLE),
        "the session was not sent where claude code puts its own worktrees"
    );
    assert!(made.is_dir(), "{} was printed and is not there", made.display());
    assert_eq!(
        git(&made, &["rev-parse", "--abbrev-ref", "HEAD"]),
        HANDLE,
        "the worktree is not on a branch of its own"
    );
    assert!(
        !made.join(".nodal").join("id").exists(),
        "nodal marked a directory in a project it does not manage"
    );
    assert!(person.machine.homes().is_empty(), "a unit was made for a project with no recipe");
}

/// The reason is said whatever the answer. A person whose session moved somewhere they
/// did not choose reads one sentence saying where it went and what to run instead.
#[test]
fn a_repository_with_no_recipe_is_told_what_happened_and_what_to_do() {
    let person = Person::new();
    let theirs = person.plain_repository();

    let answered = person.worktree_create(&theirs);

    assert!(answered.status.success(), "the reason was a refusal, which ends the session");
    let said = stderr(&answered);
    assert!(said.contains("nodal.toml"), "the reason did not name what is missing: {said}");
    assert!(said.contains("nodal init"), "the reason did not say what to do: {said}");
    assert!(
        !stdout(&answered).contains("refused"),
        "the answer was the path claude code rejects: {}",
        stdout(&answered)
    );
}

/// A directory that is already there holds somebody's work. Nodal takes it from nobody:
/// the provider finds a free name rather than answering with a directory it did not
/// make.
#[test]
fn a_worktree_that_already_holds_work_is_never_answered_with() {
    let person = Person::new();
    let theirs = person.plain_repository();
    let taken = theirs.join(WORKTREES).join(HANDLE);
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("their-work.txt"), "a session was here\n").unwrap();

    let answered = person.worktree_create(&theirs);

    assert!(answered.status.success(), "a session was ended: {}", stderr(&answered));
    let made = PathBuf::from(stdout(&answered).trim().to_owned());
    assert_ne!(made, taken, "the session was sent into a directory that already held work");
    assert!(made.is_dir(), "{} was printed and is not there", made.display());
    assert_eq!(
        std::fs::read_to_string(taken.join("their-work.txt")).unwrap(),
        "a session was here\n",
        "work that was already there did not survive the request"
    );
}

/// The other half of the same property: a project that does have a recipe still gets a
/// unit, and the plain worktree path is not reached where Nodal can do better.
#[test]
fn a_nodal_project_still_gets_a_unit_and_not_a_plain_worktree() {
    let person = Person::new();
    let source = person.machine.source.clone();

    let answered = person.worktree_create(&source);

    assert!(answered.status.success(), "a session was ended: {}", stderr(&answered));
    let made = PathBuf::from(stdout(&answered).trim().to_owned());
    assert!(made.join(".nodal").join("id").is_file(), "{} is not a unit home", made.display());
    assert!(!source.join(WORKTREES).exists(), "a nodal project was given a plain worktree");
    assert_eq!(person.machine.homes().len(), 1, "the unit was not the one thing that was made");
}
