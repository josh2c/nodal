//! Acceptance for the verdict: `nodal` in a repository Nodal has never seen.
//!
//! The fixture is the machine the command was written for. One checkout, one origin,
//! and six worktrees of it, each one a shape a person actually has:
//!
//! | worktree | what it is |
//! |---|---|
//! | `done-a`, `done-b` | finished and clean: the base carries them and they hold nothing |
//! | `unpushed-work` | a commit that exists on no remote |
//! | `dirty-tree` | a changed file and an untracked one, neither in any commit |
//! | `behind-base` | pushed, and the base has moved three commits under it |
//! | `oauth-login` | a session record says what it was made for |
//!
//! The claims:
//!
//! 1. The table prints with no `nodal init`, no registry and no file written.
//! 2. Every column carries the fact its worktree was built to have.
//! 3. The order puts the two rows holding work nowhere else at the top and the
//!    finished ones at the bottom.
//! 4. The closing line counts the three that are done and empty, and says nothing was
//!    removed.
//! 5. `--json` carries every row with the same fields.
//! 6. The cost is measured, at six worktrees and at fifty, and printed on every run.
//!
//! The word `worktree` heads the first column and `unit` appears nowhere in the table,
//! because not one of these rows is a home Nodal made.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use nodal_safety::{git, json, stdout};

/// How many worktrees the cost is measured at, beyond the six of the acceptance
/// fixture. Twelve is the size a real machine was measured at; fifty is the size that
/// says whether the cost is linear.
const MEASURED_AT: [usize; 2] = [12, 50];

/// How many runs each reported median is taken over, after one warm-up run.
const RUNS: usize = 5;

/// A checkout with an origin, and the worktrees a test plants in it.
struct Fixture {
    /// The temporary root, kept so it outlives the test.
    directory: tempfile::TempDir,
    /// The checkout the command is run in.
    checkout: PathBuf,
    /// Where the session records live, which an intent is recovered from.
    sessions: PathBuf,
}

impl Fixture {
    /// A checkout of one commit, with a remote that has it.
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let origin = root.join("origin.git");
        let checkout = root.join("checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        git(&root, &["init", "--quiet", "--bare", origin.to_str().unwrap()]);
        git(&checkout, &["init", "--quiet", "--initial-branch", "main"]);
        git(&checkout, &["config", "--local", "user.email", "acceptance@nodal.invalid"]);
        git(&checkout, &["config", "--local", "user.name", "Nodal acceptance"]);
        git(&checkout, &["remote", "add", "origin", origin.to_str().unwrap()]);
        std::fs::write(checkout.join("file"), "one\n").unwrap();
        git(&checkout, &["add", "--all"]);
        git(&checkout, &["commit", "--quiet", "--message", "one"]);
        git(&checkout, &["push", "--quiet", "--set-upstream", "origin", "main"]);
        Self { directory, checkout, sessions: root.join("claude") }
    }

    /// The machine root, which every worktree is planted beside the checkout in.
    fn root(&self) -> PathBuf {
        self.directory.path().to_path_buf()
    }

    /// A worktree of the checkout on a branch of its own, at `main`.
    fn worktree(&self, name: &str) -> PathBuf {
        let path = self.root().join(name);
        git(&self.checkout, &["worktree", "add", "--quiet", "-b", name, path.to_str().unwrap()]);
        path
    }

    /// The six worktrees of the acceptance fixture, each built to have one fact.
    fn six(&self) -> &Self {
        for finished in ["done-a", "done-b"] {
            let path = self.worktree(finished);
            git(&path, &["push", "--quiet", "--set-upstream", "origin", finished]);
        }

        let only_here = self.worktree("unpushed-work");
        std::fs::write(only_here.join("new"), "work no remote has\n").unwrap();
        git(&only_here, &["add", "--all"]);
        git(&only_here, &["commit", "--quiet", "--message", "work that is only here"]);

        let dirty = self.worktree("dirty-tree");
        git(&dirty, &["push", "--quiet", "--set-upstream", "origin", "dirty-tree"]);
        std::fs::write(dirty.join("file"), "one\nedited\n").unwrap();
        std::fs::write(dirty.join("untracked"), "not in any commit\n").unwrap();

        let behind = self.worktree("behind-base");
        std::fs::write(behind.join("theirs"), "their change\n").unwrap();
        git(&behind, &["add", "--all"]);
        git(&behind, &["commit", "--quiet", "--message", "their change"]);
        git(&behind, &["push", "--quiet", "--set-upstream", "origin", "behind-base"]);

        // The base moves under every worktree planted so far. This is the state a
        // person's machine is in after a fortnight, and it is what BEHIND is for.
        for step in 1..=3 {
            std::fs::write(self.checkout.join("file"), format!("one\nmain {step}\n")).unwrap();
            git(&self.checkout, &["add", "--all"]);
            git(&self.checkout, &["commit", "--quiet", "--message", &format!("main {step}")]);
        }
        git(&self.checkout, &["push", "--quiet", "origin", "main"]);

        let stated = self.worktree("oauth-login");
        self.session(&stated, "Add oauth login to the account page and keep the old form working.");
        self
    }

