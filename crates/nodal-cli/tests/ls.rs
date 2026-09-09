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
//! 5. A project that `nodal init` has just written a recipe for is a project the list
//!    answers about, with no units in it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use nodal_core::model::{Digest, Project, ProjectId, ProjectName, Timestamp};
use nodal_core::store::{Store, projects};
use nodal_fixture::shapes;
use nodal_safety::rows;

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
            record(&opened, &project, &slug, index, &home);
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
        let mut command = state::nodal(self.directory.path());
        command.args(args).current_dir(&self.project).env("NODAL_STORE", &self.store);
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

    /// The list as the document a tool reads.
    ///
    /// Every count and every order in this file is read from here rather than from the
    /// lines of the table. A host that cannot read its process table adds a note under
    /// the table, so the number of printed lines is a property of the host and the
    /// number of units is not.
    fn answer(&self) -> serde_json::Value {
        let output = self.nodal(&["ls", "--json"]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).expect("--json is one document")
    }

    /// The slugs of the list, in the order the list put them in.
    fn slugs(&self) -> Vec<String> {
        self.answer()["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["slug"].as_str().unwrap().to_owned())
            .collect()
    }
}

/// Whether this host publishes a process table, and a word about it when it does not.
///
/// The same seam `crates/nodal-cli/tests/ps.rs` uses. Who is attached to a unit is read
/// from `/proc`, so on a host without one the answer is a note under the table and every
/// session cell is empty. That is the right answer, not a failure, and the assertions
/// about it say which host they are making a claim about.
fn has_proc() -> bool {
    cfg!(target_os = "linux")
}

/// Record one unit and the home it is materialised in.
fn record(store: &Store, project: &Project, slug: &str, index: usize, home: &Path) {
    let row = rows::Row { index, slug, branch: slug, home, host: rows::host() };
    drop(rows::record(store, project.id, &row));
}

#[test]
fn the_list_has_one_row_per_unit_and_says_what_merging_each_would_do() {
    let fixture = Fixture::new();
    let text = fixture.text(&["ls"]);

    assert_eq!(fixture.slugs().len(), UNITS, "{text}");
    for slug in fixture.slugs() {
        assert!(text.contains(&slug), "no row for {slug} in:\n{text}");
    }
    for word in ["done (ancestor)", "done (absorbed)", "conflict", "open"] {
        assert!(text.contains(word), "no {word} in:\n{text}");
    }
    assert!(text.contains("UNIT"), "{text}");
    assert!(text.contains("MAIN"), "{text}");
}

#[test]
fn a_host_that_cannot_read_its_process_table_says_so_under_the_table() {
    let fixture = Fixture::new();
    let text = fixture.text(&["ls"]);
    let answer = fixture.answer();
    let notes: Vec<&str> =
        answer["notes"].as_array().unwrap().iter().map(|note| note.as_str().unwrap()).collect();

    if has_proc() {
        assert!(notes.is_empty(), "a host with /proc has nothing to report: {notes:?}");
        return;
    }
    let note = notes.first().expect("a host without /proc says it could not see");
    assert!(note.starts_with("who: "), "{note}");
    assert!(text.contains(note), "the note is under the table:\n{text}");
    let empty = answer["units"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["sessions"].as_array().unwrap().is_empty());
    assert!(empty, "no session is claimed on a host that cannot see one");
}

#[test]
fn a_bare_nodal_is_the_list_and_the_help_where_there_is_no_project() {
    let fixture = Fixture::new();
    assert_eq!(fixture.text(&[]), fixture.text(&["ls"]));

    let output = state::nodal(fixture.directory.path())
        .current_dir(fixture.outside())
        .env("NODAL_STORE", &fixture.store)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("Usage:"), "{text}");
}

#[test]
fn the_json_answer_carries_every_fact_the_table_shows() {
    let fixture = Fixture::new();
    let answer = fixture.answer();

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
    let slugs = fixture.slugs();

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
    assert_eq!(fixture.slugs().len(), UNITS);

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

/// The reported defect: `nodal ls`, run in the second after `nodal init` wrote the
/// recipe, said the directory was in no project Nodal knows and told the person to run
/// `nodal new` — which is what they had just been told `nodal init` was for.
///
/// The decision made here is that the registry stays as it was and the answer changes.
/// `nodal init` writes one file and opens nothing; the project row arrives with the
/// first unit. So there are three states, not two, and the third — declared, with no
/// units — gets the empty list rather than the error for a directory Nodal has never
/// heard of.
#[test]
fn a_project_that_init_has_just_written_answers_with_an_empty_list() {
    let directory = tempfile::tempdir().unwrap();
    let root = shapes::origin(directory.path().join("origin"));
    let store = directory.path().join("registry.db");
    let run = |args: &[&str]| {
        state::nodal(directory.path())
            .args(args)
            .current_dir(&root)
            .env("NODAL_STORE", &store)
            .output()
            .unwrap()
    };

    // Before the recipe there is nothing here, and the error says so.
    let before = run(&["ls"]);
    assert!(!before.status.success(), "a directory with no recipe and no rows is no project");
    let refused = String::from_utf8_lossy(&before.stderr).into_owned();
    assert!(refused.contains("is in no project Nodal knows"), "{refused}");

    let written = run(&["init", "--no-claude-hooks"]);
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stderr));

    assert_lists_nothing_yet(&run(&["ls"]));
    assert_json_is_the_ordinary_empty_list(&run(&["ls", "--json"]));
    // A bare `nodal` is the same answer, not the help.
    assert_lists_nothing_yet(&run(&[]));
}

/// The answer a person reads for a project that has been set up and has no units.
fn assert_lists_nothing_yet(output: &Output) {
    assert!(
        output.status.success(),
        "ls refused a project init had just written: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(text.contains("no units yet"), "{text}");
    assert!(!text.contains("no project Nodal knows"), "{text}");
    assert!(text.contains("nodal new"), "it says what makes the first unit: {text}");
    assert!(!text.contains("Usage:"), "a project that has been set up is not shown the help");
}

/// The empty list is the ordinary list, so a tool reads the shape it always does.
fn assert_json_is_the_ordinary_empty_list(output: &Output) {
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("--json is one document");
    assert_eq!(document["units"].as_array().unwrap().len(), 0);
    assert_eq!(document["project"], "origin");
    assert_eq!(document["notes"].as_array().unwrap().len(), 1);
}

/// The other half of the same rule: a directory with neither a recipe nor a row still
/// gets the error, and the error is unchanged.
#[test]
fn a_directory_that_is_no_project_at_all_still_says_so() {
    let fixture = Fixture::new();
    let output = state::nodal(fixture.directory.path())
        .args(["ls"])
        .current_dir(fixture.outside())
        .env("NODAL_STORE", &fixture.store)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("is in no project Nodal knows"), "{stderr}");
}
