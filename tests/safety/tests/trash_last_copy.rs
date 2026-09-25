//! The trash never becomes the only copy without a refusal and a line.
//!
//! `nodal reclaim` lets a home go when every commit in it exists somewhere else. Where
//! that somewhere else is another repository on this disk — a clone two directories away,
//! the project's own checkout, a reading of the remote taken in one — the verdict is only
//! as true as that repository. After the home is in the trash, the copy can go: somebody
//! deletes a branch in the clone, a host drops the branch a pull request merged.
//!
//! Nothing read the home again between the reclaim and the retention running out, so
//! `nodal gc` removed a directory that by then held the last copy of a commit. No
//! refusal, no line, and the only signal was the clock. That is the first line of the
//! never list broken on a timer, and this suite is the proof it is not.
//!
//! Every property here runs on a machine whose checkout has a bare `origin` beside it,
//! and every `git` Nodal starts on it may use the local file transport and nothing else.
//! `gc` reads; it never fetches.
//!
//! | property | test |
//! |---|---|
//! | a lost sibling copy keeps the home | `a_home_whose_sibling_copy_went_survives_its_retention` |
//! | a restored copy lets it go | `the_same_home_is_removed_once_the_copy_is_back` |
//! | a lost remote branch keeps it | `a_home_whose_remote_branch_went_survives_its_retention` |
//! | a copy nothing named still keeps it | `a_home_the_checkout_alone_held_is_kept_and_says_what_it_knows` |
//! | the ordinary home still goes | `a_home_whose_commits_are_in_the_checkout_is_removed_on_time` |
//! | a merged unit's home still goes | `a_merged_units_home_is_removed_although_the_squash_left_its_commits_here` |
//! | a detached head is read | `a_detached_head_whose_copy_went_keeps_its_home` |
//! | a branch neither reading misses is refused at the reclaim | `a_branch_the_reclaim_never_read_is_refused_at_the_reclaim` |
//! | an unreadable home is kept | `a_trashed_home_nothing_can_read_is_kept_and_the_report_says_why` |
//! | a reclaim writes into no other home | `a_reclaim_of_one_unit_writes_into_no_other_home` |
//! | an unreadable verdict is asked again | `a_verdict_this_binary_cannot_read_is_asked_again_rather_than_taken` |
//! | a record `done` left behind keeps nothing | `a_record_done_left_behind_does_not_keep_the_home` |
//!
//! Both hosts read every signal these properties use, so each one asserts the same thing
//! on Linux and on macOS.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::path::{Path, PathBuf};

use nodal_safety::InState as _;
use nodal_safety::project::resolved;
use nodal_safety::{Machine, Snapshot, git, json, stderr, stdout};

/// The unit every property here reclaims. It is one of the fixture's own handles.
const SLUG: &str = "worker-import";

/// The unit a reclaim of `SLUG` must not write into.
const BYSTANDER: &str = "payroll-export";

/// A path no ignore rule of the fixture covers, so a commit of it is work.
const ONLY: &str = "only-here.txt";

/// The branch a second repository keeps its copy of the home's commit on.
const COPY: &str = "rescued";

/// A second clone of the project, beside the checkout, where a person keeps a copy.
const SIBLING: &str = "sibling";

/// The branch a home pushes its work to, as a review branch on the remote.
const TOPIC: &str = "topic";

/// Only the local file transport, so no property here can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// A machine with a remote, whose trash keeps nothing.
///
/// The retention is nought, so a reclaimed home is expired the instant it is trashed and
/// the next `nodal gc` is the one that decides. What that sweep reads is the whole of
/// what these properties are about, and a fortnight of waiting is not part of it.
fn machine() -> Machine {
    let machine = Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY);
    let recipe = machine.source.join("nodal.toml");
    let written = std::fs::read_to_string(&recipe).unwrap();
    std::fs::write(&recipe, format!("{written}\n[reclaim]\ntrash_retention = 0\n")).unwrap();
    machine
}