    /// A session record for `worktree`, of the shape Claude Code writes.
    ///
    /// The path is resolved first, and that is not a detail. Git prints the resolved
    /// path of every worktree it names, and a running process is given the resolved
    /// name of the directory it is in, so the directory Claude Code encodes is the
    /// resolved one. A record filed under an unresolved name is a record nothing finds
    /// on a host whose temporary directory is a symbolic link, which is what macOS
    /// gives every test for free and what `ci/acceptance-list.sh` makes on Linux.
    fn session(&self, worktree: &Path, prompt: &str) {
        let worktree = &std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
        let encoded: String = worktree
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        let directory = self.sessions.join("projects").join(encoded);
        std::fs::create_dir_all(&directory).unwrap();
        let record = serde_json::json!({
            "type": "user",
            "isSidechain": false,
            "isMeta": false,
            "cwd": worktree,
            "timestamp": "2026-08-01T10:00:00Z",
            "message": {"content": prompt},
        });
        std::fs::write(directory.join("session.jsonl"), format!("{record}\n")).unwrap();
    }

    /// `count` worktrees of one shape, for measuring what a row costs.
    fn many(&self, count: usize) -> &Self {
        for index in 0..count {
            self.worktree(&format!("bulk-{index:03}"));
        }
        self
    }

    /// The command, with a state directory that is not there and no session records
    /// unless the fixture wrote some.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(binary());
        command
            .args(args)
            .current_dir(&self.checkout)
            .env(nodal_core::workspace::home::DIRECTORY_VAR, self.root().join("state"))
            .env("CLAUDE_CONFIG_DIR", &self.sessions)
            .env_remove("NODAL_CD_FILE");
        command
    }

    /// The command, run.
    fn nodal(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("the binary runs")
    }

    /// Whether Nodal made its state directory to answer.
    fn made_state(&self) -> bool {
        self.root().join("state").exists()
    }
}

/// The binary under test, beside the test executable.
fn binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("the test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    let binary = path.join(format!("nodal{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.exists(),
        "no `nodal` beside {}: build it with `cargo build -p nodal-cli`",
        path.display()
    );
    binary
}

/// The line of the table that names one worktree.
fn row<'a>(report: &'a str, name: &str) -> &'a str {
    report
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no row for {name} in:\n{report}"))
}

/// The rows of the table, in the order they print, by the name in the first column.
fn order(report: &str) -> Vec<&str> {
    report
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("WORKTREE"))
        .skip(1)
        .map(str::trim_start)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .collect()
}

#[test]
fn six_worktrees_print_one_table_with_no_init_and_nothing_written() {
    let fixture = Fixture::new();
    fixture.six();

    let answered = fixture.nodal(&[]);
    assert!(answered.status.success(), "the verdict failed");
    let report = stdout(&answered);

    assert!(report.contains("WORKTREE"), "the word worktree does not head the column:\n{report}");
    for name in ["done-a", "done-b", "unpushed-work", "dirty-tree", "behind-base", "oauth-login"] {
        assert!(report.contains(name), "no row for {name}:\n{report}");
    }
    assert!(
        !report.to_lowercase().contains("unit"),
        "a folder nodal did not make was called a unit:\n{report}"
    );
    assert!(!fixture.made_state(), "the verdict made a state directory:\n{report}");
}

