//! One activated home, with a registry that knows it.
//!
//! The entry tests all need the same thing: a directory that carries a unit's
//! environment, and a registry holding the project, unit and environment rows a
//! command looks up. This builds both in a temporary directory, so a test never reads
//! or writes the machine's own Nodal home.

#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Output;

use nodal_core::model::{
    EnvId, EnvName, Environment, Event, Project, ProjectId, Recipe, Timestamp, Unit, UnitId,
};
use nodal_core::store::{Store, environments, events, projects, units};
use nodal_safety::activation::{self, name};
use nodal_safety::rows;

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

        let (unit, environment, project) = registry_rows(&root, &home);
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

    /// The binary under test, for a test that puts its path in a shell script.
    ///
    /// A script spawned that way must be given this fixture's state directory as
    /// `NODAL_HOME` itself; a command from [`Fixture::nodal`] already carries it.
    #[must_use]
    pub fn binary() -> PathBuf {
        PathBuf::from(crate::state::BINARY)
    }

    /// The unit's identifier, as it appears in the environment.
    #[must_use]
    pub fn unit_id() -> &'static str {
        UNIT
    }

    /// Run `nodal` against this fixture's registry.
    #[must_use]
    pub fn nodal(&self, args: &[&str], cwd: &Path) -> Output {
        crate::state::nodal(self.directory.path())
            .args(args)
            .current_dir(cwd)
            .env("NODAL_STORE", &self.store)
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
fn registry_rows(root: &Path, home: &Path) -> (Unit, Environment, Project) {
    let now = Timestamp::now();
    let unit = rows::unit(
        UnitId::parse(UNIT).unwrap(),
        ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        SLUG,
        "nodal/fix-worker-import",
        now,
    );
    let environment =
        rows::environment(EnvId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(), unit.id, home, now);
    let project = rows::project(unit.project_id, root.join("checkout"), "fixture", now);
    (unit, environment, project)
}

/// Resolve the environment and write the three activation files.
fn write_activation(root: &Path, home: &Path, subject: (&Unit, &Environment, &Project)) {
    let mut recipe = Recipe::default();
    recipe.env.generated = vec![name("PORT"), name("APP_URL")];
    recipe.env.secrets = vec![name("SESSION_SECRET")];

    let produced: BTreeMap<EnvName, String> =
        [(name("PORT"), String::from(PORT)), (name("APP_URL"), format!("http://localhost:{PORT}"))]
            .into();

    let secrets_file = root.join("secrets.env");
    activation::write_secrets(&secrets_file, &format!("SESSION_SECRET={SECRET}\n"));
    activation::write(
        home,
        &secrets_file,
        &recipe,
        activation::Generated { produced, stand_ins: None },
        subject,
    );
}
