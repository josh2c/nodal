//! Acceptance for activation and entry (T1.9), against real shells.
//!
//! Four claims:
//!
//! 1. A process started in an activated directory carries `NODAL_ID`, by either route:
//!    the `.envrc` direnv reads, or the prompt hook `nodal shell-init` installs.
//! 2. Nothing here starts a shell inside a shell. `nodal shell` becomes the shell, and
//!    the integration only ever changes the directory of the shell that is already
//!    running.
//! 3. `nodal cd` moves the shell a person is in, in bash and in zsh.
//! 4. `nodal run` runs the command in the unit's environment and records one event,
//!    with the credential replaced by the name that holds it.
//!
//! A shell this machine does not have reports itself as skipped rather than failing, so
//! the file runs anywhere; the CI job installs bash, zsh, fish and direnv, which is what
//! makes each check real.

#![allow(clippy::unwrap_used)]

mod home;
mod state;

use std::path::{Path, PathBuf};
use std::process::Command;

use home::{Fixture, PORT, SECRET, SLUG, Workspace};

/// A terminal: a real shell, started interactive, reading its commands from a pipe,
/// with the integration installed in the start-up file it reads.
///
/// This is the shape a person's terminal has, and it is the shape the check has to
/// have: a prompt hook fires between prompts, so a shell given `-c` never runs one.
struct Terminal {
    /// The shell binary.
    program: PathBuf,
    /// The arguments that make it interactive and read the start-up file below.
    args: Vec<String>,
    /// The variables that point it at that file.
    vars: Vec<(String, PathBuf)>,
}

impl Terminal {
    /// A bash that reads a start-up file with the integration in it.
    fn bash(program: PathBuf, root: &Path) -> Self {
        let rc = root.join("rc.bash");
        let ticks = root.join("ticks");
        std::fs::write(
            &rc,
            format!(
                "PS1=''\nPROMPT_COMMAND='echo tick >> {ticks}'\n{install}\n",
                ticks = ticks.display(),
                install = install("bash"),
            ),
        )
        .unwrap();
        Self {
            program,
            args: vec![
                String::from("--noprofile"),
                String::from("--rcfile"),
                rc.to_string_lossy().into_owned(),
                String::from("-i"),
            ],
            // An interactive bash writes its history when it ends, and the file it
            // writes belongs to whoever is running the tests unless it is told
            // otherwise. Nothing here is about a shell's history; this keeps the test's
            // own typing out of a person's.
            vars: vec![(String::from("HISTFILE"), root.join("bash_history"))],
        }
    }

    /// A zsh that reads a start-up file with the integration in it.
    fn zsh(program: PathBuf, root: &Path) -> Self {
        let directory = root.join("zdotdir");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(".zshrc"), format!("PS1=''\n{}\n", install("zsh"))).unwrap();
        Self {
            program,
            args: vec![String::from("-i")],
            vars: vec![(String::from("ZDOTDIR"), directory)],
        }
    }

    /// The same terminal, with one more variable in its environment.
    fn with(mut self, name: &str, value: &Path) -> Self {
        self.vars.push((name.to_owned(), value.to_path_buf()));
        self
    }

    /// Type `lines` into the terminal and return what it printed.
    ///
    /// Standard error is dropped: a shell reading a pipe writes its prompt there, and
    /// what is being checked is what the commands printed.
    fn typed(&self, lines: &str, cwd: &Path) -> String {
        use std::io::Write as _;

        let mut child = Command::new(&self.program)
            .args(&self.args)
            .envs(self.vars.iter().map(|(name, value)| (name, value)))
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let script = format!("{lines}\nexit\n");
        child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "the shell exited with {:?}", output.status);
        String::from_utf8(output.stdout).unwrap()
    }
}

/// Where a program is, if this machine has it.
fn which(program: &str) -> Option<PathBuf> {
    let output = Command::new("sh").arg("-c").arg(format!("command -v {program}")).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    Some(PathBuf::from(path))
}

/// The shell to test with, or a note that it is not here.
macro_rules! shell_or_skip {
    ($name:literal) => {
        match which($name) {
            Some(path) => path,
            None => {
                eprintln!(concat!("skipped: this machine has no ", $name));
                return;
            }
        }
    };
}

