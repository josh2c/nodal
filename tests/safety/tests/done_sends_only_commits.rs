//! What `nodal done` sends, and what a reclaim takes back off the remote.
//!
//! A push is the one act Nodal performs that other people can see, and it cannot be
//! undone. Two properties follow, and each one is a promise a person is entitled to
//! make on Nodal's behalf to whoever else reads the remote.
//!
//! **`done` sends what was committed.** A file a person never committed is not theirs
//! to have published for them, so the work-in-progress snapshot stays here. It is still
//! taken — the home at the moment of the `done` is worth having — and `--wip` is the one
//! way it goes, which is why the third test is here: a default that cannot be overridden
//! is a limitation, and one that can is a choice.
//!
//! **A reclaim takes Nodal's own refs back and leaves the branch.** The branch is the
//! person's work and their colleagues' to read. `refs/nodal/<unit>/*` is bookkeeping
//! about a unit that is about to stop existing, and leaving it out there is litter with
//! a unit identifier on it.
//!
//! The remote here is a bare repository in the same temporary directory as everything
//! else, so these tests reach no network of any kind. What they assert is what `git`
//! was asked to do, which is the whole of what a remote would have seen.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::{Machine, git, git_ok, stdout};

/// A path no ignore rule of the fixture covers, so a file there is work.
const UNTRACKED: &str = "notes.txt";

/// What a person writes into it, which is the thing that must not travel.
const PRIVATE: &str = "the note I never committed\n";

/// The namespace Nodal keeps its own refs in. Nothing under it is a person's branch.
const NAMESPACE: &str = "refs/nodal/";

/// A machine whose unit pushes to a bare repository next door.
///
/// The remote is a directory, not a host. Every property here is about which refs `git`
/// was given, and a bare repository answers that question exactly as a server would.
fn machine_pushing_somewhere(slug: &str) -> (Machine, PathBuf, PathBuf) {
    let machine = Machine::new();
    let home = machine.unit(slug);
    let remote = machine.state.parent().unwrap().join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git_ok(&remote, &["init", "--bare", "-q", "-b", "main"]);
    let url = remote.to_str().unwrap();
    // A home carries the base's remote, so this either points that one somewhere else
    // or names the first one, and the test says which without depending on either.
    let named = git(&home, &["remote"]);
    let action = if named.lines().any(|name| name == "origin") { "set-url" } else { "add" };
    git_ok(&home, &["remote", action, "origin", url]);
    (machine, home, remote)
}

/// Every ref the remote holds, in full.
fn refs_on(remote: &Path) -> Vec<String> {
    let listed = git(remote, &["for-each-ref", "--format=%(refname)"]);
    listed.lines().map(str::to_owned).collect()
}

/// The refs of Nodal's own that a remote holds.
fn nodal_refs_on(remote: &Path) -> Vec<String> {
    refs_on(remote).into_iter().filter(|name| name.starts_with(NAMESPACE)).collect()
}

/// The work-in-progress ref the home holds, which is where a snapshot stays.
fn wip_ref_in(home: &Path) -> Option<String> {
    let listed = git(home, &["for-each-ref", "--format=%(refname)", NAMESPACE]);
    listed.lines().find(|name| name.ends_with("/wip")).map(str::to_owned)
}

/// Every path every ref of a remote can reach.
fn files_on(remote: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    for reference in refs_on(remote) {
        let listed = git(remote, &["ls-tree", "-r", "--name-only", &reference]);
        paths.extend(listed.lines().map(str::to_owned));
    }
    paths
}

/// A file a person did not commit does not reach the remote, and the snapshot of it
/// stays in the home where a later reclaim and their own `git` can still read it.
#[test]
fn an_untracked_file_never_leaves_this_machine() {
    let (machine, home, remote) = machine_pushing_somewhere("worker-import");
    std::fs::write(home.join(UNTRACKED), PRIVATE).unwrap();

    let sent = machine.nodal(&["done", "worker-import"]);
    assert!(sent.status.success(), "nodal done failed: {}", nodal_safety::stderr(&sent));

    assert!(
        refs_on(&remote).contains(&String::from("refs/heads/nodal/worker-import")),
        "the branch did not reach the remote: {:?}",
        refs_on(&remote)
    );
    assert_eq!(
        nodal_refs_on(&remote),
        Vec::<String>::new(),
        "a ref of nodal's own reached the remote without --wip"
    );
    assert!(
        !files_on(&remote).contains(&String::from(UNTRACKED)),
        "the file nobody committed is on the remote: {:?}",
        files_on(&remote)
    );

    let kept = wip_ref_in(&home).expect("the snapshot is still taken, here");
    assert_eq!(
        git(&home, &["ls-tree", "-r", "--name-only", &kept])
            .lines()
            .filter(|path| *path == UNTRACKED)
            .count(),
        1,
        "the snapshot that stayed here does not hold the work it is for"
    );
    let told = stdout(&sent);
    assert!(
        told.contains("--wip"),
        "the report does not say how the snapshot would be sent: {told}"
    );
}

/// A reclaim deletes what Nodal wrote on the remote for this unit, and leaves the
/// branch. The ref is put there with plain `git` rather than by `--wip`, so the property
/// is about what a reclaim removes and not about how the ref got out there.
#[test]
fn a_reclaim_takes_nodals_own_refs_off_the_remote_and_leaves_the_branch() {
    let (machine, home, remote) = machine_pushing_somewhere("worker-import");
    std::fs::write(home.join(UNTRACKED), PRIVATE).unwrap();

    let sent = machine.nodal(&["done", "worker-import"]);
    assert!(sent.status.success(), "nodal done failed: {}", nodal_safety::stderr(&sent));
    let wip = wip_ref_in(&home).expect("a home with an untracked file has a snapshot");
    git_ok(&home, &["push", "origin", &format!("{wip}:{wip}")]);
    assert_eq!(nodal_refs_on(&remote), vec![wip.clone()], "the setup put no ref out there");

    let reclaimed = machine.nodal(&["reclaim", "worker-import", "--force"]);
    assert!(
        reclaimed.status.success(),
        "nodal reclaim failed: {}",
        nodal_safety::stderr(&reclaimed)
    );

    assert!(
        refs_on(&remote).contains(&String::from("refs/heads/nodal/worker-import")),
        "the reclaim took the branch off the remote: {:?}",
        refs_on(&remote)
    );
    assert_eq!(
        nodal_refs_on(&remote),
        Vec::<String>::new(),
        "the reclaim left a ref of nodal's own on the remote"
    );
    let told = stdout(&reclaimed);
    assert!(told.contains(&wip), "the report does not say which ref it deleted: {told}");
}

/// `--wip` sends the snapshot, so the default is a choice a person can make the other
/// way. The flag is what the contract documents as sending uncommitted files.
#[test]
fn wip_is_the_one_way_uncommitted_work_goes() {
    let (machine, home, remote) = machine_pushing_somewhere("worker-import");
    std::fs::write(home.join(UNTRACKED), PRIVATE).unwrap();

    let sent = machine.nodal(&["done", "worker-import", "--wip"]);
    assert!(sent.status.success(), "nodal done --wip failed: {}", nodal_safety::stderr(&sent));

    let wip = wip_ref_in(&home).expect("a home with an untracked file has a snapshot");
    assert_eq!(nodal_refs_on(&remote), vec![wip], "--wip did not send the snapshot");
    assert!(
        files_on(&remote).contains(&String::from(UNTRACKED)),
        "the snapshot the remote holds does not carry the work it is for"
    );
}
