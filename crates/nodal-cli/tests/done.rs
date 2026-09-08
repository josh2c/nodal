//! Acceptance test for T2.8: the states a unit passes through after the work is done.
//!
//! Every assertion here is about something a person can check for themselves. The refs
//! are read out of a real remote repository. The unit's state is read out of the
//! registry, not out of the words a command printed, because the point of recording the
//! flip is that it is a fact and not a rendering. The home is looked at on disk.
//!
//! The remote is a bare repository in the same temporary directory, so a test pushes
//! for real and reaches no network. The one test that needs a *named* host — the compare
//! URL for a GitHub remote — gets it the way Git itself offers: `url.<local>.insteadOf`
//! rewrites the GitHub address to that bare repository, so `origin` is genuinely
//! `https://github.com/...` as far as everything that reads it is concerned, and the
//! push still lands next door.
//!
//! Four of these are what the operation exists for.
//!
//! `done` **pushes both refs and opens nothing**. The branch and the work-in-progress
//! ref are on the remote afterwards, the compare page is printed, and no pull request
//! is opened — which is also asserted against the source itself, because "we did not
//! call an API" is only worth as much as there being no code that could.
//!
//! A **squash-merged unit flips to merged** once its branch is on the remote, and the
//! flip is in the registry.
//!
//! **Retention is what `gc` waits for**, and the uniqueness check still applies: a
//! merged unit somebody has put new work in is refused and named.
//!
//! An **idle live unit is reported and left alone**: its home is where it was and its
//! process is still running.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

mod state;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use nodal_core::model::UnitStatus;
use nodal_core::store::{Store, projects, units};
use tempfile::TempDir;

/// The address the compare-page test pretends its remote lives at.
const GITHUB: &str = "https://github.com/team/project.git";

/// A project with a remote, and the state directory its units go in.
struct Workspace {
    /// The temporary root, kept so that it outlives the test.
    _root: TempDir,
    /// The bare repository the project pushes to.
    remote: PathBuf,
    /// The project's own checkout.
    source: PathBuf,
    /// Nodal's state directory: the registry, every home, and the trash.
    state: PathBuf,
}

impl Workspace {
    /// A one-commit repository, a bare remote it has been pushed to, and an empty state
    /// directory beside them.
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let remote = root.path().join("remote.git");
        let source = root.path().join("project");
        let state = root.path().join("state");
        git(root.path(), &["init", "-q", "--bare", remote.to_str().unwrap()]);
        std::fs::create_dir_all(source.join("app")).unwrap();
        std::fs::write(source.join("app").join("main.txt"), "shared\n").unwrap();
        std::fs::write(source.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
        init_repository(&source);
        git(&source, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&source, &["push", "-q", "-u", "origin", "main"]);
        Self { _root: root, remote, source, state }
    }

    /// The same, with a recipe this test wrote and this machine has approved.
    fn with_recipe(recipe: &str) -> Self {
        let workspace = Self::new();
        workspace.write_recipe(recipe);
        stdout(&workspace.nodal(&["init", "--force"]));
        workspace
    }

    /// Put a recipe in the project, without approving anything.
    fn write_recipe(&self, recipe: &str) {
        std::fs::write(self.source.join("nodal.toml"), recipe).unwrap();
    }

    /// `nodal` with this workspace's state directory, run in the project.
    fn nodal(&self, args: &[&str]) -> Output {
        let mut command = state::nodal(&self.state);
        command.args(args).current_dir(&self.source);
        command.env("NODAL_SECRETS_FILE", self.state.join("secrets.env"));
        command.env("NODAL_HOOKS_FILE", self.state.join("hooks.toml"));
        command.output().unwrap()
    }

    /// Make a unit, and answer with its home, ready to be committed in.
    fn unit(&self, name: &str) -> PathBuf {
        stdout(&self.nodal(&["new", "--name", name]));
        let home = PathBuf::from(stdout(&self.nodal(&["cd", name])).trim());
        git(&home, &["config", "user.email", "unit@example.invalid"]);
        git(&home, &["config", "user.name", "Test"]);
        home
    }

    /// The registry, opened for reading.
    fn store(&self) -> Store {
        Store::open(self.state.join("registry.db")).unwrap()
    }