/// The line that installs the integration into a POSIX shell.
fn install(name: &str) -> String {
    format!(r#"eval "$({binary} shell-init {name})""#, binary = Fixture::binary().display())
}

/// What a process started inside the home should report.
fn expected() -> String {
    format!("{id}|fix-worker-import|{PORT}", id = Fixture::unit_id())
}

/// The command that prints it, run as a child process of the shell under test.
const REPORT: &str = "sh -c 'printf \"%s|%s|%s\" \"$NODAL_ID\" \"$NODAL_UNIT\" \"$PORT\"'";

#[test]
fn bash_carries_the_unit_into_every_process_it_starts() {
    let bash = shell_or_skip!("bash");
    let fixture = Fixture::new();
    let terminal = Terminal::bash(bash, fixture.root());
    let typed = format!("cd '{home}'\n{REPORT}", home = fixture.home.display());
    assert_eq!(terminal.typed(&typed, &fixture.outside()), expected());
}

#[test]
fn zsh_carries_the_unit_into_every_process_it_starts() {
    let zsh = shell_or_skip!("zsh");
    let fixture = Fixture::new();
    let terminal = Terminal::zsh(zsh, fixture.root());
    let typed = format!("cd '{home}'\n{REPORT}", home = fixture.home.display());
    assert_eq!(terminal.typed(&typed, &fixture.outside()), expected());
}

#[test]
fn fish_carries_the_unit_into_every_process_it_starts() {
    let fish = shell_or_skip!("fish");
    let fixture = Fixture::new();
    let script = format!(
        "{binary} shell-init fish | source\ncd '{home}'\n{REPORT}",
        binary = Fixture::binary().display(),
        home = fixture.home.display(),
    );
    let output =
        Command::new(&fish).arg("-c").arg(&script).current_dir(fixture.outside()).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected());
}

#[test]
fn leaving_a_home_unsets_what_entering_it_set() {
    let bash = shell_or_skip!("bash");
    let fixture = Fixture::new();
    let terminal = Terminal::bash(bash, fixture.root());
    let typed = format!(
        "cd '{home}'\ncd '{outside}'\nprintf '[%s][%s][%s]' \"$NODAL_ID\" \"$PORT\" \"$NODAL_EXPORTED\"",
        home = fixture.home.display(),
        outside = fixture.outside().display(),
    );
    assert_eq!(terminal.typed(&typed, &fixture.outside()), "[][][]");
}

#[test]
fn nodal_cd_moves_the_bash_it_is_run_from() {
    let bash = shell_or_skip!("bash");
    let fixture = Fixture::new();
    let terminal = Terminal::bash(bash, fixture.root());
    assert_eq!(terminal.typed(&cd_line(&fixture), &fixture.outside()), moved(&fixture));
}

#[test]
fn nodal_cd_moves_the_zsh_it_is_run_from() {
    let zsh = shell_or_skip!("zsh");
    let fixture = Fixture::new();
    let terminal = Terminal::zsh(zsh, fixture.root());
    assert_eq!(terminal.typed(&cd_line(&fixture), &fixture.outside()), moved(&fixture));
}

#[test]
fn nodal_new_moves_the_shell_into_the_unit_it_made() {
    let bash = shell_or_skip!("bash");
    let project = Workspace::new();
    let terminal = Terminal::bash(bash, project.root())
        .with("NODAL_HOME", &project.state)
        // The per-machine secrets file is shared by every unit on a machine, and a test
        // must never read or create the one belonging to whoever is running it.
        .with("NODAL_SECRETS_FILE", &project.state.join("secrets.env"));

    let typed =
        "nodal new 'fix the worker import' > /dev/null\nprintf '%s|%s' \"$PWD\" \"$NODAL_UNIT\"";
    let seen = terminal.typed(typed, &project.source);
    let (moved, unit) = seen.split_once('|').unwrap_or_default();

    assert!(
        Path::new(moved).starts_with(&project.state),
        "the shell is at {moved}, which is not a home under {}",
        project.state.display()
    );
    // Which handle `nodal new` derives is that command's rule, not this one's. What
    // matters here is that the shell is in the home it made and carries its unit.
    assert!(!unit.is_empty(), "the shell moved but the home was not activated: {seen}");
}

/// Ask the shell function to enter the unit by name, then report where the shell is.
fn cd_line(fixture: &Fixture) -> String {
    format!(
        "nodal --store '{store}' cd {SLUG} > /dev/null\nprintf '%s|%s' \"$PWD\" \"$NODAL_UNIT\"",
        store = fixture.store.display(),
    )
}

/// What the shell should report after it moved.
fn moved(fixture: &Fixture) -> String {
    format!("{home}|{SLUG}", home = fixture.home.display())
}

#[test]
fn installing_the_hook_keeps_the_prompt_command_a_person_already_had() {
    let bash = shell_or_skip!("bash");
    let fixture = Fixture::new();
    let terminal = Terminal::bash(bash, fixture.root());
    terminal.typed("true", &fixture.outside());
    let ticks = std::fs::read_to_string(fixture.root().join("ticks")).unwrap_or_default();
    assert!(ticks.contains("tick"), "the prompt command that was there stopped running");
}

