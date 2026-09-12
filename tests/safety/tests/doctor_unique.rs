//! `nodal doctor --machine` may not say a clone holds nothing unique without looking.
//!
//! The uniqueness column is the answer to "is it safe to delete this", and the first
//! line of the never list is never delete unique work. For a long time the column was
//! read from `git rev-list HEAD --not --remotes`, which is a question about the clone's
//! own `refs/remotes/`. A clone writes those refs when it fetches or pushes and never
//! corrects them, so a clone that pushed a branch once answered "nothing unpushed" for
//! the rest of its life, after the branch was deleted on the remote and after the branch
//! was rewritten. On the machine this suite was written on, ten clones of one repository
//! held commits no remote had and doctor called every one of them safe.
//!
//! The three properties here are the three ways that went wrong. Each one fails against
//! the old reading and passes against the new one.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::fs::{File, FileTimes};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nodal_safety::{Machine, Snapshot, git, stdout};

/// What a bare repository two clones share is called.
const REMOTE: &str = "remote.git";

/// The branch the work is done on, and then deleted from the remote.
const WORK: &str = "work";

/// What a loose object is set to before it is overwritten. Git writes them read-only.
const WRITABLE: u32 = 0o644;

/// How far ahead a clone's reading of the remote is put, so that "later" is not a race.
const LATER: Duration = Duration::from_secs(600);

/// A directory for this test's clones, beside the fixture project and named as Git and
/// the survey will both name it.
///
/// The suite runs a second time with `TMPDIR` reached through a symbolic link, and the
/// survey resolves every root it is given. A test that asserts on a path has to resolve
/// its own the same way or it compares two spellings of one directory.
fn planted_under(machine: &Machine) -> PathBuf {
    let clones = machine.source.parent().unwrap().join("clones");
    std::fs::create_dir_all(&clones).unwrap();
    std::fs::canonicalize(&clones).unwrap()
}

/// A bare remote with one commit on `main`, under `root`.
fn remote(root: &Path) -> PathBuf {
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git::init(&seed, "main");
    std::fs::write(seed.join("README.md"), "# app\n").unwrap();
    git::commit(&seed, "start");
    let bare = root.join(REMOTE);
    git(root, &["clone", "--quiet", "--bare", seed.to_str().unwrap(), bare.to_str().unwrap()]);
    std::fs::remove_dir_all(&seed).unwrap();
    bare
}

/// Clone `bare` into `root/name`, with an identity to commit under.
fn clone(root: &Path, bare: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    git(root, &["clone", "--quiet", bare.to_str().unwrap(), path.to_str().unwrap()]);
    git::identity(&path);
    path
}

/// Commit on `WORK` in `path` and push the branch, the way a person finishes a task.
fn work_and_push(path: &Path) {
    git(path, &["switch", "--quiet", "--create", WORK]);
    std::fs::write(path.join("work.md"), "the only copy\n").unwrap();
    git::commit(path, "work nobody else has");
    git(path, &["push", "--quiet", "origin", WORK]);
}

/// Delete the branch from the remote, which is what merging a pull request does.
fn delete_from_remote(bare: &Path) {
    git(bare, &["update-ref", "-d", &format!("refs/heads/{WORK}")]);
}

/// Put this clone's reading of the remote ahead of every other clone's.
///
/// Two clones made in the same second can otherwise tie, and the proof asks which clone
/// heard from the remote most recently. The test states the order rather than racing it.
fn heard_last(path: &Path) {
    let when = SystemTime::now() + LATER;
    for name in ["packed-refs", "FETCH_HEAD"] {
        let file = path.join(".git").join(name);
        if let Ok(handle) = File::options().write(true).open(&file) {
            handle.set_times(FileTimes::new().set_modified(when)).unwrap();
        }
    }
}

