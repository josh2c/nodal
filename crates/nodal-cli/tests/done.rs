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

use nodal_core::model::UnitStatus;
use nodal_core::store::{projects, units};
use nodal_safety::git::{git, git_ok, identity};
use nodal_safety::text::stdout;
use nodal_safety::{InState as _, Workspace};

/// The address the compare-page test pretends its remote lives at.
const GITHUB: &str = "https://github.com/team/project.git";

/// This suite's project: a one-commit repository, pushed to a bare remote beside it.
///
/// `nodal done` is about what a person sees after a review, so the project has the
/// remote a review happens on.
fn workspace() -> Workspace {
    let workspace = Workspace::new(state::BINARY);
    let remote = workspace.root().join("remote.git");
    git_ok(workspace.root(), &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git_ok(&workspace.source, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git_ok(&workspace.source, &["push", "-q", "-u", "origin", "main"]);
    workspace
}

/// The same, with a recipe this test wrote and this machine has approved.
fn workspace_with_recipe(recipe: &str) -> Workspace {
    let workspace = workspace();
    workspace.approve_recipe(recipe);
    workspace
}

/// The readings this suite needs beyond the ones the shared fixture has.
trait Finishing {
    /// The bare repository the project pushes to.
    fn remote(&self) -> PathBuf;

    /// Make a unit, and answer with its home, ready to be committed in.
    fn unit_home(&self, name: &str) -> PathBuf;

    /// The state the registry holds for a unit, which is the answer that matters.
    fn status(&self, slug: &str) -> UnitStatus;

    /// Every ref the remote holds, as `git for-each-ref` names them.
    fn remote_refs(&self) -> Vec<String>;

    /// Squash the unit's branch into `main` in the project's own checkout and push it,
    /// which is what a reviewer pressing the button on a website does.
    fn squash_merge(&self, branch: &str);
}

impl Finishing for Workspace {
    fn remote(&self) -> PathBuf {
        self.root().join("remote.git")
    }

    fn unit_home(&self, name: &str) -> PathBuf {
        drop(stdout(&self.nodal(&["new", "--name", name])));
        let home = PathBuf::from(stdout(&self.nodal(&["cd", name])).trim());
        identity(&home);
        home
    }

    fn status(&self, slug: &str) -> UnitStatus {
        let store = self.store();
        let project = projects::list(store.conn()).unwrap().pop().expect("one project");
        units::find_by_slug(store.conn(), project.id, &slug.parse().unwrap())
            .unwrap()
            .expect("the unit is in the registry")
            .status
    }

    fn remote_refs(&self) -> Vec<String> {
        git(self.remote(), &["for-each-ref", "--format=%(refname)"])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn squash_merge(&self, branch: &str) {
        git_ok(&self.source, &["fetch", "-q", "origin", "+refs/heads/*:refs/remotes/origin/*"]);
        git_ok(&self.source, &["merge", "--squash", "-q", &format!("origin/{branch}")]);
        git_ok(&self.source, &["commit", "-qm", "squashed"]);
        git_ok(&self.source, &["push", "-q", "origin", "main"]);
    }
}

/// Commit a change in a home, so the unit's branch carries work of its own.
fn commit(home: &Path, text: &str) {
    std::fs::write(home.join("app").join("main.txt"), text).unwrap();
    identity(home);
    git_ok(home, &["commit", "-qam", "the unit's own work"]);
}

// ---------------------------------------------------------------------------
// done.
// ---------------------------------------------------------------------------

#[test]
fn done_pushes_the_branch_and_the_wip_ref_and_puts_the_unit_up_for_review() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
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
    let carried = git(workspace.remote(), &["ls-tree", "--name-only", wip.as_str()]);
    assert!(carried.contains("notes.txt"), "the snapshot carries the uncommitted work: {carried}");
    let branch =
        git(workspace.remote(), &["ls-tree", "--name-only", "refs/heads/nodal/worker-import"]);
    assert!(!branch.contains("notes.txt"), "and the branch does not: {branch}");
}

#[test]
fn done_prints_the_compare_url_of_the_host_the_remote_names() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    commit(&home, "fixed\n");
    // The remote is called by its GitHub name and reached next door. Everything that
    // reads the remote sees github.com; `pushInsteadOf` sends the push itself to the
    // bare repository, so nothing here touches a network.
    drop(git(&home, &["remote", "set-url", "origin", GITHUB]));
    git_ok(
        &home,
        &["config", &format!("url.{}.pushInsteadOf", workspace.remote().to_str().unwrap()), GITHUB],
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
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    commit(&home, "fixed\n");
    drop(stdout(&workspace.nodal(&["done", "worker-import"])));
    std::fs::write(home.join("notes.txt"), "more, still uncommitted\n").unwrap();

    drop(stdout(&workspace.nodal(&["done", "worker-import"])));

    assert_eq!(workspace.status("worker-import"), UnitStatus::Review);
    let wip = workspace
        .remote_refs()
        .into_iter()
        .find(|name| name.ends_with("/wip"))
        .expect("the wip ref is there");
    let carried = git(workspace.remote(), &["ls-tree", "--name-only", &wip]);
    assert!(carried.contains("notes.txt"), "the second snapshot replaced the first: {carried}");
}

#[test]
fn a_home_whose_repository_names_no_remote_is_told_so_rather_than_guessed_at() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    drop(git(&home, &["remote", "remove", "origin"]));

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
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    commit(&home, "fixed\n");
    drop(stdout(&workspace.nodal(&["done", "worker-import"])));
    assert_eq!(workspace.status("worker-import"), UnitStatus::Review);

    workspace.squash_merge("nodal/worker-import");
    assert_eq!(
        workspace.status("worker-import"),
        UnitStatus::Review,
        "a home that has not heard about the merge yet says nothing about it"
    );

    // The home hears about it the way it hears about anything on the remote: a fetch,
    // run by the person's own git. The list does not fetch; it reads.
    drop(git(&home, &["fetch", "-q", "origin"]));
    let listed = stdout(&workspace.nodal(&["ls"]));

    assert!(listed.contains("merged"), "the list says so: {listed}");
    assert!(listed.contains("done (absorbed)"), "for the reason a squash leaves: {listed}");
    assert_eq!(
        workspace.status("worker-import"),
        UnitStatus::Merged,
        "and it is recorded, not just displayed"
    );
}

/// A unit nobody has begun has landed nothing, whatever the two other signals read.
///
/// This is the shape that was wrong. A unit `nodal new` has just made sits on the base's
/// own commit, so its tip is in the base's history — `integrated (ancestor)` — and the
/// commits it is made of are the project's and are already on the remote, so every
/// remote contains it. Both readings are true and neither is about this unit. Flipping
/// it starts the retention `gc` measures, and one sweep later the home of a unit nobody
/// had opened is gone, on a state that was never true.
///
/// So the assertion is made twice: the state is never written, and the sweep that would
/// have acted on it takes nothing. The recipe keeps trash for no time at all, which is
/// the setting under which the defect removed the home in a single sweep.
#[test]
fn a_brand_new_unit_is_not_merged_and_its_home_is_never_reclaimed() {
    let workspace = workspace_with_recipe("[reclaim]\ntrash_retention = 0\n");
    let home = workspace.unit_home("brand-new");

    let listed = stdout(&workspace.nodal(&["ls"]));

    assert!(!listed.contains("merged"), "a unit nobody has begun is not merged: {listed}");
    assert_eq!(workspace.status("brand-new"), UnitStatus::Open, "and nothing was recorded");

    let swept = stdout(&workspace.nodal(&["gc"]));

    assert!(swept.contains("0 units reclaimed"), "so the sweep has nothing to give back: {swept}");
    assert!(home.join("app").is_dir(), "and the home is exactly where it was");
    assert_eq!(workspace.status("brand-new"), UnitStatus::Open);
}

/// A unit whose commits were all squash-absorbed still counts as merged.
///
/// The case the predicate had to keep. A squash leaves the *changes* on the base and the
/// *commits* only on the branch, so the branch is still ahead of the base and no commit
/// of it is in the base's history — which is asserted here rather than assumed, because
/// it is the exact property that separates this from the unit above.
#[test]
fn a_unit_whose_commits_were_squash_absorbed_still_flips_to_merged() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    commit(&home, "fixed\n");
    let tip = git(&home, &["rev-parse", "HEAD"]).trim().to_owned();
    drop(stdout(&workspace.nodal(&["done", "worker-import"])));
    workspace.squash_merge("nodal/worker-import");
    drop(git(&home, &["fetch", "-q", "origin"]));

    let carried = git(&workspace.source, &["branch", "--contains", &tip, "--format=%(refname)"]);
    assert!(
        !carried.contains("refs/heads/main"),
        "no commit of the unit is on the base, which is what a squash leaves: {carried:?}"
    );

    let listed = stdout(&workspace.nodal(&["ls"]));

    assert!(listed.contains("done (absorbed)"), "{listed}");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Merged);
}