#[test]
fn nodal_cd_prints_the_path_for_a_shell_that_has_no_function() {
    let fixture = Fixture::new();
    let output = fixture.nodal(&["cd", "fix-worker-import"], &fixture.outside());
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let printed = String::from_utf8(output.stdout).unwrap();
    assert_eq!(printed.trim(), fixture.home.to_str().unwrap());
}

#[test]
fn an_ide_terminal_with_only_an_envrc_is_activated() {
    let Some(direnv) = which("direnv") else {
        eprintln!("skipped: this machine has no direnv, so the .envrc route was not exercised");
        return;
    };
    let fixture = Fixture::new();
    allow(&direnv, &fixture);
    let output = Command::new(&direnv)
        .args(["exec", ".", "sh", "-c", "printf '%s|%s' \"$NODAL_ID\" \"$PORT\""])
        .current_dir(&fixture.home)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{id}|{PORT}", id = Fixture::unit_id())
    );
}

#[test]
fn the_hook_leaves_a_home_direnv_already_activated_alone() {
    let bash = shell_or_skip!("bash");
    let Some(direnv) = which("direnv") else {
        eprintln!("skipped: this machine has no direnv");
        return;
    };
    let fixture = Fixture::new();
    allow(&direnv, &fixture);

    // A terminal an IDE opened on the home, with direnv doing the activation. The
    // prompt hook finds NODAL_ROOT already correct and adds nothing of its own, which
    // is what an unset NODAL_EXPORTED says.
    let mut terminal = Terminal::bash(bash, fixture.root());
    let mut args = vec![
        String::from("exec"),
        fixture.home.to_string_lossy().into_owned(),
        terminal.program.to_string_lossy().into_owned(),
    ];
    args.extend(terminal.args);
    terminal = Terminal { program: direnv, args, vars: terminal.vars };

    let typed = "printf '%s|%s' \"$NODAL_ID\" \"${NODAL_EXPORTED-none}\"";
    assert_eq!(terminal.typed(typed, &fixture.home), format!("{id}|none", id = Fixture::unit_id()));
}

/// Let direnv load this home's `.envrc`, as a person does once per directory.
fn allow(direnv: &Path, fixture: &Fixture) {
    let output = Command::new(direnv).arg("allow").current_dir(&fixture.home).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn nodal_shell_becomes_the_shell_rather_than_running_one_under_it() {
    let fixture = Fixture::new();
    // The wrapper records its own process id and then hands the process to nodal. A
    // shell started under nodal would report a different one.
    let record = fixture.root().join("pid");
    let script = format!(
        "echo $$ > '{record}'; exec '{binary}' shell '{home}'",
        record = record.display(),
        binary = Fixture::binary().display(),
        home = fixture.home.display(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .env("SHELL", "/bin/sh")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(b"printf '%s|%s' \"$$\" \"$NODAL_ID\"\n")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let seen = String::from_utf8(output.stdout).unwrap();
    let wrapper = std::fs::read_to_string(&record).unwrap().trim().to_owned();
    assert_eq!(seen, format!("{wrapper}|{id}", id = Fixture::unit_id()));
}

#[test]
fn nodal_run_runs_in_the_environment_and_records_one_event() {
    let fixture = Fixture::new();
    let output =
        fixture.nodal(&["run", "sh", "-c", "printf '%s' \"$PORT\"; exit 3"], &fixture.home);
    assert_eq!(output.status.code(), Some(3), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), PORT);

    let events = fixture.events();
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(event.kind, nodal_core::model::EventKind::Command);
    assert_eq!(event.epistemic, nodal_core::model::Epistemic::Observed);
    assert_eq!(event.refs.get(&reference("exit_code")).map(String::as_str), Some("3"));
}

#[test]
fn a_run_records_the_name_of_a_credential_and_never_its_value() {
    let fixture = Fixture::new();
    let output = fixture.nodal(&["run", "sh", "-c", &format!("echo {SECRET}")], &fixture.home);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let events = fixture.events();
    assert_eq!(events.len(), 1);
    assert!(!events[0].body.contains(SECRET), "the log holds a value: {}", events[0].body);
    assert!(events[0].body.contains("$SESSION_SECRET"), "{}", events[0].body);
}

/// A reference name, for reading one out of an event.
fn reference(text: &str) -> nodal_core::model::RefName {
    nodal_core::model::RefName::parse(text).unwrap()
}
