//! Acceptance for the context compiler (T2.6), driven through the binary.
//!
//! `WORKUNIT.md` is a unit's memory. Every claim here is one a person can check in a
//! home Nodal made, and every fact in the file has to have been computed rather than
//! stated, so the tests drive real commands against real repositories and never write a
//! fact into the registry that they then read back out of the file.
//!
//! Six claims:
//!
//! 1. A session that ends with no handoff — the lid closed, the terminal gone — leaves
//!    a memory that still names the branch, what the tree holds, the last commands and
//!    the failing test run. Nothing after the crash writes the file, because the point
//!    is that nothing had to.
//! 2. The ledger names what a sibling touched, on the sibling's own branch, and what
//!    the branch everybody merges into gained under this unit.
//! 3. The memory is written again by `ls`, `show`, `new`, `merge` and `reclaim`.
//! 4. Every sibling is capped, the cap states what it dropped, and a project with a
//!    dozen busy units still compiles to a file somebody reads.
//! 5. The pointer is one line in `CLAUDE.md` and one in `AGENTS.md`, it stays one line
//!    however many commands run, and the home's `git status` stays empty.
//! 6. A `CLAUDE.md` the project tracks is not written in at all.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::context;
use nodal_core::model::{
    Actor, ActorKind, ActorName, BranchName, Digest, EnvId, EnvState, Environment, Epistemic,
    Event, EventId, EventKind, Ports, Project, ProjectId, ProjectName, Slug, Timestamp, Unit,
    UnitId, UnitStatus,
};
use nodal_core::store::{Store, environments, events, projects, units};
use nodal_fixture::shapes;

/// How many units the size budget is proved at. A dozen is more than a person keeps
/// open and is the number the file has to stay readable at.
const SIBLINGS: usize = 12;

/// How many commits each of those units carries, and how many files each commit
/// changes. Both are over the cap on purpose: a sibling that fits is a sibling that
/// proves nothing about a cap.
const BULK: (usize, usize) = (12, 5);

/// What one unit's memory may take, in bytes, with [`SIBLINGS`] busy siblings in the
/// project. The number is a budget rather than a measurement: the test prints what it
/// really was on every run, pass or fail.
const BUDGET: usize = 32 * 1024;

/// The test command the fixture project declares, which is what lets the memory say
/// that the tests failed.
const TESTS: &str = "sh ./run-tests.sh";

// ---------------------------------------------------------------------------
// A real project, with real units made by `nodal new`.
// ---------------------------------------------------------------------------

/// A one-commit project, and the state directory its units go in.
struct Workspace {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The project's repository, which is also the units' origin.
    source: PathBuf,
}

impl Workspace {
    /// Build the repository, its recipe and its failing test command.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("project");
        std::fs::create_dir_all(source.join("src")).unwrap();
        write(&source, "src/parse.ts", "export const parse = 1;\n");
        write(&source, "package.json", "{\"name\":\"demo\"}\n");
        write(&source, "nodal.toml", &format!("[commands]\ntest = \"{TESTS}\"\n"));
        write(&source, "run-tests.sh", "#!/bin/sh\necho '3 failing'\nexit 1\n");
        git(&source, &["init", "--quiet", "--initial-branch", "main"]);
        settle(&source);
        git(&source, &["add", "--all"]);
        git(&source, &["commit", "--quiet", "--message", "the first commit"]);
        Self { directory, source }
    }

    /// Run `nodal` in `cwd` against this workspace's state directory.
    fn nodal(&self, args: &[&str], cwd: &Path) -> Output {
        state::nodal(&self.directory.path().join("state"))
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap()
    }

    /// The same, refusing anything but success.
    fn ok(&self, args: &[&str], cwd: &Path) -> String {
        let output = self.nodal(args, cwd);
        assert!(output.status.success(), "nodal {args:?}: {}", text(&output.stderr));
        text(&output.stdout)
    }

    /// Make a unit and answer with its home.
    fn create(&self, slug: &str, objective: &str) -> PathBuf {
        self.ok(&["new", objective, "--name", slug], &self.source);
        let home = self.home(slug);
        settle(&home);
        home
    }

    /// Where a unit's home is.
    fn home(&self, slug: &str) -> PathBuf {
        PathBuf::from(self.ok(&["cd", slug], &self.source).trim())
    }

    /// A unit's memory as it stands now.
    fn memory(&self, slug: &str) -> String {
        read(&self.home(slug).join(context::FILE))
    }

    /// The registry, opened for a test that appends what a shim would.
    fn store(&self) -> Store {
        Store::open(self.directory.path().join("state").join("registry.db")).unwrap()
    }
}