/// A unit whose one commit exists only in its home, and that commit.
fn only_here(machine: &Machine, slug: &str) -> (PathBuf, String) {
    let home = machine.unit(slug);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work only this home has"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    (home, tip)
}

/// Reclaim the unit, insisting that it went ahead and that the home is in the trash.
fn reclaimed(machine: &Machine, slug: &str) -> PathBuf {
    let done = machine.nodal(&["reclaim", slug]);
    assert!(done.status.success(), "the reclaim refused: {}", stderr(&done));
    let said = stdout(&done);
    assert!(said.contains("nothing that is only here"), "{said}");
    let trashed = machine.trashed();
    assert_eq!(trashed.len(), 1, "the trash holds exactly the home that was reclaimed");
    trashed.into_iter().next().unwrap()
}

/// A second repository beside the checkout, holding `tip` on a branch of its own.
///
/// Beside the checkout and not inside it, because that is where the reading looks: a
/// destructive path asks the repositories under the checkout's parent
/// ([`nodal_core::doctor::scan::siblings`]).
fn sibling_holding(machine: &Machine, from: &Path, tip: &str) -> PathBuf {
    let parent = machine.source.parent().unwrap();
    let path = parent.join(SIBLING);
    let named = path.to_str().unwrap();
    git(parent, &["init", "--quiet", "--initial-branch", "main", named]);
    let spec = format!("{tip}:refs/heads/{COPY}");
    git(&path, &["fetch", "--quiet", from.to_str().unwrap(), &spec]);
    assert!(reaches(&path, tip), "the sibling does not hold the commit");
    path
}

/// A path as the filesystem spells it, for a report that resolves every path it prints.
///
/// One directory reached through a symbolic link and reached directly is one directory
/// with two spellings, and the macOS runner reaches its temporary directories through
/// one. A test that compared the unresolved spelling would pass on one host and fail on
/// the other.
fn named(path: &Path) -> String {
    resolved(path).to_str().expect("a utf-8 path").to_owned()
}

/// Whether a repository reaches this commit from a ref of its own.
fn reaches(repo: &Path, tip: &str) -> bool {
    git(repo, &["rev-list", "--all"]).lines().any(|line| line == tip)
}

/// The invariant: the trashed home is still there and plain Git still reads the work out
/// of it. A sweep that kept a row and took the directory would keep nothing.
fn readable(trash: &Path, tip: &str) {
    assert!(trash.is_dir(), "the sweep removed the home it said it kept");
    assert_eq!(git(trash, &["cat-file", "-t", tip]), "commit", "the commit is not readable");
    assert_eq!(std::fs::read_to_string(trash.join(ONLY)).unwrap(), "the only copy\n");
}

