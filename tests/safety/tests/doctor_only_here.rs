//! Doctor's only-here survey: one reading of the checkout, and a verdict for each home.
//!
//! The section answers one question — does a unit home hold work that exists nowhere
//! else — and it answers it for every open home of the project. The evaluator is the
//! reclaim's own (`nodal_core::lifecycle::assess`), so `tests/reclaim_check.rs` holds
//! what a verdict means. This file holds the two things that are true of the survey
//! rather than of one verdict.
//!
//! **One reading, and a verdict each.** What the evaluator asks of the checkout is the
//! same question whichever home it is reading: the checkout's git directory, its refs,
//! the name of its `origin`, and which of its own tips its object store holds. That
//! reading is taken once for the project. Two homes judged against one reading must
//! still get their own answers, and the first property puts a home the remote proves
//! beside a home nothing proves and reads the rows.
//!
//! **Once per survey, and not once per home.** The second property counts the `git`
//! processes doctor starts, with the shim `ci/measure.sh` uses: a `git` first on the
//! search path that writes its arguments to a file and then runs the real one. The
//! claim is a number, so it is measured rather than asserted from the shape of the
//! code. A project with three homes must start no more processes against its checkout
//! than a project with one, and a project whose homes are all gone must start none of
//! them at all.
//!
//! `ci/measure.sh` gates the same number and can be told to leave its git-process rows
//! out with `NODAL_MEASURE_SKIP_GIT`. That variable reaches the script and never this
//! file: the property here is a test, it runs on every `cargo test`, and it is set in
//! the environment below to say so out loud.
//!
//! | property | test |
//! |---|---|
//! | one reading, a verdict each | `two_homes_share_one_reading_and_keep_their_own_verdicts` |
//! | once per survey, not once per home | `the_survey_reads_the_checkout_once_however_many_homes_it_has` |

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::{Machine, git, stdout};
use serde_json::Value;

/// The unit whose work the remote is made to prove.
const PROVED: &str = "worker-import";

/// The unit whose work exists in its home and nowhere else.
const ONLY: &str = "report-export";

/// A third unit, so that "once" is measured against more than two homes.
const THIRD: &str = "api-gateway";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const WORK: &str = "only-here.txt";

/// The branch a home pushes its work to, as a review branch on the remote.
const TOPIC: &str = "topic";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// What doctor calls a row about a home that holds work nothing else has.
const UNIQUE_WORK: &str = "unique_work";

/// Commit `WORK` in `home`, saying `what`, and answer with the commit.
///
/// The words are the caller's because two homes of one project are clones of one base
/// at one commit. Two commits made in the same second, of the same content, under the
/// fixture's one identity, with the same message, are one commit object — and a test
/// that gave two homes the same work would be asserting about one commit in two places
/// rather than about two homes.
fn commit(home: &Path, what: &str) -> String {
    std::fs::write(home.join(WORK), format!("{what}\n")).unwrap();
    git(home, &["add", "--all"]);
    git(home, &["commit", "--quiet", "--message", what]);
    git(home, &["rev-parse", "HEAD"])
}

/// The only-here rows of `nodal doctor`, by the unit each one is about.
fn unique_rows(machine: &Machine) -> Vec<String> {
    let printed = stdout(&machine.nodal(&["doctor", "--json"]));
    let report: Value = serde_json::from_str(&printed).expect("doctor --json is one document");
    report["here"]
        .as_array()
        .expect("the section is a list")
        .iter()
        .filter(|row| row["kind"] == UNIQUE_WORK)
        .map(|row| row["what"].as_str().expect("a row names its unit").to_owned())
        .collect()
}

/// Two homes of one project, read against one reading of one checkout.
///
/// One of them pushed its work and the person then fetched, so the checkout is the
/// newest reading of the remote and it reaches that commit. The other never pushed, so
/// the same reading of the same checkout reaches nothing of its work. One reading, two
/// questions, two answers — and the row is about the second home alone.
#[test]
fn two_homes_share_one_reading_and_keep_their_own_verdicts() {
    let machine = Machine::with_remote().with_env(ONLY_LOCAL);
    let proved = machine.unit(PROVED);
    let only = machine.unit(ONLY);

    commit(&proved, "work the remote is given");
    git(&proved, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{TOPIC}")]);
    let stays = commit(&only, "work that stays in its home");
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);

    let rows = unique_rows(&machine);
    assert_eq!(rows, vec![ONLY.to_owned()], "one reading gave two homes one answer");
    assert_eq!(git(&only, &["cat-file", "-t", &stays]), "commit", "the report took something");
    assert!(proved.is_dir() && only.is_dir(), "the report moved a home");
}

/// The `git` the shim runs, found before the shim is put in front of it.
fn real_git() -> PathBuf {
    let path = std::env::var_os("PATH").expect("a search path");
    std::env::split_paths(&path)
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("git is on the search path")
}