    /// The state the registry holds for a unit, which is the answer that matters.
    fn status(&self, slug: &str) -> UnitStatus {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("one project");
        units::find_by_slug(store.conn(), project.id, &slug.parse().unwrap())
            .unwrap()
            .expect("the unit is in the registry")
            .status
    }

    /// Every ref the remote holds, as `git for-each-ref` names them.
    fn remote_refs(&self) -> Vec<String> {
        stdout(
            &Command::new("git")
                .args(["-C", self.remote.to_str().unwrap(), "for-each-ref", "--format=%(refname)"])
                .output()
                .unwrap(),
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }

    /// Squash the unit's branch into `main` in the project's own checkout and push it,
    /// which is what a reviewer pressing the button on a website does.
    fn squash_merge(&self, branch: &str) {
        git(&self.source, &["fetch", "-q", "origin", "+refs/heads/*:refs/remotes/origin/*"]);
        git(&self.source, &["merge", "--squash", "-q", &format!("origin/{branch}")]);
        git(&self.source, &["commit", "-qm", "squashed"]);
        git(&self.source, &["push", "-q", "origin", "main"]);
    }
}

/// Run `git` in a directory and fail the test if it did not work.
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git").args(args).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

/// A repository with one commit and an identity of its own.
fn init_repository(source: &Path) {
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "unit@example.invalid"],
        vec!["config", "user.name", "Test"],
        vec!["add", "-A"],
        vec!["commit", "-qm", "first"],
    ] {
        git(source, &args);
    }
}

/// The standard output of a command that was meant to work.
fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "command failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// Commit a change in a home, so the unit's branch carries work of its own.
fn commit(home: &Path, text: &str) {
    std::fs::write(home.join("app").join("main.txt"), text).unwrap();
    git(home, &["commit", "-qam", "the unit's own work"]);
}

// ---------------------------------------------------------------------------
// done.
// ---------------------------------------------------------------------------

#[test]
fn done_pushes_the_branch_and_the_wip_ref_and_puts_the_unit_up_for_review() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    commit(&home, "fixed\n");
    std::fs::write(home.join("notes.txt"), "not committed yet\n").unwrap();

    let report = stdout(&workspace.nodal(&["done", "worker-import"]));

    let refs = workspace.remote_refs();
    assert!(
        refs.contains(&String::from("refs/heads/nodal/worker-import")),
        "the branch is on the remote: {refs:?}"
    );
    let wip = refs.iter().find(|name| name.ends_with("/wip")).expect("the wip ref went too");
    assert!(wip.starts_with("refs/nodal/"), "and it is in nodal's own namespace: {wip}");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Review);
    assert!(report.contains("one `git push`"), "the report says what left the machine: {report}");
    assert!(report.contains("nodal opens none"), "and what it did not do: {report}");

    // The uncommitted file is on the remote in the snapshot and not on the branch.
    let carried = git(&workspace.remote, &["ls-tree", "--name-only", wip.as_str()]);
    assert!(carried.contains("notes.txt"), "the snapshot carries the uncommitted work: {carried}");
    let branch =
        git(&workspace.remote, &["ls-tree", "--name-only", "refs/heads/nodal/worker-import"]);
    assert!(!branch.contains("notes.txt"), "and the branch does not: {branch}");
}

#[test]
fn done_prints_the_compare_url_of_the_host_the_remote_names() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    commit(&home, "fixed\n");
    // The remote is called by its GitHub name and reached next door. Everything that
    // reads the remote sees github.com; `pushInsteadOf` sends the push itself to the
    // bare repository, so nothing here touches a network.
    git(&home, &["remote", "set-url", "origin", GITHUB]);
    git(
        &home,
        &["config", &format!("url.{}.pushInsteadOf", workspace.remote.to_str().unwrap()), GITHUB],
    );

    let report = stdout(&workspace.nodal(&["done", "worker-import"]));

    assert!(
        report.contains("https://github.com/team/project/compare/nodal/worker-import?expand=1"),
        "the compare page for this branch on this host: {report}"
    );
    assert!(
        workspace.remote_refs().contains(&String::from("refs/heads/nodal/worker-import")),
        "and the branch really went"
    );
}

