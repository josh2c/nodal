//! Acceptance test for `nodal uninstall` and `nodal upgrade`.
//!
//! The claim the task is about is a byte claim, so the test is a byte test. A start-up
//! file is written, `nodal shell-init --install` is run against it, `nodal uninstall`
//! is run, and the file is compared with the bytes it held before. Six start-up files
//! are used, and they are the awkward ones: a file that ends with a newline, one that
//! does not, an empty one, one that does not exist yet, one with a blank line at the
//! end, and one a person added lines to after the install.
//!
//! Three more claims, each about a promise the command makes rather than about a
//! message it prints:
//!
//! - **nothing goes unannounced.** Every item is named, with its path, before anything
//!   is removed, and a terminal that is not watched is refused rather than waited on.
//! - **the state directory is opt-in and checked.** `--state` is what removes the
//!   registry and the unit homes, and a home that holds work nothing else has stops the
//!   uninstall until `--force` says otherwise. That is the same check every other
//!   destructive path in Nodal makes.
//! - **`upgrade` and `update` answer identically, and fetch nothing.** The channel is
//!   read from the path of the binary, so the test puts the binary in a cargo bin
//!   directory and in a plain one and reads the command each is told to run.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_safety::text::stdout;
use tempfile::TempDir;

/// A machine: a person's own directory, and Nodal's state directory beside it.
struct Machine {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// What `$HOME` is for every command the test runs.
    home: PathBuf,
    /// Nodal's state directory.
    state: PathBuf,
}

impl Machine {
    /// A machine with nothing installed on it.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let home = root.path().join("home");
        let state = root.path().join("state");
        std::fs::create_dir_all(&home).unwrap();
        Self { _root: root, home, state }
    }

    /// `nodal` with this machine's directories, run from the person's own directory.
    fn nodal(&self, args: &[&str]) -> Output {
        nodal_safety::runner::Runner::new(state::BINARY, &self.state, &self.home)
            .with_env("HOME", &self.home)
            .with_env("USERPROFILE", &self.home)
            .nodal(args)
    }

    /// One shell's start-up file.
    fn rc(&self, shell: &str) -> PathBuf {
        match shell {
            "bash" => self.home.join(".bashrc"),
            "zsh" => self.home.join(".zshrc"),
            _ => self.home.join(".config/fish/config.fish"),
        }
    }
}

/// The bytes of a file, or `None` when the file is not there.
fn bytes(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

// ---------------------------------------------------------------------------
// The byte claim.
// ---------------------------------------------------------------------------

#[test]
fn a_start_up_file_is_byte_identical_after_an_install_and_an_uninstall() {
    let awkward: [Option<&str>; 5] = [
        Some("PS1='$ '\nexport EDITOR=vi\n"),
        Some("PS1='$ '"),
        Some("# a comment\n\n"),
        None,
        Some(""),
    ];
    for before in awkward {
        let machine = Machine::new();
        let rc = machine.rc("bash");
        if let Some(text) = before {
            std::fs::write(&rc, text).unwrap();
        }

        drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));
        assert_ne!(bytes(&rc).as_deref(), before.map(str::as_bytes), "the install did nothing");
        drop(stdout(&machine.nodal(&["uninstall", "--yes"])));

        assert_eq!(
            bytes(&rc).as_deref().map(<[u8]>::to_vec),
            before.map(|text| text.as_bytes().to_vec()),
            "a start-up file that held {before:?} did not come back byte for byte"
        );
    }
}

#[test]
fn every_shell_that_was_installed_into_comes_back_byte_identical() {
    let machine = Machine::new();
    let before = "# the person's own line\n";
    let mut held = Vec::new();
    for shell in ["bash", "zsh", "fish"] {
        let rc = machine.rc(shell);
        std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
        std::fs::write(&rc, before).unwrap();
        drop(stdout(&machine.nodal(&["shell-init", "--install", shell])));
        held.push(rc);
    }

    drop(stdout(&machine.nodal(&["uninstall", "--yes"])));

    for rc in held {
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), before, "{}", rc.display());
    }
}