/// A verdict of "second local copy" rests on a ref in another repository, and that ref
/// can go while the home sits in the trash. When it does, the sweep keeps the home and
/// names the copy the reclaim rested on.
///
/// Nothing here pushed this commit and nothing here read the remote after the home did,
/// so the sweep cannot settle the remote question. It keeps the home over an open
/// question and says which question it is, which is not the same claim as "only here".
#[test]
fn a_home_whose_sibling_copy_went_survives_its_retention() {
    let machine = machine();
    let (home, tip) = only_here(&machine, SLUG);
    let sibling = sibling_holding(&machine, &home, &tip);
    let trash = reclaimed(&machine, SLUG);

    // The copy the verdict rested on goes, which is one ordinary command in a
    // repository Nodal never touched.
    git(&sibling, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&sibling, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(!reaches(&sibling, &tip), "the sibling still reaches the commit");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    assert!(report.contains(&named(&sibling)), "the line does not name it: {report}");
    assert!(report.contains(&format!("refs/heads/{COPY}")), "or the ref: {report}");
    assert!(report.contains("is gone"), "{report}");
    // Nothing on this machine read the remote after the home did, so the sweep does not
    // say "only here" over a question it could not settle. It says which one it is.
    assert!(report.contains("could not be checked"), "{report}");
    assert!(report.contains("nothing here read the remote"), "and why: {report}");
    readable(&trash, &tip);
    assert_eq!(machine.trashed(), vec![trash.clone()], "and the row was kept with it");

    carries_the_same_in_json(&machine);
    readable(&trash, &tip);
}

/// The same answer in `--json`, because a script deciding whether work is still
/// reachable reads that and not the table.
fn carries_the_same_in_json(machine: &Machine) {
    let carried = json(&machine.nodal(&["gc", "--json"]));
    let held = carried["held"].as_array().expect("the answer carries the homes it kept");
    assert_eq!(held.len(), 1, "{carried}");
    assert_eq!(held[0]["finding"]["count"], 1, "{carried}");
    assert_eq!(held[0]["finding"]["witness"]["kind"], "unchecked", "{carried}");
    assert_eq!(held[0]["gone"].as_array().map(Vec::len), Some(1), "{carried}");
    assert_eq!(held[0]["entry"]["rested"]["kind"], "safe", "{carried}");
}

/// The other half of the same property. A kept home keeps its row and stays expired, so
/// the next sweep reads it again: a copy somebody restores is all it takes.
#[test]
fn the_same_home_is_removed_once_the_copy_is_back() {
    let machine = machine();
    let (home, tip) = only_here(&machine, SLUG);
    let sibling = sibling_holding(&machine, &home, &tip);
    let trash = reclaimed(&machine, SLUG);

    git(&sibling, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&sibling, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(stdout(&machine.nodal(&["gc"])).contains("could not be checked"), "the sweep kept it");
    assert!(trash.is_dir(), "the home is still there");

    // The person puts the copy back, out of the trashed home itself, which is what the
    // line told them was there to rescue.
    let back = format!("{tip}:refs/heads/{COPY}");
    git(&sibling, &["fetch", "--quiet", trash.to_str().unwrap(), &back]);
    assert!(reaches(&sibling, &tip), "the sibling does not reach the commit again");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    assert!(!trash.exists(), "the home stayed although the copy is back: {}", stdout(&swept));
    assert!(machine.trashed().is_empty(), "and the row went with the directory");
}

/// A verdict of "proved on the remote" rests on a branch a host can delete the moment a
/// pull request merges. The sweep reads the home again and keeps it.
#[test]
fn a_home_whose_remote_branch_went_survives_its_retention() {
    let machine = machine();
    let (home, tip) = only_here(&machine, SLUG);
    git(&home, &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{TOPIC}")]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    let trash = reclaimed(&machine, SLUG);

    // The host deletes the branch, and the person's next fetch drops their reading of it.
    git(machine.origin(), &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
    git(&machine.source, &["fetch", "--quiet", "--prune", "origin"]);
    nodal_safety::git::fetched_later(&machine.source);
    git(&machine.source, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(!reaches(&machine.source, &tip), "the checkout still reaches the commit");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    assert!(report.contains("is only here"), "{report}");
    assert!(report.contains(&named(&machine.source)), "{report}");
    assert!(report.contains(&format!("refs/remotes/origin/{TOPIC}")), "{report}");
    readable(&trash, &tip);
}

/// A copy on a branch of the project's own checkout is not reported as a second copy at
/// all: the checkout's own refs are the denominator every disposition is drawn from, so
/// a commit one of them reaches is the project's history rather than this unit's work
/// ([`nodal_core::lifecycle::assess`]). The reclaim therefore writes down no copy.
///
/// The sweep still keeps the home, because it reads the machine again rather than the
/// row, and the line says exactly what it knows: this commit is only here, and this
/// home's reclaim recorded no copy outside it.
#[test]
fn a_home_the_checkout_alone_held_is_kept_and_says_what_it_knows() {
    let machine = machine();
    let (home, tip) = only_here(&machine, SLUG);
    let spec = format!("HEAD:refs/heads/{COPY}");
    git(&machine.source, &["fetch", "--quiet", home.to_str().unwrap(), &spec]);
    let trash = reclaimed(&machine, SLUG);

    git(&machine.source, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&machine.source, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(!reaches(&machine.source, &tip), "the checkout still reaches the commit");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    assert!(report.contains("recorded no copy outside it"), "{report}");
    readable(&trash, &tip);
}

/// The control, and the reason this is a reading rather than a refusal to collect. A
/// home whose commits the project's own checkout reaches holds no last copy of anything,
/// and the retention running out removes it exactly as it always did.
#[test]
fn a_home_whose_commits_are_in_the_checkout_is_removed_on_time() {
    let machine = machine();
    let home = machine.unit(SLUG);
    assert!(home.is_dir());
    let trash = reclaimed(&machine, SLUG);

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    assert!(stdout(&swept).contains("1 home"), "{}", stdout(&swept));
    assert!(!trash.exists(), "the sweep kept a home holding nothing: {}", stdout(&swept));
    assert!(machine.trashed().is_empty());
}

/// A reclaim of one unit writes into that unit's home and into the registry, and into
/// nothing else.
///
/// It used to write three things into every other open home of the project — a rewritten
/// `WORKUNIT.md`, a fetched set of refs, and a dangling tree object — under a report that
/// said it had changed nothing of theirs. The whole tree is compared, `.git` included, so
/// every one of the three is a difference.
#[test]
fn a_reclaim_of_one_unit_writes_into_no_other_home() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let bystander = machine.unit(BYSTANDER);
    assert!(home.is_dir());

    let before = Snapshot::of(&bystander);
    let refs_before = git(&bystander, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    assert!(before.len() > 1, "the snapshot read the home");

    let done = machine.nodal(&["reclaim", SLUG]);
    assert!(done.status.success(), "{}", stderr(&done));
    assert!(stdout(&done).contains("wrote into no other unit's home"), "{}", stdout(&done));

    before.assert_unchanged(&Snapshot::of(&bystander), "a reclaim wrote into another home");
    assert_eq!(
        git(&bystander, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        refs_before,
        "a reclaim moved a ref in another home"
    );
}

/// `nodal merge` squashes the branch and writes the commits it folded to
/// `refs/nodal/<unit>/premerge` before it reclaims the unit. Those commits exist nowhere
/// else by construction: the merge is what put their content on the target branch, and
/// the objects behind them were never meant to outlive it.
///
/// A sweep that read that ref as this home's work would keep every merged unit's home
/// for ever. The retention still removes it.
#[test]
fn a_merged_units_home_is_removed_although_the_squash_left_its_commits_here() {
    let machine = machine();
    let home = machine.unit(SLUG);
    for (name, text) in [("first.txt", "one\n"), ("second.txt", "two\n")] {
        std::fs::write(home.join(name), text).unwrap();
        git(&home, &["add", "--all"]);
        git(&home, &["commit", "--quiet", "--message", name]);
    }
    let folded = git(&home, &["rev-parse", "HEAD"]);

    let merged = machine.nodal(&["merge", SLUG, "--yes"]);
    assert!(merged.status.success(), "{}", stderr(&merged));
    let trash = machine.trashed().pop().expect("the merge reclaimed the home");
    assert_eq!(
        git(&trash, &["rev-parse", &format!("refs/nodal/{}/premerge", unit_id(&machine))]),
        folded,
        "the premerge ref is the one this property is about"
    );

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    assert!(!trash.exists(), "a squashed branch kept the home: {}", stdout(&swept));
    assert!(machine.trashed().is_empty(), "and the row went with the directory");
}

/// A home whose `HEAD` is detached still holds the person's work, and `HEAD` is the only
/// ref that reaches it. A reading that named the unit's branch alone would call such a
/// home empty and let the last copy of a commit go.
#[test]
fn a_detached_head_whose_copy_went_keeps_its_home() {
    let machine = machine();
    let home = machine.unit(SLUG);
    git(&home, &["checkout", "--quiet", "--detach"]);
    std::fs::write(home.join(ONLY), "the only copy\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work on a detached head"]);
    let tip = git(&home, &["rev-parse", "HEAD"]);
    let sibling = sibling_holding(&machine, &home, &tip);
    let trash = reclaimed(&machine, SLUG);

    git(&sibling, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&sibling, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(!reaches(&sibling, &tip), "the sibling still reaches the commit");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    readable(&trash, &tip);
}

/// Nothing is removed on a reading nobody could make. A home this account cannot open is
/// not a home proved empty, and the report names the directory and what went wrong.
///
/// Both hosts refuse a directory with no permissions to the account that owns it, so the
/// property is asserted on both. The permissions are given back at the end, so the
/// temporary directory can be removed.
#[test]
fn a_trashed_home_nothing_can_read_is_kept_and_the_report_says_why() {
    let machine = machine();
    let home = machine.unit(SLUG);
    assert!(home.is_dir());
    let trash = reclaimed(&machine, SLUG);
    permissions(&trash, 0o000);

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    permissions(&trash, 0o755);
    assert!(trash.is_dir(), "a home nobody could read was removed: {report}");
    assert!(report.contains("trashed home"), "the report does not name it: {report}");
    // The line is the directory and then the reason. The reason is the host's own
    // wording for a directory it would not open, so the property is that there is one.
    let named = format!("{}: ", trash.display());
    let (_, why) = report.split_once(&named).unwrap_or_else(|| panic!("no line for it: {report}"));
    assert!(!why.trim().is_empty(), "the line does not say why: {report}");
    assert_eq!(machine.trashed(), vec![trash], "and the row stayed with it");
}

/// Set the mode of one directory.
fn permissions(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A verdict this binary cannot read is a verdict, and a snapshot beside it is not
/// permission to remove the home.
///
/// The row is put into that state by hand, because no version of this binary writes one:
/// text a later Nodal wrote in the `rested` column, and the `snapshot` column filled as a
/// forced reclaim fills it. The reading used to answer `forced` for that pair, and a
/// forced reclaim is the one answer the sweep does not ask again — so an unreadable
/// verdict removed the home on the clock alone. It reads as `unrecorded` now: the sweep
/// reads the home again and keeps it over the copy that went.
///
/// This shape carries into the adversarial grid (lane C) when it lands: a row whose
/// verdict this binary cannot parse.
#[test]
fn a_verdict_this_binary_cannot_read_is_asked_again_rather_than_taken() {
    let machine = machine();
    let (home, tip) = only_here(&machine, SLUG);
    let sibling = sibling_holding(&machine, &home, &tip);
    let trash = reclaimed(&machine, SLUG);
    {
        let store = machine.store();
        let later = String::from("{\"kind\":\"from a later nodal\"}");
        let snapshot = format!("refs/nodal/{}/wip", unit_id(&machine));
        store
            .conn()
            .execute("UPDATE trash SET rested = ?, snapshot = ?", [later, snapshot])
            .unwrap();
    }

    git(&sibling, &["update-ref", "-d", &format!("refs/heads/{COPY}")]);
    git(&sibling, &["reflog", "expire", "--expire=now", "--all"]);
    assert!(!reaches(&sibling, &tip), "the sibling still reaches the commit");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(report.contains(&format!("kept: {}", &tip[..8])), "{report}");
    readable(&trash, &tip);
    assert_eq!(machine.trashed(), vec![trash], "and the row was kept with it");
}

/// The identifier of the one unit this machine has, as its refs spell it.
fn unit_id(machine: &Machine) -> String {
    let store = machine.store();
    let project = machine.project(&store);
    nodal_core::store::units::list(store.conn(), project.id)
        .unwrap()
        .into_iter()
        .find(|unit| unit.slug.as_str() == SLUG)
        .expect("the unit is registered")
        .id
        .to_string()
}

/// A reclaim and a sweep read the same refs, so neither can hold the other to a ref it
/// never saw.
///
/// The reclaim read the working tree and `HEAD` and no `refs/heads/*` at all, and the
/// sweep read every ref the home holds. A commit only a branch reached therefore went
/// into the trash unexamined and was found on the first sweep, and the home was kept from
/// then on with nothing a person could do to release it.
///
/// Both readings are one reading now (`nodal_core::lifecycle::assess::Work`), so the
/// commit is refused at the reclaim, where the person still has the home and can act on
/// it. Both branches a home can strand such a commit on are here.
///
/// A home carries every branch the base it was copied from had, `refs/heads/main` among
/// them, frozen at the moment the base was built. That is the first.
///
/// The unit's own branch is the second, and it is the one that looked safe. In a home
/// whose `HEAD` is detached the branch can be ahead of `HEAD`.
#[test]
fn a_branch_the_reclaim_never_read_is_refused_at_the_reclaim() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let branch = git(&home, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&home, &["checkout", "--quiet", "-B", "main"]);
    std::fs::write(home.join(ONLY), "on a branch nobody reads\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work on the copied branch"]);
    let stranded = git(&home, &["rev-parse", "refs/heads/main"]);
    git(&home, &["checkout", "--quiet", &branch]);
    assert!(!reaches(&machine.source, &stranded), "the checkout already holds the commit");

    // And the same on the unit's own branch, which `HEAD` is then moved off.
    std::fs::write(home.join(ONLY), "on the unit's own branch\n").unwrap();
    git(&home, &["add", "--all"]);
    git(&home, &["commit", "--quiet", "--message", "work the detached head does not reach"]);
    let ahead = git(&home, &["rev-parse", "HEAD"]);
    git(&home, &["checkout", "--quiet", "--detach", "HEAD~1"]);
    assert_eq!(git(&home, &["rev-parse", &branch]), ahead, "the branch is not ahead of HEAD");
    assert!(!reaches(&machine.source, &ahead), "the checkout already holds the commit");

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "a branch nothing else has went: {}", stdout(&refused));
    assert!(home.is_dir(), "the refusal moved the home");
    assert!(machine.trashed().is_empty(), "the refusal put something in the trash");

    // The person puts both commits somewhere else, and the home goes on time.
    let beside = sibling_holding(&machine, &home, &stranded);
    let kept = format!("{ahead}:refs/heads/kept");
    git(&beside, &["fetch", "--quiet", home.to_str().unwrap(), &kept]);
    let trash = reclaimed(&machine, SLUG);
    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    assert!(!trash.exists(), "a branch both readings proved kept the home: {}", stdout(&swept));
    assert!(machine.trashed().is_empty(), "and the row went with the directory");
}

/// A record `nodal done` wrote is not the home's work, and the retention still removes the
/// home.
///
/// `nodal done` commits the whole home to `refs/nodal/<unit>/wip` on every run, whether or
/// not `--wip` sends it. That commit is built on `HEAD`, so no branch reaches it and no
/// push carries it. The sweep read that ref by name, found the commit only there, and kept
/// the home — on that sweep and on every sweep after it, with nothing a person could do to
/// release it. Any unit that had ever run `done` was affected.
///
/// The sweep reads the ref the trash row names instead. An ordinary reclaim names none,
/// because it had no work to preserve.
#[test]
fn a_record_done_left_behind_does_not_keep_the_home() {
    let machine = machine();
    let home = machine.unit(SLUG);
    let sent = machine.nodal(&["done", SLUG]);
    assert!(sent.status.success(), "the push failed: {}", stderr(&sent));
    let wip = format!("refs/nodal/{}/wip", unit_id(&machine));
    let record = git(&home, &["rev-parse", &wip]);
    assert!(!reaches(&machine.source, &record), "the checkout holds the record already");

    let trash = reclaimed(&machine, SLUG);
    assert_eq!(git(&trash, &["rev-parse", &wip]), record, "the record is what this is about");

    let swept = machine.nodal(&["gc"]);
    assert!(swept.status.success(), "{}", stderr(&swept));
    let report = stdout(&swept);
    assert!(!trash.exists(), "a record `done` wrote kept the home: {report}");
    assert!(machine.trashed().is_empty(), "and the row went with the directory");
}