/// Write the counting `git` of `ci/measure.sh` into `directory`, logging to `log`.
fn counting_git(directory: &Path, log: &Path) {
    let shim = directory.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\nexec {} \"$@\"\n",
            log.display(),
            real_git().display()
        ),
    )
    .unwrap();
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(&shim, mode).unwrap();
}

/// The lines of the shim's log, or none when nothing ran.
fn logged(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log).unwrap_or_default().lines().map(ToOwned::to_owned).collect()
}

/// Whether one logged invocation ran in `repo`.
///
/// Nodal names the directory of every invocation with `-C`, which is the first argument
/// of every line. A path is compared as the filesystem spells it, so the name a test
/// built and the name Nodal resolved are one directory.
fn ran_in(line: &str, repo: &Path) -> bool {
    line.strip_prefix("-C ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|directory| std::fs::canonicalize(directory).ok())
        .is_some_and(|read| read == repo)
}

/// How many `git` processes the log holds that were run in `repo`.
fn against(log: &Path, repo: &Path) -> usize {
    let wanted = std::fs::canonicalize(repo).unwrap();
    logged(log).iter().filter(|line| ran_in(line, &wanted)).count()
}

/// Run one survey and answer with every `git` process it started in the checkout.
///
/// Every one of them, and no rule about which. A count that named the reading it was
/// about would only see the readings whoever wrote it thought of, which is how a second
/// per-home reading of the checkout sat under this test unnoticed.
fn survey(machine: &Machine, log: &Path) -> usize {
    drop(std::fs::remove_file(log));
    let read = machine.nodal(&["doctor"]);
    assert!(read.status.success(), "doctor failed: {}", nodal_safety::stderr(&read));
    against(log, &machine.source)
}

/// What a home costs the checkout, in each of the two relations a project can be in.
///
/// A project cloned from a remote costs nothing per home: everything the survey asks of
/// the checkout is a fact about the checkout, and the survey asks it once.
///
/// A project with no remote of its own is dearer, and not because of the survey. Its
/// homes' `origin` is the checkout, so the checkout is read as the remote itself, and
/// the proof then asks it two questions about each home — which of that home's remote
/// tips it holds, and which of them its branches no longer reach
/// (`nodal_core::doctor::unique`). Those are questions about the home. Nothing can
/// answer them once for a survey, and this test holds them at two so that a fact about
/// the checkout can never hide among them again.
const PER_HOME: [(&str, usize); 2] = [("a clone of a remote", 0), ("no remote of its own", 2)];

/// The survey reads the checkout once, whatever the project's homes cost.
///
/// Three surveys of one machine, in each relation. The homes are taken away between
/// them by removing the directories, which is the state a person leaves by deleting a
/// home by hand and which the survey already skips.
///
/// Two numbers carry the claim, and both are over every `git` process the command
/// started in the checkout rather than over a reading named in advance. Two more homes
/// must cost what [`PER_HOME`] says and not a process more, because what the survey
/// asks of the checkout it asks once. And a project with no home left to read must cost
/// less than one with a home, because a reading nothing needs is a reading nothing
/// takes.
#[test]
fn the_survey_reads_the_checkout_once_however_many_homes_it_has() {
    for (relation, per_home) in PER_HOME {
        let machine = if per_home == 0 { Machine::with_remote() } else { Machine::new() };
        let machine = machine.with_env(ONLY_LOCAL);
        let homes = [machine.unit(PROVED), machine.unit(ONLY), machine.unit(THIRD)];

        let counted = tempfile::tempdir().unwrap();
        let log = counted.path().join("git.log");
        counting_git(counted.path(), &log);
        let search = search_through(counted.path());
        let machine = machine
            .with_env(("PATH", &search))
            // The measurement script honours this. A test is not the script, and this
            // property runs whatever it says.
            .with_env(("NODAL_MEASURE_SKIP_GIT", "1"));

        let three = survey(&machine, &log);
        for home in &homes {
            assert!(against(&log, home) > 0, "{relation}: the survey did not read a home");
        }

        std::fs::remove_dir_all(&homes[1]).unwrap();
        std::fs::remove_dir_all(&homes[2]).unwrap();
        let one = survey(&machine, &log);
        assert_eq!(three - one, per_home * 2, "{relation}: two more homes cost the checkout");

        std::fs::remove_dir_all(&homes[0]).unwrap();
        let none = survey(&machine, &log);
        assert!(none < one, "{relation}: a project with no home to read still read its checkout");
    }
}

/// This machine's own search path, with `directory` in front of it.
fn search_through(directory: &Path) -> String {
    let inherited = std::env::var_os("PATH").unwrap();
    let joined = std::env::join_paths(
        std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&inherited)),
    )
    .unwrap();
    joined.into_string().unwrap()
}