#[test]
fn lines_a_person_added_after_the_install_are_still_there_afterwards() {
    let machine = Machine::new();
    let rc = machine.rc("bash");
    std::fs::write(&rc, "first\n").unwrap();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));
    let installed = std::fs::read_to_string(&rc).unwrap();
    std::fs::write(&rc, format!("{installed}last\n")).unwrap();

    drop(stdout(&machine.nodal(&["uninstall", "--yes"])));

    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "first\nlast\n");
}

#[test]
fn installing_twice_writes_one_block_and_uninstalling_once_removes_it() {
    let machine = Machine::new();
    let rc = machine.rc("bash");
    std::fs::write(&rc, "PS1='$ '\n").unwrap();

    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));
    let second = stdout(&machine.nodal(&["shell-init", "--install", "bash"]));

    assert!(second.contains("already there"), "{second}");
    let installed = std::fs::read_to_string(&rc).unwrap();
    assert_eq!(installed.matches("# >>> nodal >>>").count(), 1, "{installed}");
    drop(stdout(&machine.nodal(&["uninstall", "--yes"])));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "PS1='$ '\n");
}

// ---------------------------------------------------------------------------
// What the block loads, and what goes with it.
// ---------------------------------------------------------------------------

#[test]
fn the_block_sources_a_file_nodal_wrote_and_runs_nothing_on_a_shell_start() {
    let machine = Machine::new();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));

    let block = std::fs::read_to_string(machine.rc("bash")).unwrap();
    let shim = machine.state.join("shims/nodal.bash");
    assert!(shim.is_file(), "the script the block loads is not there");
    assert!(block.contains(&shim.display().to_string()), "{block}");
    assert!(!block.contains("eval"), "the block evaluates something: {block}");
    assert!(!block.contains("$("), "the block starts a process: {block}");

    let printed = stdout(&machine.nodal(&["shell-init", "bash"]));
    assert_eq!(
        std::fs::read_to_string(&shim).unwrap(),
        printed,
        "the file a shell sources is not the text `nodal shell-init` prints"
    );
}

#[test]
fn a_bash_that_reads_the_start_up_file_gets_the_function_and_no_error() {
    let Some(bash) = which("bash") else { return };
    let machine = Machine::new();
    std::fs::write(machine.rc("bash"), "PS1=''\n").unwrap();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));

    let output = Command::new(bash)
        .args(["--noprofile", "--rcfile", &machine.rc("bash").display().to_string(), "-i"])
        .current_dir(&machine.home)
        .env("HOME", &machine.home)
        .env("HISTFILE", machine.home.join("history"))
        .env("NODAL_HOME", &machine.state)
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let complaints = String::from_utf8_lossy(&output.stderr);
    assert!(!complaints.contains("No such file"), "{complaints}");
    assert!(!complaints.contains("command not found"), "{complaints}");
}

#[test]
fn the_shims_go_with_the_block_and_the_directory_goes_with_the_last_of_them() {
    let machine = Machine::new();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));
    drop(stdout(&machine.nodal(&["shell-init", "--install", "zsh"])));
    assert!(machine.state.join("shims/nodal.zsh").is_file());

    drop(stdout(&machine.nodal(&["uninstall", "--yes"])));

    assert!(!machine.state.join("shims").exists(), "the scripts directory is still there");
}

// ---------------------------------------------------------------------------
// The summary, and what it does not do without being told.
// ---------------------------------------------------------------------------

#[test]
fn every_item_is_named_with_its_path_before_anything_is_removed() {
    let machine = Machine::new();
    std::fs::write(machine.rc("bash"), "PS1='$ '\n").unwrap();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));

    let listed = machine.nodal(&["uninstall", "--dry-run"]);
    let summary = String::from_utf8_lossy(&listed.stderr);

    assert!(summary.contains("rc block"), "{summary}");
    assert!(summary.contains(&machine.rc("bash").display().to_string()), "{summary}");
    assert!(summary.contains("shim"), "{summary}");
    assert!(summary.contains("nothing was done"), "{summary}");
    assert!(
        std::fs::read_to_string(machine.rc("bash")).unwrap().contains("# >>> nodal >>>"),
        "a dry run removed something"
    );
}

