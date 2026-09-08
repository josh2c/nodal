//! Acceptance for T1.0: `nodal doctor` reports what a machine has left behind.
//!
//! The machine is built here rather than found: a checkout with two nested worktrees,
//! one of them locked, a build cache nothing has written to for a month, an orphan
//! database directory under Nodal's state, and a Docker that answers with one exited
//! container and one unreferenced volume. Every claim of the task is then a check
//! against that one machine.
//!
//! The last check is the one the command exists for. Doctor is read-only
//! (`decisions/DL-015`), so this records the name, the size and the modification time of
//! every path of the machine before the report and compares them after it. A report that
//! changed one byte of the machine fails here.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use nodal_core::doctor::{self, Machine};
use nodal_core::model::Timestamp;
use nodal_core::output::view::doctor::{Doctor, Finding, Kind};
use nodal_core::services::docker::{Docker, Output};
use nodal_core::store::Store;

/// The container the fake daemon reports as exited, with a writable layer of 120 MB.
const EXITED: &str = concat!(
    r#"{"Name":"/acme-replay-db","Config":{"Image":"postgres:16","Labels":null},"#,
    r#""State":{"FinishedAt":"2026-08-10T09:00:00Z"},"SizeRw":120000000,"Mounts":[]}"#,
    "\n"
);

/// One volume nothing refers to, and one two containers do.
const DISK_USAGE: &str = concat!(
    r#"{"Volumes":[{"Name":"acme_pgdata","Links":0,"Size":"1.4GB"},"#,
    r#"{"Name":"live_pgdata","Links":2,"Size":"800MB"}]}"#,
    "\n"
);

/// The name of the orphan database directory the machine holds.
const ORPHAN: &str = "nodal_app_01j8z6h0";

/// A Docker that answers from the fixtures above.
struct Daemon;

impl Docker for Daemon {
    fn run(&self, args: &[&str]) -> nodal_core::Result<Output> {
        let stdout = match args.first().copied() {
            Some("ps") => String::from("2f6c9a\n"),
            Some("inspect") => String::from(EXITED),
            Some("system") => String::from(DISK_USAGE),
            _ => String::new(),
        };
        Ok(Output { stdout, stderr: String::new(), code: Some(0) })
    }
}

/// A Docker that is not installed on this machine.
struct NoDocker;

impl Docker for NoDocker {
    fn run(&self, _args: &[&str]) -> nodal_core::Result<Output> {
        Err(nodal_core::Error::ToolSpawn {
            program: String::from("docker"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })
    }
}

/// A machine with everything doctor is meant to find on it.
struct Planted {
    /// Holds the whole machine; dropping it removes it.
    directory: tempfile::TempDir,
    /// The checkout the command is run in.
    checkout: PathBuf,
    /// Nodal's state directory.
    state: PathBuf,
    /// Claude Code's records.
    sessions: PathBuf,
    /// The registry, which holds nothing: doctor runs before any unit exists.
    store: Store,
    /// A symbolic link to the machine root: the same directories under another name.
    link: PathBuf,
}

impl Planted {
    /// The root of the whole planted machine.
    fn root(&self) -> &Path {
        self.directory.path()
    }

    /// The report this machine produces, from a daemon that answers.
    fn report(&self) -> Doctor {
        self.report_with(&Daemon)
    }

    /// The report this machine produces from a given Docker.
    fn report_with(&self, docker: &dyn Docker) -> Doctor {
        self.report_from(docker, &self.checkout, &self.state)
    }

    /// The report produced by running in `cwd`, with `state` as the state directory.
    fn report_from(&self, docker: &dyn Docker, cwd: &Path, state: &Path) -> Doctor {
        let machine = Machine::here(cwd, state, Some(&self.sessions));
        doctor::survey(self.store.conn(), docker, &machine, Timestamp::now())
            .expect("a machine doctor can read")
    }
}

fn plant() -> Planted {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    let checkout = root.join("code/app");
    let state = root.join("state");
    let sessions = root.join("claude");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&state).unwrap();

    git(&checkout, &["init", "--quiet", "."]);
    write(&checkout.join("README.md"), "# app\n");
    commit(&checkout, "the project");

    // A remote, so that "pushed" and "unpushed" are answers about somewhere else and
    // not about a repository that has nowhere to be contained by.
    let remote = root.join("remote.git");
    git(root, &["init", "--quiet", "--bare", remote.to_str().unwrap()]);
    git(&checkout, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&checkout, &["push", "--quiet", "origin", "HEAD"]);