#[test]
fn done_on_a_second_run_pushes_again_and_leaves_the_unit_in_review() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    commit(&home, "fixed\n");
    stdout(&workspace.nodal(&["done", "worker-import"]));
    std::fs::write(home.join("notes.txt"), "more, still uncommitted\n").unwrap();

    stdout(&workspace.nodal(&["done", "worker-import"]));

    assert_eq!(workspace.status("worker-import"), UnitStatus::Review);
    let wip = workspace
        .remote_refs()
        .into_iter()
        .find(|name| name.ends_with("/wip"))
        .expect("the wip ref is there");
    let carried = git(&workspace.remote, &["ls-tree", "--name-only", &wip]);
    assert!(carried.contains("notes.txt"), "the second snapshot replaced the first: {carried}");
}

#[test]
fn a_home_whose_repository_names_no_remote_is_told_so_rather_than_guessed_at() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    git(&home, &["remote", "remove", "origin"]);

    let refused = workspace.nodal(&["done", "worker-import"]);

    assert!(!refused.status.success(), "there is nowhere to push");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert!(told.contains("names no remote"), "{told}");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Open, "and nothing was recorded");
}

/// Nodal opens no pull request, and there is no code in it that could.
///
/// The other tests assert that `done` did not open one. This asserts the stronger and
/// more useful thing: no path exists. A host's API is reached over a name or through
/// somebody's command-line client, so both are looked for over the whole of both
/// crates. A future change that adds one fails here and has to argue with the decision
/// rather than with a test that happened not to notice.
#[test]
fn no_code_path_in_nodal_can_open_a_pull_request() {
    let forbidden = [
        "api.github.com",
        "api.gitlab.com",
        "api.bitbucket.org",
        "/pulls",
        "merge_requests",
        "pull-request",
        "Command::new(\"gh\")",
        "Command::new(\"glab\")",
        "Command::new(\"hub\")",
    ];
    let mut checked = 0;
    for file in sources() {
        let text = std::fs::read_to_string(&file).unwrap();
        checked += 1;
        for token in forbidden {
            assert!(
                !text.contains(token),
                "{}: nodal opens no pull request, and {token:?} is how one would be opened",
                file.display()
            );
        }
    }
    assert!(checked > 50, "the scan found only {checked} files, so it proved nothing");
}

/// Every Rust source file both crates ship: the product, not the tests that read it.
fn sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
    let mut found = Vec::new();
    for crate_name in ["nodal-core", "nodal-cli"] {
        walk(&crates.join(crate_name).join("src"), &mut found);
    }
    found
}

/// Collect every `.rs` file under a directory.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Merged detection.
// ---------------------------------------------------------------------------

#[test]
fn a_squash_merged_unit_flips_to_merged_once_its_branch_is_on_the_remote() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    commit(&home, "fixed\n");
    stdout(&workspace.nodal(&["done", "worker-import"]));
    assert_eq!(workspace.status("worker-import"), UnitStatus::Review);

    workspace.squash_merge("nodal/worker-import");
    assert_eq!(
        workspace.status("worker-import"),
        UnitStatus::Review,
        "a home that has not heard about the merge yet says nothing about it"
    );

    // The home hears about it the way it hears about anything on the remote: a fetch,
    // run by the person's own git. The list does not fetch; it reads.
    git(&home, &["fetch", "-q", "origin"]);
    let listed = stdout(&workspace.nodal(&["ls"]));

    assert!(listed.contains("merged"), "the list says so: {listed}");
    assert!(listed.contains("done (absorbed)"), "for the reason a squash leaves: {listed}");
    assert_eq!(
        workspace.status("worker-import"),
        UnitStatus::Merged,
        "and it is recorded, not just displayed"
    );
}

