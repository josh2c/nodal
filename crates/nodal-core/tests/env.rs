//! Acceptance for env activation over the fixture project.
//!
//! Four claims, and the fixture is what makes them mean something: its recipe declares
//! four generated names and five credentials, which is the shape a real project has.
//!
//! 1. A home activated from that recipe carries every generated variable, and a shell
//!    that reads `.envrc` gets them.
//! 2. A credential no source holds is a line of the report and the home is still
//!    written.
//! 3. The per-machine file is created owner-only and refused when it is not.
//! 4. No value reaches a manifest, a report, a log line or an error message.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;

use nodal_core::env::files;
use nodal_core::env::secrets::{MachineSecrets, SecretSource, UnitGenerated};
use nodal_core::env::{self, Produced};
use nodal_core::model::{
    BranchName, EnvId, EnvName, EnvState, Environment, HostName, Manifest, Origin, Ports, Project,
    ProjectId, ProjectName, Recipe, Slug, Timestamp, Unit, UnitId, UnitStatus, Want,
};
use nodal_core::output::view::EnvReport;
use nodal_core::output::{Format, render};
use ulid::Ulid;

/// A value no source is meant to reveal, distinctive enough to be searched for in any
/// rendering of anything.
const SECRET: &str = "s3cr3t-canary-value";

/// The generated values a service adapter would have produced for one unit.
const PRODUCED: &[(&str, &str)] = &[
    ("PORT", "3011"),
    ("APP_URL", "http://localhost:3011"),
    ("NEXT_PUBLIC_APP_URL", "http://localhost:3011"),
    ("DATABASE_URL", "postgresql://u@localhost:54322/unit_3011"),
];