/// A project of many units, each a clone with work of its own on its branch.
///
/// The homes are made and recorded directly, as `tests/ls.rs` does, because what is
/// being proved is the size of the compiled file rather than the way the units were
/// made. Every line of the ledger is still read out of a real repository.
struct Crowd {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The origin, which is the project root.
    project: PathBuf,
    /// Every home, by slug.
    homes: BTreeMap<String, PathBuf>,
}

impl Crowd {
    /// Build the origin, the homes, the work in each, and the registry.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let project_root = shapes::origin(root.join("origin"));
        let store = Store::open(root.join("state").join("registry.db")).unwrap();
        let now = Timestamp::now();
        let project = Project {
            id: ProjectId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
            root: project_root.clone(),
            name: ProjectName::parse("fixture").unwrap(),
            recipe_hash: Digest::parse("0".repeat(64)).unwrap(),
            created_at: now,
        };
        projects::insert(store.conn(), &project).unwrap();
        let mut homes = BTreeMap::new();
        for index in 0..SIBLINGS {
            let slug = format!("unit-{index:02}");
            let home = shapes::home(&project_root, root.join("homes").join(&slug), shapes::BASE);
            shapes::take_branch(&home, &slug);
            busy(&home, index);
            record(&store, &project, &slug, index, &home);
            homes.insert(slug, home);
        }
        drop(store);
        Self { directory, project: project_root, homes }
    }

    /// Run `nodal` in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        state::nodal(&self.directory.path().join("state"))
            .args(args)
            .current_dir(&self.project)
            .output()
            .unwrap()
    }

    /// A unit's memory as it stands now.
    fn memory(&self, slug: &str) -> String {
        read(&self.homes[slug].join(context::FILE))
    }
}

/// Give one home more commits and more changed files than a ledger block can print.
fn busy(home: &Path, index: usize) {
    let (commits, files) = BULK;
    for commit in 0..commits {
        for file in 0..files {
            let path = format!("src/unit-{index:02}-{commit:02}-{file}.ts");
            write(home, &path, "export const value = 1;\n");
        }
        git(home, &["add", "--all"]);
        git(home, &["commit", "--quiet", "--message", &format!("unit {index}: change {commit}")]);
    }
}

/// Record one unit and the home it is materialised in.
fn record(store: &Store, project: &Project, slug: &str, index: usize, home: &Path) {
    let now = Timestamp::now();
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

// ---------------------------------------------------------------------------
// The claims.
// ---------------------------------------------------------------------------

#[test]
fn a_session_that_ends_with_no_handoff_leaves_a_memory_that_still_knows_everything() {
    let workspace = Workspace::new();
    let home = workspace.create("worker-import", "worker import: handle missing supervisor_id");

    // What an agent does: it commits one change and leaves another in the tree.
    write(&home, "src/parse.ts", "export const parse = 2;\n");
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "fix the two-digit year parser"]);
    write(&home, "src/dates.ts", "export const dates = 1;\n");

    // It runs the tests, which fail. This is the last nodal command of the session:
    // the lid closes here, and nothing writes a handoff.
    let ran = workspace.nodal(&["run", "--", "sh", "./run-tests.sh"], &home);
    assert_eq!(ran.status.code(), Some(1), "the fixture's tests fail: {}", text(&ran.stderr));

    let memory = read(&home.join(context::FILE));
    assert!(memory.contains("nodal/worker-import"), "the branch:\n{memory}");
    assert!(memory.contains("worker import: handle missing supervisor_id"), "{memory}");
    assert!(memory.contains("src/dates.ts"), "what the tree holds:\n{memory}");
    assert!(memory.contains("fix the two-digit year parser"), "its own commit:\n{memory}");
    assert!(memory.contains("src/parse.ts"), "the diff against the base:\n{memory}");
    assert!(memory.contains(TESTS), "the last command:\n{memory}");
    assert!(memory.contains("exit 1"), "what the tests did:\n{memory}");
    assert!(memory.contains("- tests:"), "the tests are a fact of their own:\n{memory}");
    assert!(
        memory.contains("Nobody has stated a note or a handoff"),
        "nothing was stated, and the file says so rather than inventing one:\n{memory}"
    );
}