#[test]
fn a_terminal_nobody_is_watching_is_refused_rather_than_waited_on() {
    let machine = Machine::new();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));

    let refused = machine.nodal(&["uninstall"]);

    assert!(!refused.status.success(), "an unwatched uninstall ran without being agreed to");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert!(told.contains("--yes"), "{told}");
    assert!(machine.state.join("shims/nodal.bash").is_file(), "and nothing was removed");
}

#[test]
fn a_machine_with_nothing_installed_says_so_and_removes_nothing() {
    let machine = Machine::new();
    let answer = stdout(&machine.nodal(&["uninstall", "--yes"]));
    assert!(answer.contains("nodal has installed nothing"), "{answer}");
}

#[test]
fn the_state_directory_stays_until_it_is_asked_for() {
    let machine = Machine::new();
    drop(stdout(&machine.nodal(&["shell-init", "--install", "bash"])));
    std::fs::write(machine.state.join("registry.db"), b"").unwrap();

    drop(stdout(&machine.nodal(&["uninstall", "--yes"])));

    assert!(machine.state.join("registry.db").is_file(), "the registry went unasked");

    let answer = stdout(&machine.nodal(&["uninstall", "--yes", "--state"]));
    assert!(answer.contains("state"), "{answer}");
    assert!(!machine.state.exists(), "the state directory is still there");
}

#[test]
fn force_needs_state_and_is_not_a_flag_on_its_own() {
    let machine = Machine::new();
    let refused = machine.nodal(&["uninstall", "--force", "--yes"]);
    assert!(!refused.status.success(), "--force ran without --state");
}

// ---------------------------------------------------------------------------
// The state directory holds work, so it is checked before it is removed.
// ---------------------------------------------------------------------------

/// A one-commit project, and one unit made from it, in this machine's state directory.
///
/// `nodal new` is what makes the unit, so the home the check reads is a home Nodal
/// really made rather than a directory the test arranged to look like one.
fn a_unit_of_a_real_project(machine: &Machine) -> PathBuf {
    let source = machine.home.join("project");
    std::fs::create_dir_all(source.join("app")).unwrap();
    std::fs::write(source.join("app/main.txt"), "shared\n").unwrap();
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
    let mut command = state::nodal(&machine.state);
    command.args(["new", "worker-import", "--json"]).current_dir(&source);
    command.env("HOME", &machine.home).env("USERPROFILE", &machine.home);
    command.env("NODAL_SECRETS_FILE", machine.state.join("secrets.env"));
    command.env("NODAL_HOOKS_FILE", machine.state.join("hooks.toml"));
    let made = command.output().unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let answer: serde_json::Value =
        serde_json::from_slice(&made.stdout).expect("nodal new answers with JSON");
    PathBuf::from(answer["unit"]["environment"]["home"].as_str().expect("the home path"))
}

#[test]
fn a_home_that_holds_work_nothing_else_has_stops_the_state_directory_going() {
    let machine = Machine::new();
    let home = a_unit_of_a_real_project(&machine);
    std::fs::write(home.join("notes.txt"), "not committed anywhere\n").unwrap();

    let refused = machine.nodal(&["uninstall", "--state", "--yes"]);

    assert!(!refused.status.success(), "the state directory went with work in it");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert!(told.contains("notes.txt"), "the refusal names the work: {told}");
    assert!(home.is_dir(), "and the home is still there");
    assert!(machine.state.join("registry.db").is_file());
}

#[test]
fn force_says_what_it_accepted_losing_and_then_removes_it() {
    let machine = Machine::new();
    let home = a_unit_of_a_real_project(&machine);
    std::fs::write(home.join("notes.txt"), "not committed anywhere\n").unwrap();

    let forced = machine.nodal(&["uninstall", "--state", "--force", "--yes"]);

    assert!(forced.status.success(), "{}", String::from_utf8_lossy(&forced.stderr));
    let summary = String::from_utf8_lossy(&forced.stderr);
    assert!(summary.contains("notes.txt"), "{summary}");
    assert!(summary.contains("--force was given"), "{summary}");
    assert!(!machine.state.exists(), "the state directory is still there");
}