    // Two nested worktrees, the way another tool makes them. One is locked.
    git(&checkout, &["worktree", "add", "--quiet", "-b", "loose", ".claude/worktrees/loose"]);
    write(&checkout.join(".claude/worktrees/loose/note.md"), "unpushed work\n");
    commit(&checkout.join(".claude/worktrees/loose"), "work nobody else has");
    write(&checkout.join(".claude/worktrees/loose/dirty.md"), "uncommitted\n");
    // A third with nothing of its own in it: every commit it has is the project's, and
    // already on the remote. What doctor may say about it is that it is pushed.
    git(&checkout, &["worktree", "add", "--quiet", "-b", "fresh", ".claude/worktrees/fresh"]);
    git(&checkout, &["worktree", "add", "--quiet", "-b", "held", ".claude/worktrees/held"]);
    write(&checkout.join(".claude/worktrees/held/big.bin"), &"x".repeat(4096));
    git(
        &checkout,
        &["worktree", "lock", "--reason", "an agent is running here", ".claude/worktrees/held"],
    );

    // A build cache nothing has written to for a month.
    let cache = checkout.join("apps/web/.next/build.json");
    write(&cache, &"c".repeat(2048));
    age(&cache, Duration::from_secs(30 * 24 * 60 * 60));

    // A database directory under Nodal's state that no registry row names.
    write(&state.join("app").join(ORPHAN).join("base"), &"d".repeat(512));

    // The record of the session that made the loose worktree.
    //
    // Under the resolved path, because that is the path the tool itself writes: it
    // records the directory the operating system gives it, with every link already
    // followed. On a host whose temporary directory is a link this is not the path
    // this test built, which is the whole reason the symlink test below exists.
    let worktree = nodal_core::lifecycle::guard::resolve(&checkout.join(".claude/worktrees/loose"));
    let encoded = doctor::intent::encode(&worktree);
    write(
        &sessions.join("projects").join(encoded).join("s.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","isSidechain":false,"cwd":{cwd},"timestamp":"2026-08-19T23:37:11Z","#,
                r#""message":{{"role":"user","content":"Make the importer retry a failed row"}}}}"#,
                "\n"
            ),
            cwd = serde_json::to_string(&worktree).unwrap()
        ),
    );

    // A second name for the whole machine, so a test can reach it the way macOS does:
    // `/var/folders/...` is a link and `/private/var/folders/...` is the directory.
    let link = root.join("by-another-name");
    std::os::unix::fs::symlink(root, &link).unwrap();

    let store = Store::open(root.join("registry.db")).expect("a registry");
    Planted { directory, checkout, state, sessions, store, link }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "--all"]);
    git(
        dir,
        &[
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "user.name=test",
            "commit",
            "--quiet",
            "--message",
            message,
        ],
    );
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().expect("a parent")).unwrap();
    std::fs::write(path, body).unwrap();
}

/// Put a file's modification time `age` into the past.
fn age(path: &Path, age: Duration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

/// The one finding of a kind whose name holds `needle`.
fn one<'a>(findings: &'a [Finding], kind: Kind, needle: &str) -> &'a Finding {
    let matched: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.kind == kind && finding.what.contains(needle))
        .collect();
    assert_eq!(matched.len(), 1, "expected one {kind:?} holding {needle:?}: {findings:#?}");
    matched[0]
}

#[test]
fn every_kind_of_leftover_is_found_and_sized() {
    let machine = plant();
    let report = machine.report();

    let loose = one(&report.here, Kind::NestedWorktree, "loose");
    assert!(loose.bytes.is_some_and(|bytes| bytes > 0), "{loose:?}");
    assert!(loose.state.iter().any(|word| word == "unpushed 1"), "{:?}", loose.state);
    assert!(loose.state.iter().any(|word| word == "dirty 1"), "{:?}", loose.state);
    assert_eq!(loose.intent.as_deref(), Some("Make the importer retry a failed row"));

    let cache = one(&report.here, Kind::StaleCache, ".next");
    assert_eq!(cache.bytes, Some(2048));

    let container = one(&report.here, Kind::ExitedContainer, "acme-replay-db");
    assert_eq!(container.bytes, Some(120_000_000));

    let volume = one(&report.here, Kind::DanglingVolume, "acme_pgdata");
    assert_eq!(volume.bytes, Some(1_400_000_000), "docker's own figure, read as bytes");
    assert!(
        !report.here.iter().any(|finding| finding.what == "live_pgdata"),
        "a volume a container refers to is not a leftover"
    );

    let database = one(&report.here, Kind::OrphanDatabase, ORPHAN);
    assert_eq!(database.bytes, Some(512));
    assert_eq!(database.what, format!("app/{ORPHAN}"), "named by where it sits under the state");
}