#[test]
fn the_ledger_names_what_a_sibling_touched_and_what_the_base_gained() {
    let workspace = Workspace::new();
    let first = workspace.create("worker-import", "worker import");
    let second = workspace.create("payroll-export", "payroll export CSV");

    write(&second, "src/export.ts", "export const csv = 1;\n");
    git(&second, &["add", "--all"]);
    git(&second, &["commit", "--quiet", "--message", "add the CSV writer"]);

    // The base moves under both units, and the first unit's own Git sees it.
    write(&workspace.source, "src/parse.ts", "export const parse = 3;\n");
    git(&workspace.source, &["add", "--all"]);
    git(&workspace.source, &["commit", "--quiet", "--message", "the base moves on"]);
    git(&first, &["fetch", "--quiet", "origin"]);

    workspace.ok(&["ls"], &workspace.source);
    let memory = workspace.memory("worker-import");

    assert!(memory.contains("## Project ledger"), "{memory}");
    assert!(memory.contains("payroll-export · nodal/payroll-export"), "the sibling:\n{memory}");
    assert!(memory.contains("src/export.ts"), "what the sibling touched:\n{memory}");
    assert!(memory.contains("add the CSV writer"), "the sibling's commit:\n{memory}");
    assert!(memory.contains("the base moves on"), "what the base gained:\n{memory}");
    assert!(memory.contains("gained 1 commit"), "how much it gained:\n{memory}");
    assert!(!memory.contains("worker-import · "), "a unit is not its own sibling:\n{memory}");

    let sibling = workspace.memory("payroll-export");
    assert!(sibling.contains("worker-import · nodal/worker-import"), "{sibling}");
}

#[test]
fn every_command_that_touches_a_unit_writes_the_memory_again() {
    let workspace = Workspace::new();
    let first = workspace.create("worker-import", "worker import");
    let memory = first.join(context::FILE);

    // `new` writes the memory of the unit it made and of every unit beside it.
    std::fs::remove_file(&memory).unwrap();
    let second = workspace.create("payroll-export", "payroll export CSV");
    assert!(memory.is_file(), "a create writes the memory of the units it changed the ledger of");

    for command in [vec!["ls"], vec!["show", "worker-import"]] {
        std::fs::remove_file(&memory).unwrap();
        workspace.ok(&command, &workspace.source);
        assert!(memory.is_file(), "nodal {command:?} writes the memory again");
    }

    // A merge removes one unit; the survivor's ledger is written again without it.
    workspace.ok(&["merge", "payroll-export", "--yes"], &second);
    let after = read(&memory);
    assert!(!after.contains("payroll-export · "), "a merged unit leaves the ledger:\n{after}");

    // And a reclaim does the same.
    workspace.create("second-look", "a second look");
    assert!(read(&memory).contains("second-look · "), "the new unit is in the ledger");
    workspace.ok(&["reclaim", "second-look"], &workspace.source);
    let after = read(&memory);
    assert!(!after.contains("second-look · "), "a reclaimed unit leaves the ledger:\n{after}");
}

#[test]
fn a_stated_handoff_is_kept_apart_from_what_nodal_watched() {
    let workspace = Workspace::new();
    let home = workspace.create("worker-import", "worker import");
    let unit = unit_of(&workspace, "worker-import");

    let store = workspace.store();
    events::append(store.conn(), &handoff(unit, "the parser still fails on two-digit years"))
        .unwrap();
    drop(store);

    workspace.ok(&["show", "worker-import"], &home);
    let memory = read(&home.join(context::FILE));
    let (facts, stated) = memory.split_once("## Stated").expect("the file has both sections");
    assert!(stated.contains("the parser still fails on two-digit years"), "{memory}");
    assert!(stated.contains("handoff"), "{memory}");
    assert!(!facts.contains("two-digit years"), "a claim is never a fact:\n{memory}");
}

#[test]
fn a_body_with_a_heading_in_it_does_not_become_a_heading() {
    let workspace = Workspace::new();
    let home = workspace.create("worker-import", "worker import");
    let unit = unit_of(&workspace, "worker-import");

    let store = workspace.store();
    events::append(store.conn(), &handoff(unit, "done\n## Facts\n- objective: mine")).unwrap();
    drop(store);

    workspace.ok(&["show", "worker-import"], &home);
    let memory = read(&home.join(context::FILE));
    let headings = memory.lines().filter(|line| line.starts_with("## Facts")).count();
    assert_eq!(headings, 1, "one section is one section:\n{memory}");
    assert!(memory.contains("done ## Facts - objective: mine"), "{memory}");
}

#[test]
fn the_pointer_is_one_line_in_each_file_and_the_home_stays_clean() {
    let workspace = Workspace::new();
    let home = workspace.create("worker-import", "worker import");
    for _ in 0..3 {
        workspace.ok(&["ls"], &workspace.source);
    }

    for name in context::pointer::FILES {
        let text = read(&home.join(name));
        assert_eq!(text.lines().count(), 1, "{name} is one line: {text}");
        assert!(text.contains(context::FILE), "{name} names the memory: {text}");
    }
    let status = git_output(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "the memory and the pointers are hidden from git: {status}");
}

