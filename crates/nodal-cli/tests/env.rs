//! `nodal env` end to end, and the two ways a shell is activated.
//!
//! The engine's own acceptance test lives in `nodal-core` (`tests/env.rs`). This one
//! covers the seam a user meets: a home found from a directory inside it, a report that
//! names variables without printing one, and a shell that really does carry the
//! generated variables.
//!
//! Both activation routes are exercised, because entry chooses between them and `nodal
//! env` ships what each one reads. The direnv route needs direnv, so it reports itself
//! as skipped on a machine that has none rather than failing there.

#![allow(clippy::unwrap_used)]

mod state;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};

use state::Machine;

use nodal_core::model::{EnvId, EnvName, HostName, ProjectId, Recipe, Timestamp, UnitId};
use nodal_safety::activation::{self, name};
use nodal_safety::rows;

/// The credential the test hides, and the port it looks for in a shell.
const SECRET: &str = "s3cr3t-canary-value";
const PORT: &str = "3011";

/// A recipe that declares one generated name and one credential. The engine test uses
/// the whole fixture; here the seam is what matters, so the recipe is the smallest one
/// that has both kinds of name.
fn recipe() -> Recipe {
    let mut recipe = Recipe::default();
    recipe.env.generated = vec![name("PORT"), name("APP_URL")];
    recipe.env.secrets = vec![name("SESSION_SECRET"), name("RESEND_API_KEY")];
    recipe
}

/// Write an activated home under `root` and return it.
fn write_home(root: &Path) -> std::path::PathBuf {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let now = Timestamp::now();
    let unit = rows::unit(
        UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        "fix-worker-import",
        "nodal/fix-worker-import",
        now,
    );
    let mut environment =
        rows::environment(EnvId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(), unit.id, &home, now);
    environment.host = HostName::parse("workstation").unwrap();
    let project = rows::project(unit.project_id, root.to_path_buf(), "fixture", now);

    let produced: BTreeMap<EnvName, String> =
        [(name("PORT"), String::from(PORT)), (name("APP_URL"), format!("http://localhost:{PORT}"))]
            .into();
    let secrets_file = root.join("secrets.env");
    activation::write_secrets(&secrets_file, &format!("SESSION_SECRET={SECRET}\n"));
    activation::write(
        &home,
        &secrets_file,
        &recipe(),
        activation::Generated { produced, stand_ins: None },
        (&unit, &environment, &project),
    );
    home
}

fn nodal(machine: &Machine, args: &[&str]) -> Output {
    machine.nodal().arg("env").args(args).output().unwrap()
}

#[test]
fn env_reports_the_names_and_never_a_value() {
    let directory = tempfile::tempdir().unwrap();
    let machine = Machine::new();
    let home = write_home(directory.path());
    let inside = home.join("apps").join("web");
    std::fs::create_dir_all(&inside).unwrap();

    let output = nodal(&machine, &[inside.to_str().unwrap()]);
    assert!(output.status.success(), "env exited with {:?}", output.status);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("fix-worker-import"), "{stdout}");
    for expected in ["PORT", "SESSION_SECRET", "RESEND_API_KEY", "NODAL_ID"] {
        assert!(stdout.contains(expected), "{expected} is not in {stdout}");
    }
    assert!(!stdout.contains(SECRET), "the report printed a value: {stdout}");

    let json = nodal(&machine, &["--json", home.to_str().unwrap()]);
    assert!(json.status.success());
    let text = String::from_utf8(json.stdout).unwrap();
    assert!(!text.contains(SECRET), "--json printed a value: {text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["unit"], "fix-worker-import");
    assert_eq!(value["missing"][0]["name"], "RESEND_API_KEY");
    assert_eq!(value["missing"][0]["want"], "secret");
}