fn name(text: &str) -> EnvName {
    EnvName::parse(text).expect("a valid environment name")
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<EnvName, String> {
    pairs.iter().map(|(key, value)| (name(key), (*value).to_owned())).collect()
}

/// The fixture's recipe, inferred the way `nodal new` would infer it.
fn fixture_recipe(root: &Path) -> Recipe {
    let project = nodal_fixture::write(root);
    nodal_core::recipe::load(&project).expect("the fixture is a readable project").recipe
}

fn unit() -> Unit {
    let now = Timestamp::now();
    Unit {
        id: UnitId::from_ulid(Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()),
        project_id: ProjectId::from_ulid(Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap()),
        slug: Slug::parse("fix-worker-import").unwrap(),
        objective: None,
        objective_epistemic: None,
        branch: BranchName::parse("nodal/fix-worker-import").unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    }
}

fn environment(home: &Path) -> Environment {
    let now = Timestamp::now();
    Environment {
        id: EnvId::from_ulid(Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap()),
        unit_id: unit().id,
        attempt: 1,
        home: home.to_path_buf(),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: HostName::parse("workstation").unwrap(),
        db_name: None,
        ports: Ports::default(),
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: now,
        last_active: now,
    }
}

fn project(root: &Path) -> Project {
    Project {
        id: unit().project_id,
        root: root.to_path_buf(),
        name: ProjectName::parse("fixture").unwrap(),
        recipe_hash: nodal_core::model::Digest::parse("0".repeat(64)).unwrap(),
        created_at: Timestamp::now(),
        remote_url: None,
    }
}

/// Everything one activation of the fixture produces, with the sources a machine has.
struct Activated {
    /// The temporary root, kept so it outlives the test.
    _directory: tempfile::TempDir,
    /// The unit's home.
    home: std::path::PathBuf,
    /// What was resolved.
    activation: env::Activation,
    /// The manifest that was written beside it.
    manifest: Manifest,
}

/// Activate a home from the fixture recipe, with `machine` written to the per-machine
/// file and `generated` minted by the unit's own services.
fn activate(machine: &[(&str, &str)], generated: &[(&str, &str)]) -> Activated {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    let recipe = fixture_recipe(root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();

    let secrets_file = root.join("secrets.env");
    write_owner_only(&secrets_file, machine);
    let machine_source = MachineSecrets::open(&secrets_file).expect("an owner-only file");
    let unit_source = UnitGenerated::new(values(generated));
    let sources: [&dyn SecretSource; 2] = [&unit_source, &machine_source];

    let (unit, environment, project) = (unit(), environment(&home), project(root));
    let activation = env::resolve(
        (&unit, &environment, &project),
        &recipe,
        &Produced::new(values(PRODUCED)),
        None,
        &sources,
    )
    .expect("activation reads its sources");
    let manifest = activation.manifest(&unit, &environment, &project);
    files::write(&home, &activation, &manifest).expect("the home takes its files");
    Activated { _directory: directory, home, activation, manifest }
}

/// Write a dotenv file at owner-only permissions, the way Nodal creates one.
fn write_owner_only(path: &Path, pairs: &[(&str, &str)]) {
    let mut text = String::new();
    for (key, value) in pairs {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    std::fs::write(path, text).unwrap();
    set_mode(path, 0o600);
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn an_activated_home_carries_every_generated_variable_and_its_identity() {
    let activated = activate(&[("SESSION_SECRET", SECRET)], &[]);

    for (name, value) in PRODUCED {
        let var = activated.activation.get(name).expect("a generated variable");
        assert_eq!(var.expose(), *value);
        assert_eq!(var.origin(), Origin::Generated);
    }
    for identity in env::vars::ALL {
        assert!(activated.activation.get(identity).is_some(), "{identity} is not set");
    }
    assert_eq!(
        activated.activation.get("NODAL_UNIT").map(|var| var.expose().to_owned()),
        Some(String::from("fix-worker-import"))
    );
    assert_eq!(
        activated.activation.get("NODAL_ROOT").map(|var| var.expose().to_owned()),
        Some(activated.home.display().to_string())
    );

    let envrc = std::fs::read_to_string(activated.home.join(files::ENVRC)).unwrap();
    assert_eq!(envrc, files::ENVRC_CONTENTS);
    let dotenv = std::fs::read_to_string(activated.home.join(files::ENV)).unwrap();
    for (name, value) in PRODUCED {
        assert!(dotenv.contains(&format!("{name}=\"{value}\"")), "{name} is not in {dotenv}");
    }
}

#[test]
fn a_missing_secret_is_a_report_line_and_the_home_is_still_written() {
    let activated = activate(&[("SESSION_SECRET", SECRET)], &[]);

    let missing: Vec<String> =
        activated.activation.missing.iter().map(|line| line.name.to_string()).collect();
    assert!(missing.contains(&String::from("RESEND_API_KEY")), "{missing:?}");
    assert!(!missing.contains(&String::from("SESSION_SECRET")), "{missing:?}");
    assert!(
        activated.activation.missing.iter().all(|line| line.want == Want::Secret),
        "the fixture leaves only credentials unanswered: {:?}",
        activated.activation.missing
    );

    assert!(activated.home.join(files::ENV).is_file(), "a missing secret must not be fatal");
    assert!(activated.home.join(files::MANIFEST).is_file());
    assert_eq!(activated.manifest.missing, activated.activation.missing);
}

#[test]
fn a_generated_value_wins_and_a_machine_value_fills_the_rest() {
    let activated = activate(
        &[("POSTGRES_PASSWORD", "machine-wide"), ("SESSION_SECRET", SECRET)],
        &[("POSTGRES_PASSWORD", "minted-for-this-unit")],
    );

    let password = activated.activation.get("POSTGRES_PASSWORD").expect("a resolved credential");
    assert_eq!(password.origin(), Origin::Generated);
    assert_eq!(password.expose(), "minted-for-this-unit");
    assert_eq!(
        activated.activation.get("SESSION_SECRET").map(nodal_core::env::EnvVar::origin),
        Some(Origin::Machine)
    );
}

#[cfg(unix)]
#[test]
fn the_per_machine_file_is_created_owner_only_and_refused_when_it_is_not() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nodal").join("secrets.env");

    let created = MachineSecrets::open_or_create(&path).expect("a file is created");
    assert!(created.is_empty());
    assert_eq!(mode_of(&path), 0o600, "a created secrets file must be owner-only");

    std::fs::write(&path, format!("SESSION_SECRET={SECRET}\n")).unwrap();
    set_mode(&path, 0o644);
    let refused = MachineSecrets::open(&path).expect_err("a readable-by-others file is refused");
    let message = refused.to_string();
    assert!(message.contains("644"), "{message}");
    assert!(!message.contains(SECRET), "the refusal must not carry the file's contents");

    set_mode(&path, 0o600);
    let read = MachineSecrets::open(&path).expect("an owner-only file is read");
    assert_eq!(read.len(), 1);
}

#[cfg(unix)]
#[test]
fn the_written_dotenv_is_owner_only() {
    let activated = activate(&[("SESSION_SECRET", SECRET)], &[]);
    assert_eq!(mode_of(&activated.home.join(files::ENV)), 0o600);
}

#[test]
fn no_secret_value_reaches_a_manifest_a_report_or_a_message() {
    let activated = activate(
        &[("SESSION_SECRET", SECRET), ("CRON_SECRET", SECRET), ("SENTRY_DSN", SECRET)],
        &[("POSTGRES_PASSWORD", SECRET)],
    );

    // The value did resolve, so the test is about hiding it rather than about not having it.
    assert_eq!(
        activated.activation.get("SESSION_SECRET").map(nodal_core::env::EnvVar::expose),
        Some(SECRET)
    );

    let manifest_file = std::fs::read_to_string(activated.home.join(files::MANIFEST)).unwrap();
    assert!(manifest_file.contains("SESSION_SECRET"), "the manifest names it: {manifest_file}");
    assert!(!manifest_file.contains(SECRET), "the manifest carries a value: {manifest_file}");

    let report = EnvReport::from_manifest(&activated.manifest, Timestamp::now());
    for format in [Format::Human, Format::Json] {
        let text = render(&report, format).unwrap();
        assert!(text.contains("SESSION_SECRET"), "{text}");
        assert!(!text.contains(SECRET), "a report rendering carries a value: {text}");
    }

    // Debug is what a trace line and a panic message print.
    for text in [
        format!("{:?}", activated.activation),
        format!("{:?}", activated.manifest),
        serde_json::to_string(&activated.manifest).unwrap(),
        toml::to_string_pretty(&activated.manifest).unwrap(),
    ] {
        assert!(!text.contains(SECRET), "a rendering carries a value: {text}");
    }

    // The one rendering that is meant to carry a person's credential is the export a
    // shell evaluates on the way in. The file in the home is not: a home is a directory
    // a second account on a shared host may enter, and it holds no credential at all.
    // `SESSION_SECRET` is the name the person's own file answered. The generated names
    // are in the file and are meant to be: they are the unit's, not a person's.
    let dotenv = std::fs::read_to_string(activated.home.join(files::ENV)).unwrap();
    assert!(!dotenv.contains("SESSION_SECRET"), "the dotenv file carries a credential: {dotenv}");
    assert!(dotenv.contains("NODAL_UNIT"), "the dotenv file says which unit this is");
    assert!(files::export(&activated.activation).contains(SECRET), "--export is for scripts");
}

#[test]
fn the_files_are_idempotent_and_a_step_takes_them_back() {
    let activated = activate(&[("SESSION_SECRET", SECRET)], &[]);
    let before = std::fs::read_to_string(activated.home.join(files::ENV)).unwrap();

    files::write(&activated.home, &activated.activation, &activated.manifest).unwrap();
    let after = std::fs::read_to_string(activated.home.join(files::ENV)).unwrap();
    assert_eq!(before, after, "writing the same activation twice must leave the same bytes");

    files::remove(&activated.home).unwrap();
    files::remove(&activated.home).unwrap();
    for relative in files::PATHS {
        assert!(!activated.home.join(relative).exists(), "{relative} is still there");
    }
}

/// The file reads back as what was written into it, escapes and all.
///
/// What was written into it is the activation without the values a person supplied:
/// those are resolved when somebody enters the home, from their own file. The awkward
/// value is still a person's credential, because the escaping is what this test is
/// about and a credential is the value most likely to hold a quote.
#[test]
fn the_dotenv_file_reads_back_as_the_set_it_was_written_from() {
    let awkward = "a $b \"c\" 'd' \\e";
    let activated = activate(&[("SESSION_SECRET", awkward)], &[]);
    let read = files::read_dotenv(&activated.home).unwrap();

    let written: Vec<(EnvName, String)> = activated
        .activation
        .vars
        .iter()
        .filter(|var| !files::is_a_persons_own(var))
        .map(|var| (var.name().clone(), var.expose().to_owned()))
        .collect();
    assert_eq!(read, written);
    assert!(!read.iter().any(|(_, value)| value == awkward), "a credential is in the file");
}

#[test]
fn the_manifest_round_trips_through_the_file_it_is_written_to() {
    let activated = activate(&[("SESSION_SECRET", SECRET)], &[]);
    let read = files::read_manifest(&activated.home).unwrap();
    assert_eq!(read.env, activated.manifest.env);
    assert_eq!(read.missing, activated.manifest.missing);
    assert_eq!(read.slug, activated.manifest.slug);
}

#[test]
fn the_activation_files_are_hidden_from_the_project_git_status() {
    let directory = tempfile::tempdir().unwrap();
    let git_dir = directory.path().join(".git");
    std::fs::create_dir_all(git_dir.join("info")).unwrap();

    assert!(files::hide(&git_dir).unwrap(), "the first call adds the lines");
    assert!(!files::hide(&git_dir).unwrap(), "the second call adds nothing");
    let exclude = std::fs::read_to_string(git_dir.join("info").join("exclude")).unwrap();
    assert!(exclude.contains("/.nodal/"), "{exclude}");
    assert!(exclude.contains("/.envrc"), "{exclude}");
}
