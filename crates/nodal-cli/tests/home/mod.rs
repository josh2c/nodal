//! One activated home, with a registry that knows it.
//!
//! The entry tests all need the same thing: a directory that carries a unit's
//! environment, and a registry holding the project, unit and environment rows a
//! command looks up. This builds both in a temporary directory, so a test never reads
//! or writes the machine's own Nodal home.

#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::env::secrets::{MachineSecrets, SecretSource, UnitGenerated};
use nodal_core::env::{self, Produced, files};
use nodal_core::model::{
    BranchName, Digest, EnvId, EnvName, EnvState, Environment, Event, Ports, Project, ProjectId,
    ProjectName, Recipe, Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::store::{Store, environments, events, projects, units};

/// The credential the home carries, distinctive enough to search any output for.
pub const SECRET: &str = "s3cr3t-canary-value";

/// The port the home's recipe generates.
pub const PORT: &str = "3011";

/// The unit's identifier, fixed so a test can assert on it.
pub const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

/// The unit's slug.
pub const SLUG: &str = "fix-worker-import";

/// An activated home and the registry that knows about it.
pub struct Fixture {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The unit's home.
    pub home: PathBuf,
    /// The registry file.
    pub store: PathBuf,
}

impl Fixture {
    /// Build the home and the registry.
    #[must_use]
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let home = root.join("project-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();

        let (unit, environment, project) = rows(&root, &home);
        write_activation(&root, &home, (&unit, &environment, &project));

        let store = root.join("registry.db");
        let opened = Store::open(&store).unwrap();
        projects::insert(opened.conn(), &project).unwrap();
        units::insert(opened.conn(), &unit).unwrap();
        environments::insert(opened.conn(), &environment).unwrap();
        drop(opened);

        Self { directory, home, store }
    }

    /// The temporary root.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.directory.path()
    }

    /// A directory that is in no unit home.
    #[must_use]
    pub fn outside(&self) -> PathBuf {
        self.directory.path().join("elsewhere")
    }

    /// The binary under test.
    #[must_use]
    pub fn binary() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_nodal"))
    }

    /// The unit's identifier, as it appears in the environment.
    #[must_use]
    pub fn unit_id() -> &'static str {
        UNIT
    }

    /// Run `nodal` against this fixture's registry.
    #[must_use]
    pub fn nodal(&self, args: &[&str], cwd: &Path) -> Output {
        Command::new(Self::binary())
            .args(args)
            .current_dir(cwd)
            .env("NODAL_STORE", &self.store)
            .env("NODAL_HOME", self.directory.path())
            .env_remove("NODAL_CD_FILE")
            .output()
            .unwrap()
    }

    /// Every event the unit's log holds.
    #[must_use]
    pub fn events(&self) -> Vec<Event> {
        let store = Store::open(&self.store).unwrap();
        events::list_for_unit(store.conn(), UnitId::parse(UNIT).unwrap()).unwrap()
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

/// The registry rows this home is a materialisation of.
fn rows(root: &Path, home: &Path) -> (Unit, Environment, Project) {
    let now = Timestamp::now();
    let unit = Unit {
        id: UnitId::parse(UNIT).unwrap(),
        project_id: ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        slug: Slug::parse(SLUG).unwrap(),
        objective: None,
        branch: BranchName::parse("nodal/fix-worker-import").unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    };
    let environment = Environment {
        id: EnvId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(),
        unit_id: unit.id,
        attempt: 1,
        home: home.to_path_buf(),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: nodal_core::lifecycle::owner::current_host(),
        db_name: None,
        ports: Ports::default(),
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: now,
        last_active: now,
    };
    let project = Project {
        id: unit.project_id,
        root: root.join("checkout"),
        name: ProjectName::parse("fixture").unwrap(),
        recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
        created_at: now,
    };
    (unit, environment, project)
}

/// Resolve the environment and write the three activation files.
fn write_activation(root: &Path, home: &Path, subject: (&Unit, &Environment, &Project)) {
    let name = |text: &str| EnvName::parse(text).unwrap();
    let mut recipe = Recipe::default();
    recipe.env.generated = vec![name("PORT"), name("APP_URL")];
    recipe.env.secrets = vec![name("SESSION_SECRET")];

    let produced: BTreeMap<EnvName, String> =
        [(name("PORT"), String::from(PORT)), (name("APP_URL"), format!("http://localhost:{PORT}"))]
            .into();

    let secrets_file = root.join("secrets.env");
    std::fs::write(&secrets_file, format!("SESSION_SECRET={SECRET}\n")).unwrap();
    set_owner_only(&secrets_file);
    let machine = MachineSecrets::open(&secrets_file).unwrap();
    let generated = UnitGenerated::default();
    let sources: [&dyn SecretSource; 2] = [&generated, &machine];

    let activation = env::resolve(subject, &recipe, &Produced::new(produced), &sources).unwrap();
    let (unit, environment, project) = subject;
    let manifest = activation.manifest(unit, environment, project);
    files::write(home, &activation, &manifest).unwrap();
}

/// A one-commit project, and the state directory the units it makes go in.
///
/// Named for what it holds rather than for the project inside it, because
/// `nodal_core::model::Project` is a row in the registry and this is a place on a disk.
///
/// `nodal new` needs a repository with a commit in it; this is the smallest one that is
/// still a project a recipe can be inferred from.
pub struct Workspace {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The repository units are made from.
    pub source: PathBuf,
    /// Nodal's state directory: the registry, and every home.
    pub state: PathBuf,
}

impl Workspace {
    /// Build the repository and its one commit.
    #[must_use]
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("project");
        let state = directory.path().join("state");
        std::fs::create_dir_all(source.join("app")).unwrap();
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
        Self { directory, source, state }
    }

    /// The temporary root, which is where a test writes a shell start-up file.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.directory.path()
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) {}
