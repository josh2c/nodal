//! Acceptance for the list (T1.11b), driven through the binary.
//!
//! The fixture builds one origin repository, one clone per unit, and a registry that
//! knows them, so every row is read from a real repository. Ten units are recorded
//! because ten is the size the list is measured at: the last test spawns the binary
//! against all ten and reports how long the answer takes.
//!
//! Four claims:
//!
//! 1. `nodal ls` prints one row per unit, with the integration verdict of each.
//! 2. A bare `nodal` prints the same table, and prints the help where there is no
//!    project to list.
//! 3. `--json` is the same answer as one document a tool reads.
//! 4. The ten-unit list is timed, and the number is printed on every run.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use nodal_core::model::{
    BranchName, Digest, EnvId, EnvState, Environment, Ports, Project, ProjectId, ProjectName, Slug,
    Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::store::{Store, environments, projects, units};
use nodal_fixture::shapes;

/// How many units the list is measured at.
const UNITS: usize = 10;

/// How many runs the reported median is taken over, after one warm-up run.
const RUNS: usize = 20;

/// A project of ten units, each a clone of one branch of the shape fixture.
struct Fixture {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The project root, which is also the origin repository.
    project: PathBuf,
    /// The registry file.
    store: PathBuf,
}

impl Fixture {
    /// Build the origin, the ten homes and the registry.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let project_root = shapes::origin(root.join("origin"));
        let store = root.join("registry.db");
        let opened = Store::open(&store).unwrap();
        let now = Timestamp::now();
        let project = Project {
            id: ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
            root: project_root.clone(),
            name: ProjectName::parse("fixture").unwrap(),
            recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
            created_at: now,
        };
        projects::insert(opened.conn(), &project).unwrap();
        for index in 0..UNITS {
            let branch = shapes::BRANCHES[index % shapes::BRANCHES.len()].name;
            let slug = format!("{branch}-{index}");
            let home = shapes::home(&project_root, root.join("homes").join(&slug), branch);
            shapes::take_branch(&home, &slug);
            record(&opened, &project, &Row { slug: &slug, index, home: &home }, now);
        }
        drop(opened);
        Self { directory, project: project_root, store }
    }

    /// Run `nodal` against this fixture's registry, in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// The command a run uses, so a measurement and a check spawn the same thing.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_nodal")));
        command
            .args(args)
            .current_dir(&self.project)
            .env("NODAL_STORE", &self.store)
            .env("NODAL_HOME", self.directory.path())
            .env_remove("NODAL_CD_FILE");
        command
    }

    /// A directory that is in no project.
    fn outside(&self) -> PathBuf {
        let path = self.directory.path().join("elsewhere");
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// The standard output of a run that must succeed.
    fn text(&self, args: &[&str]) -> String {
        let output = self.nodal(args);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// One unit of the fixture: what it is called, which one it is, and where it lives.
struct Row<'a> {
    /// The unit's handle, which is also its branch here.
    slug: &'a str,
    /// Which unit of the ten this is, so identifiers stay fixed.
    index: usize,
    /// The clone the unit is materialised in.
    home: &'a Path,
}

/// Record one unit and the home it is materialised in.
fn record(store: &Store, project: &Project, row: &Row<'_>, now: Timestamp) {
    let (slug, index, home) = (row.slug, row.index, row.home);
    let unit = Unit {
        id: UnitId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5F{index:02}")).unwrap(),
        project_id: project.id,
        slug: Slug::parse(slug).unwrap(),
        objective: None,
        branch: BranchName::parse(slug).unwrap(),
        parent_branch: None,
        status: UnitStatus::Open,
        created_at: now,
        updated_at: now,
    };
    let environment = Environment {
        id: EnvId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5E{index:02}")).unwrap(),
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
    units::insert(store.conn(), &unit).unwrap();
    environments::insert(store.conn(), &environment).unwrap();
}

#[test]
fn the_list_has_one_row_per_unit_and_says_what_merging_each_would_do() {
    let fixture = Fixture::new();
    let text = fixture.text(&["ls"]);
    let rows: Vec<&str> = text.lines().skip(1).collect();

    assert_eq!(rows.len(), UNITS, "{text}");
    for word in ["done (ancestor)", "done (absorbed)", "conflict", "open"] {
        assert!(text.contains(word), "no {word} in:\n{text}");
    }
    assert!(text.contains("UNIT"), "{text}");
    assert!(text.contains("MAIN"), "{text}");
}

#[test]
fn a_bare_nodal_is_the_list_and_the_help_where_there_is_no_project() {
    let fixture = Fixture::new();
    assert_eq!(fixture.text(&[]), fixture.text(&["ls"]));

    let output = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_nodal")))
        .current_dir(fixture.outside())
        .env("NODAL_STORE", &fixture.store)
        .env("NODAL_HOME", fixture.directory.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("Usage:"), "{text}");
}

#[test]
fn the_json_answer_carries_every_fact_the_table_shows() {
    let fixture = Fixture::new();
    let output = fixture.nodal(&["ls", "--json"]);
    let answer: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json is one document");

    let rows = answer["units"].as_array().unwrap();
    assert_eq!(rows.len(), UNITS);
    assert_eq!(answer["project"], "fixture");
    let squashed = rows.iter().find(|row| row["slug"] == "squashed-3").unwrap();
    assert_squash_merge_is_done(squashed);

    let clashes = rows.iter().find(|row| row["slug"] == "conflicting-4").unwrap();
    assert_eq!(clashes["work"]["integration"]["state"], "conflict");
}

/// Every field a tool reads off the squash-merged unit's row.
fn assert_squash_merge_is_done(row: &serde_json::Value) {
    let work = &row["work"];
    assert_eq!(work["integration"]["state"], "integrated");
    assert_eq!(work["integration"]["reason"], "absorbed");
    assert_eq!(work["dirty"], 0);
    assert_eq!(work["staged"], 0);
    assert_eq!(work["untracked"], 0);
    assert_eq!(work["detached"], false);
    assert!(work["main"]["behind"].as_u64().unwrap() > 0);
    assert!(row["created_at"].is_string());
    assert!(row["sessions"].is_array());
}

#[test]
fn the_units_the_base_has_moved_under_are_printed_first() {
    let fixture = Fixture::new();
    let text = fixture.text(&["ls"]);
    let slugs: Vec<String> = text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect();

    let done = |slug: &str| {
        slug.starts_with("merged")
            || slug.starts_with("squashed")
            || slug.starts_with("follows-base")
    };
    let first_done = slugs.iter().position(|slug| done(slug)).unwrap();
    assert!(slugs.iter().skip(first_done).all(|slug| done(slug)), "{slugs:?}");
    assert!(first_done > 0, "every unit read as finished: {slugs:?}");
}

/// Time the ten-unit list and print the number, pass or fail.
///
/// The list is the command a shell calls most, so what it costs is a fact worth
/// recording on every run. No threshold is asserted: the number depends on the machine
/// and on how many homes Git has to answer for, and a gate on it would be calibrated
/// against one runner (`ci/startup-budget.sh` states that argument for the fast path).
#[test]
fn the_ten_unit_list_is_timed_and_the_number_is_printed() {
    let fixture = Fixture::new();
    assert_eq!(fixture.text(&["ls"]).lines().skip(1).count(), UNITS);

    let mut timings = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let status = fixture.command(&["ls"]).output().unwrap().status;
        timings.push(started.elapsed());
        assert!(status.success());
    }
    timings.sort_unstable();
    println!(
        "list: {UNITS} units · {RUNS} runs · median {} · min {} · max {}",
        milliseconds(timings[RUNS / 2]),
        milliseconds(timings[0]),
        milliseconds(timings[RUNS - 1])
    );
}

/// A duration in milliseconds, to two places, so two runs compare.
fn milliseconds(duration: Duration) -> String {
    let tenths = duration.as_micros() / 10;
    format!("{}.{:02} ms", tenths / 100, tenths % 100)
}