/// Integration alone does not make a unit merged; the work has to be somewhere else too.
///
/// The base here really does carry the change — somebody squashed it in — but the
/// unit's own commit is on no remote this home knows. That is the state a person is in
/// after rebasing the base under an unpushed branch, and calling it merged would be
/// calling work that exists on one disk finished.
#[test]
fn work_that_is_on_no_remote_is_not_merged_however_integrated_it_reads() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");
    commit(&home, "fixed\n");
    drop(stdout(&workspace.nodal(&["done", "worker-import"])));
    workspace.squash_merge("nodal/worker-import");
    drop(git(&home, &["fetch", "-q", "origin"]));
    // Take the second signal away: the branch's own commit is now on no remote-tracking
    // ref, exactly as it would be had it never been pushed.
    drop(git(&home, &["update-ref", "-d", "refs/remotes/origin/nodal/worker-import"]));

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
    let workspace = workspace_with_recipe("[reclaim]\ntrash_retention = 14\n");
    let home = merged_unit(&workspace, "worker-import");

    let held = stdout(&workspace.nodal(&["gc"]));
    assert!(held.contains("0 units reclaimed"), "a merged unit keeps its home: {held}");
    assert!(home.is_dir(), "and the home is where it was");

    workspace.write_recipe("[reclaim]\ntrash_retention = 0\n");
    drop(stdout(&workspace.nodal(&["init", "--force"])));
    let swept = stdout(&workspace.nodal(&["gc"]));

    assert!(swept.contains("1 unit reclaimed"), "{swept}");
    assert!(swept.contains("worker-import"), "the sweep names it: {swept}");
    assert!(!home.exists(), "and the home has gone from where it was");
}