/// Overwrite the loose object HEAD points at, so Git can read the ref and not the commit.
///
/// A clone's history arrives in a pack, and a commit made afterwards is a file of its
/// own under `.git/objects`. Damaging that file leaves the refs intact and the commit
/// unreadable, which is the shape a half-written repository has.
fn damage_head(path: &Path) {
    let head = git(path, &["rev-parse", "HEAD"]);
    let (directory, file) = head.split_at(2);
    let object = path.join(".git/objects").join(directory).join(file);
    std::fs::set_permissions(&object, std::fs::Permissions::from_mode(WRITABLE)).unwrap();
    std::fs::write(&object, b"this is not a Git object").unwrap();
}

/// The report `nodal doctor --machine` prints for one directory of clones.
fn survey(machine: &Machine, clones: &Path) -> String {
    stdout(&machine.nodal(&["doctor", "--machine", clones.to_str().unwrap()]))
}

/// The same answer as JSON, which is where a tool reads the same facts from.
fn survey_json(machine: &Machine, clones: &Path) -> String {
    stdout(&machine.nodal(&["doctor", "--machine", clones.to_str().unwrap(), "--json"]))
}

/// A second bare repository, so a clone can have a remote that is not its `origin`.
fn spare_remote(root: &Path, name: &str) -> PathBuf {
    let bare = root.join(name);
    git(root, &["init", "--quiet", "--bare", "--initial-branch=main", bare.to_str().unwrap()]);
    bare
}

/// A clone holding a commit no other clone and no remote has is named, with its count.
///
/// `keeper` pushed the branch and the remote dropped it afterwards. `fresh` was cloned
/// after that, so it never had the branch and its reading of the remote is the newest
/// one on this machine. The commit is in `keeper` and nowhere else.
#[test]
fn a_commit_only_one_clone_holds_is_reported_as_unique_work() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);

    let keeper = clone(&clones, &bare, "keeper");
    work_and_push(&keeper);
    delete_from_remote(&bare);
    let fresh = clone(&clones, &bare, "fresh");
    heard_last(&fresh);

    let report = survey(&machine, &clones);
    assert!(!report.contains("nothing unique"), "the commit is only in one clone:\n{report}");
    assert!(report.contains("unique work"), "{report}");
    assert!(report.contains(keeper.to_str().unwrap()), "the clone is not named:\n{report}");
    assert!(report.contains("1 commit"), "the count is not there:\n{report}");
    assert!(!report.contains(fresh.to_str().unwrap()), "the empty clone holds nothing:\n{report}");
}

/// A clone whose branch the remote no longer has is never reported as pushed.
///
/// Here the commit does survive the folder: `second` cloned the branch before it was
/// deleted, so deleting `keeper` loses nothing. That makes the folder safe to remove and
/// leaves the work on no remote, and the report has to say the second thing as well.
#[test]
fn a_branch_the_remote_dropped_is_not_called_pushed() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);

    let keeper = clone(&clones, &bare, "keeper");
    work_and_push(&keeper);
    clone(&clones, &bare, "second");
    delete_from_remote(&bare);
    let fresh = clone(&clones, &bare, "fresh");
    heard_last(&fresh);

    let report = survey(&machine, &clones);
    assert!(report.contains("on no remote"), "the branch is not on any remote:\n{report}");
    assert!(report.contains(keeper.to_str().unwrap()), "the clone is not named:\n{report}");
    assert!(report.contains("1 commit"), "the count is not there:\n{report}");
    assert!(!report.contains(fresh.to_str().unwrap()), "{report}");
    assert!(
        !report.trim_end().ends_with("nothing unique"),
        "the column may not end on a reassurance while work sits on no remote:\n{report}"
    );
}

/// A clone this run could not examine is reported as not checked, never as clean.
///
/// Its HEAD commit is on disk and unreadable, which is what a truncated write or a bad
/// disk leaves behind. The old reading answered zero commits for any Git error and
/// folded the clone into "nothing unique" with the clones it had managed to read.
#[test]
fn a_clone_that_cannot_be_examined_is_not_called_clean() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);
    let broken = clone(&clones, &bare, "broken");
    clone(&clones, &bare, "sound");

    std::fs::write(broken.join("later.md"), "a commit of its own\n").unwrap();
    git::commit(&broken, "one loose object to damage");
    damage_head(&broken);

    let planted = Snapshot::of(&clones);
    let report = survey(&machine, &clones);
    assert!(!report.contains("nothing unique"), "one clone was never read:\n{report}");
    assert!(report.contains("not checked"), "{report}");
    assert!(report.contains(broken.to_str().unwrap()), "the clone is not named:\n{report}");
    planted.assert_unchanged(&Snapshot::of(&clones), "doctor --machine wrote in a broken clone");
}