#[test]
fn every_column_carries_the_fact_its_worktree_was_built_to_have() {
    let fixture = Fixture::new();
    fixture.six();
    let report = stdout(&fixture.nodal(&[]));

    let only_here = row(&report, "../unpushed-work");
    assert!(only_here.contains("^1"), "the unpushed commit is not on the row:\n{only_here}");
    assert!(
        only_here.contains("open"),
        "a branch with a commit of its own is not done:\n{only_here}"
    );

    let dirty = row(&report, "../dirty-tree");
    assert!(dirty.contains("*2"), "the changed and untracked paths are not counted:\n{dirty}");

    let behind = row(&report, "../behind-base");
    assert!(
        behind.contains("-3 (main)"),
        "the base has moved three and the row does not say so:\n{behind}"
    );

    for finished in ["../done-a", "../done-b"] {
        let line = row(&report, finished);
        assert!(line.contains("done ("), "a finished worktree is not called done:\n{line}");
        assert!(line.contains('—'), "a worktree holding nothing says it holds something:\n{line}");
    }

    let stated = row(&report, "../oauth-login");
    assert!(
        stated.contains("Add oauth login to the account page"),
        "the recovered intent is not on the row:\n{stated}"
    );
}

#[test]
fn the_worktrees_holding_work_nowhere_else_are_first_and_the_finished_ones_are_last() {
    let fixture = Fixture::new();
    fixture.six();
    let report = stdout(&fixture.nodal(&[]));
    let printed = order(&report);

    let top: Vec<&str> = printed.iter().take(2).copied().collect();
    assert!(
        top.contains(&"../unpushed-work"),
        "a worktree holding an unpushed commit is not at the top: {printed:?}"
    );
    assert!(
        top.contains(&"../dirty-tree"),
        "a worktree holding uncommitted work is not at the top: {printed:?}"
    );

    let bottom: Vec<&str> = printed.iter().rev().take(3).copied().collect();
    for finished in ["../done-a", "../done-b", "../oauth-login"] {
        assert!(bottom.contains(&finished), "{finished} is not at the bottom: {printed:?}");
    }
}

#[test]
fn the_closing_line_counts_what_could_go_and_says_nothing_was_removed() {
    let fixture = Fixture::new();
    fixture.six();
    let report = stdout(&fixture.nodal(&[]));
    let closing = report.lines().rev().find(|line| !line.trim().is_empty()).unwrap().trim();

    assert!(
        closing.starts_with("3 worktrees are done and hold nothing unique:"),
        "the closing line does not count the three that could go:\n{closing}"
    );
    assert!(closing.ends_with("nodal removed nothing."), "the promise is not made:\n{closing}");
}

#[test]
fn the_json_answer_carries_every_row_with_the_same_fields() {
    let fixture = Fixture::new();
    fixture.six();
    let answered = fixture.nodal(&["ls", "--json"]);
    let document = json(&answered);

    let rows = document["rows"].as_array().expect("the rows are a list");
    assert_eq!(rows.len(), 6, "the JSON answer holds a different number of rows: {document}");
    assert!(document["project"].is_null(), "a checkout with no row is named as a project");

    for found in rows {
        for field in [
            "kind",
            "name",
            "path",
            "branch",
            "intent",
            "done",
            "unpushed",
            "uncommitted",
            "behind",
            "bytes",
            "made_at",
        ] {
            assert!(found.get(field).is_some(), "the row has no {field}: {found}");
        }
        assert_eq!(found["kind"], "worktree", "a foreign worktree is not marked as one: {found}");
    }

    let stated = rows.iter().find(|found| found["name"] == "../oauth-login").expect("the row");
    assert!(
        stated["intent"].as_str().unwrap().contains("Add oauth login"),
        "the JSON answer left the intent out: {stated}"
    );
    assert!(!fixture.made_state(), "nodal ls --json made a state directory");
}

/// The cost of the answer, at the size a real machine had and at four times it.
///
/// This is reported rather than asserted against a threshold. A number pinned on
/// somebody else's hardware fails on a slower machine and says nothing on a faster one,
/// and the thing worth watching is the shape: fifty worktrees should cost about four
/// times what twelve do, because each row is a fixed number of Git reads and one walk.
#[test]
fn the_cost_of_the_answer_is_measured_and_printed() {
    for count in MEASURED_AT {
        let fixture = Fixture::new();
        fixture.many(count);
        let report = stdout(&fixture.nodal(&[]));
        assert_eq!(order(&report).len(), count, "the table lost a row at {count}");

        let mut runs: Vec<Duration> = Vec::new();
        let _ = fixture.nodal(&[]);
        for _ in 0..RUNS {
            let started = Instant::now();
            let answered = fixture.nodal(&[]);
            runs.push(started.elapsed());
            assert!(answered.status.success(), "the verdict failed at {count} worktrees");
        }
        runs.sort_unstable();
        let median = runs[runs.len() / 2];
        println!(
            "nodal (bare) over {count} worktrees: median {:?} of {RUNS} runs, per worktree {:?}",
            median,
            median / u32::try_from(count).unwrap()
        );
    }
}