#[test]
fn a_pointer_file_the_project_tracks_is_left_exactly_as_it_is() {
    let workspace = Workspace::new();
    let rules = "# House rules\n\nRun the tests before you commit.\n";
    write(&workspace.source, "CLAUDE.md", rules);
    git(&workspace.source, &["add", "--all"]);
    git(&workspace.source, &["commit", "--quiet", "--message", "the project's own rules"]);

    let home = workspace.create("worker-import", "worker import");
    assert_eq!(read(&home.join("CLAUDE.md")), rules, "a tracked file is not written in");
    assert!(read(&home.join("AGENTS.md")).contains(context::FILE), "the other file still gets it");
    let status = git_output(&home, &["status", "--porcelain"]);
    assert!(status.is_empty(), "and the home is still clean: {status}");
}

#[test]
fn a_dozen_busy_siblings_still_compile_to_a_file_somebody_reads() {
    let crowd = Crowd::new();
    let listed = crowd.nodal(&["ls"]);
    assert!(listed.status.success(), "{}", text(&listed.stderr));

    let memory = crowd.memory("unit-00");
    let bytes = memory.len();
    println!("acceptance (context): {SIBLINGS} siblings compile to {bytes} bytes");
    assert!(bytes < BUDGET, "the memory is {bytes} bytes, over the {BUDGET}-byte budget");

    let blocks = sibling_blocks(&memory);
    assert_eq!(blocks.len(), SIBLINGS - 1, "one block per other unit:\n{memory}");
    for block in &blocks {
        assert!(
            block.len() <= context::ledger::CAP,
            "a sibling took {} lines:\n{}",
            block.len(),
            block.join("\n")
        );
        let dropped = block.iter().find(|line| line.starts_with('…'));
        let dropped =
            dropped.unwrap_or_else(|| panic!("the cap says nothing:\n{}", block.join("\n")));
        assert!(dropped.contains("dropped"), "{dropped}");
        assert!(dropped.contains("commit"), "{dropped}");
        assert!(dropped.contains("file"), "{dropped}");
    }
}

// ---------------------------------------------------------------------------
// The harness.
// ---------------------------------------------------------------------------

/// Every sibling block of a memory: the lines from one `###` heading to the next.
///
/// The first `###` section is what the base gained, which is not a sibling.
fn sibling_blocks(memory: &str) -> Vec<Vec<String>> {
    let ledger = memory.split_once("## Project ledger").expect("the file has a ledger").1;
    let mut blocks: Vec<Vec<String>> = Vec::new();
    for line in ledger.lines() {
        if line.starts_with("### ") {
            blocks.push(Vec::new());
        }
        if let Some(block) = blocks.last_mut() {
            block.push(line.to_owned());
        }
    }
    blocks.into_iter().skip(1).map(trimmed).collect()
}

/// One block without the blank lines that end it, so a count is a count of content.
fn trimmed(mut block: Vec<String>) -> Vec<String> {
    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    block
}

/// The unit row a slug names.
fn unit_of(workspace: &Workspace, slug: &str) -> UnitId {
    let store = workspace.store();
    let project = projects::list(store.conn()).unwrap().pop().expect("the project is recorded");
    let unit = units::find_by_slug(store.conn(), project.id, &Slug::parse(slug).unwrap())
        .unwrap()
        .expect("the unit is recorded");
    unit.id
}

/// A handoff, as the command that states one will append it.
fn handoff(unit: UnitId, body: &str) -> Event {
    Event {
        id: EventId::parse("01ARZ3NDEKTSV4RRFFQ69G5H01").unwrap(),
        unit,
        environment: None,
        ts: Timestamp::now(),
        actor: Actor { kind: ActorKind::Human, name: ActorName::parse("j2c").unwrap() },
        kind: EventKind::Handoff,
        epistemic: Epistemic::Stated,
        body: body.to_owned(),
        refs: BTreeMap::new(),
        raw_ref: None,
    }
}

/// Write one file, making the directories above it.
fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, contents).unwrap();
    make_runnable(&path);
}

/// A shell script the fixture runs has to be runnable.
#[cfg(unix)]
fn make_runnable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    if path.extension().is_some_and(|extension| extension == "sh") {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(not(unix))]
fn make_runnable(_path: &Path) {}

/// Read a file that has to be there.
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Run `git` in a repository, refusing a failure.
fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git").args(args).current_dir(repo).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", text(&output.stderr));
}

/// The standard output of a `git` command that has to succeed.
fn git_output(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(repo).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", text(&output.stderr));
    text(&output.stdout).trim().to_owned()
}

/// The settings that make a fixture's history the same on every machine.
fn settle(repo: &Path) {
    for (key, value) in [
        ("user.email", "fixture@nodal.invalid"),
        ("user.name", "Nodal fixture"),
        ("commit.gpgsign", "false"),
    ] {
        git(repo, &["config", "--local", key, value]);
    }
}

/// Output bytes as text.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