/// Integration alone does not make a unit merged; the work has to be somewhere else too.
///
/// The base here really does carry the change — somebody squashed it in — but the
/// unit's own commit is on no remote this home knows. That is the state a person is in
/// after rebasing the base under an unpushed branch, and calling it merged would be
/// calling work that exists on one disk finished.
#[test]
fn work_that_is_on_no_remote_is_not_merged_however_integrated_it_reads() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");
    commit(&home, "fixed\n");
    stdout(&workspace.nodal(&["done", "worker-import"]));
    workspace.squash_merge("nodal/worker-import");
    git(&home, &["fetch", "-q", "origin"]);
    // Take the second signal away: the branch's own commit is now on no remote-tracking
    // ref, exactly as it would be had it never been pushed.
    git(&home, &["update-ref", "-d", "refs/remotes/origin/nodal/worker-import"]);

    let listed = stdout(&workspace.nodal(&["ls"]));

    assert!(listed.contains("done (absorbed)"), "the base carries the change: {listed}");
    assert_eq!(
        workspace.status("worker-import"),
        UnitStatus::Review,
        "and the unit is still where done left it"
    );
}

// ---------------------------------------------------------------------------
// Retention, and the check that still applies.
// ---------------------------------------------------------------------------

#[test]
fn gc_reclaims_a_merged_unit_once_its_retention_has_run_out_and_not_before() {
    let workspace = Workspace::with_recipe("[reclaim]\ntrash_retention = 14\n");
    let home = merged_unit(&workspace, "worker-import");

    let held = stdout(&workspace.nodal(&["gc"]));
    assert!(held.contains("0 units reclaimed"), "a merged unit keeps its home: {held}");
    assert!(home.is_dir(), "and the home is where it was");

    workspace.write_recipe("[reclaim]\ntrash_retention = 0\n");
    stdout(&workspace.nodal(&["init", "--force"]));
    let swept = stdout(&workspace.nodal(&["gc"]));

    assert!(swept.contains("1 unit reclaimed"), "{swept}");
    assert!(swept.contains("worker-import"), "the sweep names it: {swept}");
    assert!(!home.exists(), "and the home has gone from where it was");
}

#[test]
fn gc_refuses_a_merged_unit_somebody_has_put_new_work_in() {
    let workspace = Workspace::with_recipe("[reclaim]\ntrash_retention = 0\n");
    let home = merged_unit(&workspace, "worker-import");
    std::fs::write(home.join("later.txt"), "written after the merge\n").unwrap();

    let swept = stdout(&workspace.nodal(&["gc"]));

    assert!(swept.contains("0 units reclaimed"), "{swept}");
    assert!(swept.contains("worker-import"), "the refusal names the unit: {swept}");
    assert!(swept.contains("later.txt"), "and what it found, not a policy: {swept}");
    assert!(home.is_dir(), "the home is untouched");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Merged, "and so is the state");
}

/// A unit whose work has been squash-merged and recorded as merged.
fn merged_unit(workspace: &Workspace, name: &str) -> PathBuf {
    let home = workspace.unit(name);
    commit(&home, "fixed\n");
    stdout(&workspace.nodal(&["done", name]));
    workspace.squash_merge(&format!("nodal/{name}"));
    git(&home, &["fetch", "-q", "origin"]);
    stdout(&workspace.nodal(&["ls"]));
    assert_eq!(workspace.status(name), UnitStatus::Merged);
    home
}

// ---------------------------------------------------------------------------
// Idle detection.
// ---------------------------------------------------------------------------

#[test]
fn gc_idle_reports_a_quiet_live_unit_and_leaves_everything_of_it_alone() {
    let workspace = Workspace::new();
    let home = workspace.unit("worker-import");

    let reported = stdout(&workspace.nodal(&["gc", "--idle", "0"]));

    assert!(reported.contains("1 live unit past the threshold"), "{reported}");
    assert!(reported.contains("reported only, nothing was stopped"), "{reported}");
    assert!(reported.contains("worker-import"), "the report names it: {reported}");
    assert!(home.join("app").is_dir(), "and the home is exactly where it was");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Open, "and so is its state");
}

#[test]
fn a_unit_inside_the_threshold_is_not_reported_and_no_threshold_asks_nothing() {
    let workspace = Workspace::new();
    workspace.unit("worker-import");

    let inside = stdout(&workspace.nodal(&["gc", "--idle", "7"]));
    assert!(inside.contains("0 live units past the threshold"), "{inside}");

    let unasked = stdout(&workspace.nodal(&["gc"]));
    assert!(
        !unasked.contains("idle"),
        "a sweep nobody asked about idleness says nothing: {unasked}"
    );
}