#[test]
fn a_locked_worktree_is_reported_as_locked_and_read_no_further() {
    let machine = plant();
    let report = machine.report();
    let held = one(&report.here, Kind::NestedWorktree, "held");
    assert!(held.state.iter().any(|word| word == "locked"), "{:?}", held.state);
    assert!(
        held.state.iter().any(|word| word == "an agent is running here"),
        "the reason the tool gave is reported: {:?}",
        held.state
    );
    assert_eq!(held.bytes, None, "a locked worktree is not walked, so it has no size");
    assert!(held.intent.is_none(), "a locked worktree is not read for an intent");
    for word in ["pushed", "unpushed 0", "dirty", "behind"] {
        assert!(!held.state.iter().any(|said| said == word), "{word} was read from a locked tree");
    }
}

/// Doctor reports the fact it read, and no claim the fact does not support.
///
/// A worktree with no commits of its own is contained by every remote, because the
/// commits it is made of are the project's and were pushed with the project. That is
/// worth saying and it is `pushed`. It is not `merged`: nothing of that worktree has
/// been merged anywhere, and the word would be a judgement about work that does not
/// exist. The same vacuous containment recorded units as merged at the moment they were
/// created (`crate::lifecycle::states`); here it never decided anything, but it was
/// still saying something untrue.
#[test]
fn a_worktree_with_no_commits_of_its_own_is_reported_as_pushed_and_never_as_merged() {
    let machine = plant();
    let report = machine.report();
    let fresh = one(&report.here, Kind::NestedWorktree, "fresh");

    assert!(fresh.state.iter().any(|word| word == "pushed"), "{:?}", fresh.state);
    for word in ["merged", "unmerged"] {
        assert!(
            !fresh.state.iter().any(|said| said.starts_with(word)),
            "{word:?} is a claim about work this worktree has not done: {:?}",
            fresh.state
        );
    }
}

#[test]
fn another_projects_leftovers_are_a_section_of_their_own_with_names_and_sizes_only() {
    let machine = plant();
    let other = machine.root().join("code/other");
    write(&other.join("README.md"), "# other\n");
    git(&other, &["init", "--quiet", "."]);
    commit(&other, "the other project");
    git(&other, &["worktree", "add", "--quiet", "-b", "w", ".claude/worktrees/w"]);
    nodal_core::store::projects::insert(machine.store.conn(), &project_row(&other))
        .expect("a second project");

    let report = machine.report();
    assert!(
        report.here.iter().all(|finding| !finding.what.contains("code/other")),
        "another project's worktree is in this project's section: {:#?}",
        report.here
    );
    let elsewhere = one(&report.elsewhere, Kind::NestedWorktree, ".claude/worktrees/w");
    assert!(elsewhere.bytes.is_some(), "another project's leftover is still sized");
    assert!(elsewhere.state.is_empty(), "the second section states nothing: {elsewhere:?}");
    assert!(elsewhere.intent.is_none(), "the second section recovers no intent");
}

/// A project row for a root, with the fields doctor reads and defaults for the rest.
fn project_row(root: &Path) -> nodal_core::model::Project {
    nodal_core::model::Project {
        id: nodal_core::model::ProjectId::from_ulid(ulid::Ulid::new()),
        root: root.to_owned(),
        name: nodal_core::model::ProjectName::parse("other").expect("a name"),
        recipe_hash: nodal_core::model::Digest::parse("0".repeat(64)).expect("a digest"),
        created_at: Timestamp::now(),
    }
}

#[test]
fn a_project_over_the_unit_threshold_is_a_row_of_its_own() {
    let machine = plant();
    let project = project_row(&machine.checkout);
    nodal_core::store::projects::insert(machine.store.conn(), &project).expect("the project");
    let over = doctor::units::LIMIT + 1;
    for index in 0..over {
        let unit = unit_row(project.id, index);
        nodal_core::store::units::insert(machine.store.conn(), &unit).expect("a unit");
    }

    let report = machine.report();
    let counted = one(&report.here, Kind::UnitCount, "other");
    assert!(
        counted.state.iter().any(|word| word.contains(&format!("{over} open units"))),
        "{:?}",
        counted.state
    );
    assert!(counted.bytes.is_some(), "the disk the homes hold is measured");
}