#[test]
fn gc_refuses_a_merged_unit_somebody_has_put_new_work_in() {
    let workspace = workspace_with_recipe("[reclaim]\ntrash_retention = 0\n");
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
    drop(stdout(&workspace.nodal(&["done", name])));
    workspace.squash_merge(&format!("nodal/{name}"));
    drop(git(&home, &["fetch", "-q", "origin"]));
    drop(stdout(&workspace.nodal(&["ls"])));
    assert_eq!(workspace.status(name), UnitStatus::Merged);
    home
}

// ---------------------------------------------------------------------------
// Idle detection.
// ---------------------------------------------------------------------------

#[test]
fn gc_idle_reports_a_quiet_live_unit_and_leaves_everything_of_it_alone() {
    let workspace = workspace();
    let home = workspace.unit_home("worker-import");

    let reported = stdout(&workspace.nodal(&["gc", "--idle", "0"]));

    assert!(reported.contains("1 live unit past the threshold"), "{reported}");
    assert!(reported.contains("reported only, nothing was stopped"), "{reported}");
    assert!(reported.contains("worker-import"), "the report names it: {reported}");
    assert!(home.join("app").is_dir(), "and the home is exactly where it was");
    assert_eq!(workspace.status("worker-import"), UnitStatus::Open, "and so is its state");
}

#[test]
fn a_unit_inside_the_threshold_is_not_reported_and_no_threshold_asks_nothing() {
    let workspace = workspace();
    workspace.unit_home("worker-import");

    let inside = stdout(&workspace.nodal(&["gc", "--idle", "7"]));
    assert!(inside.contains("0 live units past the threshold"), "{inside}");

    let unasked = stdout(&workspace.nodal(&["gc"]));
    assert!(
        !unasked.contains("idle"),
        "a sweep nobody asked about idleness says nothing: {unasked}"
    );
}

// ---------------------------------------------------------------------------
// The same reasoning, where it is already right.
// ---------------------------------------------------------------------------

/// Vacuous containment is the correct answer to the question `reclaim` asks.
///
/// The uniqueness check reads the same signal, and reads it for a different question:
/// not "did this unit contribute anything" but "is there work here that exists nowhere
/// else". A unit with no commits of its own has nothing to lose, so being contained by
/// every remote for somebody else's commits is the right answer and not a vacuous one.
///
/// The check is not weakened by it either, which is the half worth proving: the same
/// brand-new home with one untracked file in it is still refused, because uncommitted
/// and untracked paths are read independently of any remote.
#[test]
fn a_unit_with_no_commits_is_safe_to_reclaim_and_one_with_new_files_is_still_refused() {
    let workspace = workspace();
    let dirty = workspace.unit_home("has-a-file");
    std::fs::write(dirty.join("scratch.txt"), "not committed anywhere\n").unwrap();

    let refused = workspace.nodal(&["reclaim", "has-a-file"]);
    assert!(!refused.status.success(), "an untracked file is still work that is only here");
    let told = String::from_utf8_lossy(&refused.stderr);
    assert!(told.contains("untracked files"), "{told}");
    assert!(dirty.is_dir(), "and the home is where it was");

    let empty = workspace.unit_home("nothing-in-it");
    let report = stdout(&workspace.nodal(&["reclaim", "nothing-in-it", "--json"]));
    assert!(report.contains("\"findings\": []"), "nothing is only here: {report}");
    assert!(!empty.exists(), "so the home goes, without a refusal");
}