#[test]
fn a_directory_that_is_not_a_home_is_a_message_rather_than_a_panic() {
    let directory = tempfile::tempdir().unwrap();
    let machine = Machine::new();
    let output = nodal(&machine, &[directory.path().to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("not a unit home"));
}

#[test]
fn a_shell_that_evaluates_the_export_carries_every_generated_variable() {
    let directory = tempfile::tempdir().unwrap();
    let machine = Machine::new();
    let home = write_home(directory.path());

    let script = format!(
        "eval \"$({binary} env --export {home})\"; \
         printf '%s|%s|%s|%s' \"$PORT\" \"$APP_URL\" \"$NODAL_UNIT\" \"$SESSION_SECRET\"",
        binary = state::BINARY,
        home = home.display(),
    );
    // The shell spawns the binary itself, so the shell is what has to carry the state
    // directory and the secrets file. A command built by the harness carries both
    // already; this one is built here, so it names both here. The secrets file is the
    // point of the test: the export resolves it for whoever runs the command.
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env(nodal_core::workspace::home::DIRECTORY_VAR, machine.path())
        .env(nodal_core::env::secrets::PATH_VAR, directory.path().join("secrets.env"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{PORT}|http://localhost:{PORT}|fix-worker-import|{SECRET}")
    );
}

/// The dotenv file is what the home is, and it holds nobody's credential.
///
/// A home under a shared state root is a directory two accounts may enter. If the
/// credential were in the file, the second account would read the first's. So the file
/// carries the unit's identity and the values its own services generated, and the
/// credential is not in it at all.
#[test]
fn the_dotenv_file_carries_the_unit_and_no_credential() {
    let directory = tempfile::tempdir().unwrap();
    let home = write_home(directory.path());

    let script = format!(
        "set -a; . '{home}/.nodal/env'; set +a; printf '%s|%s|%s' \"$PORT\" \"$NODAL_UNIT\" \"$SESSION_SECRET\"",
        home = home.display(),
    );
    let output = Command::new("sh").arg("-c").arg(script).output().unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("{PORT}|fix-worker-import|"));

    let written = std::fs::read_to_string(home.join(".nodal/env")).unwrap();
    assert!(!written.contains(SECRET), "the dotenv file holds a credential: {written}");
}

/// The two lines of `.envrc`, run in order, are the whole activation.
///
/// This is what direnv does and what the rc hook does. The first line says what the
/// home is; the second resolves what the person entering it has. Together they deliver
/// the same set the file used to hold on its own.
#[test]
fn the_two_lines_of_the_envrc_together_carry_the_whole_set() {
    let directory = tempfile::tempdir().unwrap();
    let machine = Machine::new();
    let home = write_home(directory.path());

    let script = format!(
        "set -a; . '{home}/.nodal/env'; eval \"$({binary} env --export {home})\"; set +a; \
         printf '%s|%s|%s' \"$PORT\" \"$NODAL_UNIT\" \"$SESSION_SECRET\"",
        binary = state::BINARY,
        home = home.display(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .env(nodal_core::workspace::home::DIRECTORY_VAR, machine.path())
        .env(nodal_core::env::secrets::PATH_VAR, directory.path().join("secrets.env"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{PORT}|fix-worker-import|{SECRET}")
    );
}

#[test]
fn direnv_activates_the_home_from_the_envrc_this_task_writes() {
    let Ok(direnv) = which_direnv() else {
        eprintln!("skipped: direnv is not on PATH, so the .envrc route was not exercised");
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    let home = write_home(directory.path());

    let allow = Command::new(&direnv).arg("allow").current_dir(&home).output().unwrap();
    assert!(allow.status.success(), "{:?}", String::from_utf8_lossy(&allow.stderr));
    let output = Command::new(&direnv)
        .args(["exec", ".", "sh", "-c", "printf '%s|%s' \"$PORT\" \"$NODAL_UNIT\""])
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("{PORT}|fix-worker-import"));
}

/// Where direnv is, if this machine has one.
fn which_direnv() -> Result<String, ()> {
    let output = Command::new("sh").args(["-c", "command -v direnv"]).output().map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    Ok(String::from_utf8(output.stdout).map_err(|_| ())?.trim().to_owned())
}