/// One open unit of a project, named by its index so that every slug and branch differ.
fn unit_row(project: nodal_core::model::ProjectId, index: usize) -> nodal_core::model::Unit {
    let now = Timestamp::now();
    nodal_core::model::Unit {
        id: nodal_core::model::UnitId::from_ulid(ulid::Ulid::new()),
        project_id: project,
        slug: nodal_core::model::Slug::parse(format!("unit-{index}")).expect("a slug"),
        objective: None,
        objective_epistemic: None,
        branch: nodal_core::model::BranchName::parse(format!("nodal/unit-{index}"))
            .expect("a branch"),
        parent_branch: None,
        status: nodal_core::model::UnitStatus::Open,
        created_at: now,
        updated_at: now,
    }
}

/// The macOS condition, reproduced on every host.
///
/// On macOS the temporary directory is a link: a test builds its machine under
/// `/var/folders/...` and the directory is `/private/var/folders/...`. Git and Docker
/// answer with the second name, and a registry row holds the first. Every comparison
/// doctor makes is then between two names for one directory.
///
/// So this runs the whole survey through a second name for the machine: the checkout is
/// entered through a link, the state directory is named through the link, and the other
/// project's row holds a path through the link. The report must be the same report.
#[test]
fn a_machine_reached_by_another_name_reports_the_same_things() {
    let machine = plant();
    let by_link = |relative: &str| machine.link.join(relative);

    // A second project, whose row holds a path nothing resolved.
    let other = machine.root().join("code/other");
    write(&other.join("README.md"), "# other\n");
    git(&other, &["init", "--quiet", "."]);
    commit(&other, "the other project");
    git(&other, &["worktree", "add", "--quiet", "-b", "w", ".claude/worktrees/w"]);
    nodal_core::store::projects::insert(machine.store.conn(), &project_row(&by_link("code/other")))
        .expect("a second project");

    let report = machine.report_from(&Daemon, &by_link("code/app"), &by_link("state"));

    // This project, reached through the link, is still this project.
    let loose = one(&report.here, Kind::NestedWorktree, "loose");
    assert_eq!(loose.what, ".claude/worktrees/loose", "named relative to the checkout");
    assert_eq!(
        loose.intent.as_deref(),
        Some("Make the importer retry a failed row"),
        "the session record is found under the name the tool wrote it with"
    );
    assert!(one(&report.here, Kind::StaleCache, ".next").bytes.is_some());
    assert!(one(&report.here, Kind::OrphanDatabase, ORPHAN).bytes.is_some());

    // The other project's row named a path through the link. It is still another
    // project, and its worktree is still in the second section.
    assert!(one(&report.elsewhere, Kind::NestedWorktree, ".claude/worktrees/w").bytes.is_some());
    assert!(
        report.here.iter().all(|finding| !finding.what.contains("code/other")),
        "another project's worktree reached the first section: {:#?}",
        report.here
    );
}

#[test]
fn a_machine_with_no_docker_gets_a_note_and_the_rest_of_the_report() {
    let machine = plant();
    let report = machine.report_with(&NoDocker);
    assert_eq!(report.notes.len(), 1);
    assert_eq!(report.notes[0].source, "docker");
    assert_eq!(report.notes[0].why, "docker is not installed");
    assert!(
        !report.here.iter().any(|finding| finding.kind == Kind::ExitedContainer),
        "a machine with no daemon reports no container"
    );
    assert!(
        report.here.iter().any(|finding| finding.kind == Kind::NestedWorktree),
        "the rest of the report is unaffected"
    );
}

#[test]
fn the_report_writes_nothing_anywhere() {
    let machine = plant();
    let before = snapshot(machine.root());
    let report = machine.report();
    assert!(!report.here.is_empty(), "the machine was read");
    let after = snapshot(machine.root());

    let missing: Vec<&PathBuf> = before.keys().filter(|path| !after.contains_key(*path)).collect();
    assert!(missing.is_empty(), "doctor removed {missing:#?}");
    let added: Vec<&PathBuf> = after.keys().filter(|path| !before.contains_key(*path)).collect();
    assert!(added.is_empty(), "doctor wrote {added:#?}");
    for (path, was) in &before {
        assert_eq!(after.get(path), Some(was), "doctor changed {}", path.display());
    }
}

/// Every path under `root`, with the size and the modification time of each.
///
/// The registry file is left out. Opening it is a write by design — SQLite creates its
/// write-ahead log — and the command line opens the registry before doctor is called,
/// which is what `nodal ps` and every other read command do too. Everything else on the
/// machine has to come back unchanged.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("registry.db"))
            {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                queue.push(path.clone());
            }
            found.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
    found
}