/// A clone no other clone here can check is never called clean on its own bookkeeping.
///
/// One clone of a remote, and its branch is pushed. Its `refs/remotes/origin/work` says
/// so, and that ref is exactly the record this survey learnt not to believe: it is
/// written at the push and never corrected. With no other clone of the remote here,
/// nothing can say whether the branch is still there, so the answer is not known. It
/// used to be read straight off the ref and reported as nothing unique.
#[test]
fn a_clone_no_other_clone_can_check_is_not_called_clean() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);
    let alone = clone(&clones, &bare, "alone");
    work_and_push(&alone);

    let report = survey(&machine, &clones);
    assert!(!report.contains("nothing unique"), "nothing here checked that ref:\n{report}");
    assert!(report.contains("not checked"), "{report}");
    assert!(report.contains(alone.to_str().unwrap()), "the clone is not named:\n{report}");
    assert!(
        report.contains("no fresher clone") || report.contains("could not be checked"),
        "the report does not say why it could not answer:\n{report}"
    );
}

/// The JSON names the clone whose reading of the remote checked this one's refs.
///
/// A verdict that rests on another clone's reading is only auditable if the report says
/// which clone that was. `fresh` heard from the remote last, so `fresh` is what convicted
/// `keeper` of holding work no remote has.
#[test]
fn the_json_names_the_clone_that_checked_the_refs() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);
    let keeper = clone(&clones, &bare, "keeper");
    work_and_push(&keeper);
    delete_from_remote(&bare);
    let fresh = clone(&clones, &bare, "fresh");
    heard_last(&fresh);

    let answer = survey_json(&machine, &clones);
    assert!(answer.contains("\"witnesses\""), "the field is not there:\n{answer}");
    let named = answer
        .split("\"path\"")
        .find(|chunk| chunk.starts_with(&format!(": \"{}\"", keeper.display())))
        .unwrap_or_else(|| panic!("no row for the keeper:\n{answer}"));
    assert!(
        named.split("\"only_copy\"").next().unwrap().contains(fresh.to_str().unwrap()),
        "the row does not name the clone that checked it:\n{named}"
    );
}

/// A ref of another remote may not stand in for a ref of `origin`.
///
/// The group is the clones that share an `origin`, so the question is what `origin` has.
/// Here the work went to `backup` and never to `origin`, and both clones fetched it back
/// from `backup`. The commit survives the folder, because the other clone holds it, and
/// it is on no remote of this group. Reading `backup/feature` as an answer about
/// `origin` hid both facts and reported the group as clean.
#[test]
fn a_ref_of_another_remote_does_not_answer_for_origin() {
    let machine = Machine::new();
    let clones = planted_under(&machine);
    let bare = remote(&clones);
    let backup = spare_remote(&clones, "backup.git");

    let keeper = clone(&clones, &bare, "keeper");
    git(&keeper, &["switch", "--quiet", "--create", "feature"]);
    std::fs::write(keeper.join("feature.md"), "sent to the backup only\n").unwrap();
    git::commit(&keeper, "work the origin never saw");
    git(&keeper, &["remote", "add", "backup", backup.to_str().unwrap()]);
    git(&keeper, &["push", "--quiet", "backup", "feature"]);

    let fresh = clone(&clones, &bare, "fresh");
    git(&fresh, &["remote", "add", "backup", backup.to_str().unwrap()]);
    git(&fresh, &["fetch", "--quiet", "backup"]);
    heard_last(&fresh);

    let report = survey(&machine, &clones);
    assert!(report.contains("on no remote"), "the origin has no such branch:\n{report}");
    assert!(report.contains(keeper.to_str().unwrap()), "the clone is not named:\n{report}");
    assert!(report.contains("1 commit"), "the count is not there:\n{report}");
}