#[test]
fn a_clean_unit_is_not_a_reason_to_refuse_and_the_summary_counts_it() {
    let machine = Machine::new();
    let home = a_unit_of_a_real_project(&machine);

    let listed = machine.nodal(&["uninstall", "--state", "--dry-run"]);
    let summary = String::from_utf8_lossy(&listed.stderr);

    assert!(summary.contains("1 unit home"), "{summary}");
    assert!(!summary.contains("nothing was done; commit"), "{summary}");
    assert!(home.is_dir(), "a dry run removed a home");
}

// ---------------------------------------------------------------------------
// Upgrade and update.
// ---------------------------------------------------------------------------

/// A directory for a fake install tree, on the file system the binary is on.
///
/// `CARGO_TARGET_TMPDIR` is the directory Cargo gives an integration test inside the
/// target directory, so a name under it and the binary under test are always on one
/// file system. [`link_binary`] needs that, and nothing else here does.
fn install_root() -> TempDir {
    TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// The binary at `to`, so that the channel it reports is the channel of the directory
/// the test put it in.
///
/// A hard link, not a copy, and the difference is what stops these tests racing each
/// other. Four tests here put the binary somewhere and run it, the harness runs them as
/// threads of one process, and a thread that starts a process forks: for as long as the
/// child has not reached its own `exec`, it holds a copy of every descriptor its parent
/// had open, the descriptor another thread is writing a copy of the binary through
/// included. Linux refuses to execute a file that any process holds open for writing,
/// with `Text file busy`, and that is what these tests raced for. A link writes nothing,
/// so no descriptor to what is about to run ever exists.
///
/// The link names the same file under a second name, which is what these tests need:
/// the channel is read from the path the process was started as, and that is the path
/// given here rather than the one Cargo built.
fn link_binary(to: &Path) -> PathBuf {
    std::fs::create_dir_all(to).unwrap();
    let linked = to.join("nodal");
    std::fs::hard_link(state::BINARY, &linked).unwrap();
    linked
}

/// Run the binary under the name it was given, and return what it printed.
fn run(binary: &Path, args: &[&str], state: &Path) -> String {
    let output = Command::new(binary)
        .args(args)
        .env("NODAL_HOME", state)
        .env_remove("CARGO_HOME")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_cargo_install_is_told_to_use_cargo_and_a_plain_binary_is_told_where_releases_are() {
    let root = install_root();
    let state = root.path().join("state");

    let cargo = link_binary(&root.path().join("home/.cargo/bin"));
    let answer = run(&cargo, &["upgrade"], &state);
    assert!(answer.contains("cargo"), "{answer}");
    assert!(answer.contains("cargo install nodal --force"), "{answer}");

    let plain = link_binary(&root.path().join("opt/tools"));
    let answer = run(&plain, &["upgrade"], &state);
    assert!(answer.contains("releases"), "{answer}");
    assert!(!answer.contains("cargo install"), "{answer}");
}

#[test]
fn upgrade_and_update_are_the_same_answer() {
    let root = install_root();
    let state = root.path().join("state");
    let binary = link_binary(&root.path().join("home/.cargo/bin"));

    assert_eq!(run(&binary, &["upgrade"], &state), run(&binary, &["update"], &state));
    assert_eq!(
        run(&binary, &["upgrade", "--json"], &state),
        run(&binary, &["update", "--json"], &state)
    );
}

#[test]
fn upgrade_says_that_nodal_fetches_nothing() {
    let root = install_root();
    let state = root.path().join("state");
    let binary = link_binary(&root.path().join("opt/tools"));

    let answer = run(&binary, &["upgrade"], &state);

    assert!(answer.contains("fetches nothing"), "{answer}");
}

#[test]
fn upgrade_makes_no_state_directory_of_its_own() {
    let root = install_root();
    let state = root.path().join("state");
    let binary = link_binary(&root.path().join("opt/tools"));

    run(&binary, &["upgrade"], &state);

    assert!(!state.exists(), "upgrade opened a registry it has no use for");
}

/// Where a program is, if this machine has it.
fn which(program: &str) -> Option<PathBuf> {
    let output = Command::new("sh").arg("-c").arg(format!("command -v {program}")).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    Some(PathBuf::from(path)).filter(|found| found.exists())
}
