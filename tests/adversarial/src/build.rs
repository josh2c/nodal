//! Build one shape in a temporary machine, and keep the commands that made it.
//!
//! The order the axes are applied in is the order a person's week happens in, and it is the
//! order that makes the axes independent of each other:
//!
//! 1. a machine: the fixture project, a bare `origin` beside it, and a state directory;
//! 2. a home, made by `nodal new`;
//! 3. one commit of work in the home, and the [`Refs`] axis puts it under a ref;
//! 4. the [`Tree`] axis leaves the working tree in one of four states;
//! 5. the [`Witness`] axis builds a store beside the checkout;
//! 6. the [`Observed`] axis pushes the work and moves what the checkout saw;
//! 7. the [`Occupant`] axis starts something against the home.
//!
//! ## The work is one commit, and the ref is the axis
//!
//! Every shape writes the same file with the same content. Only the ref it ends up under
//! differs, so a disagreement is about the ref and never about the content.
//!
//! ## What a clone fetches, and why that is the interesting half
//!
//! `git clone` fetches `refs/heads/*` and the tags. It fetches no stash, no note and
//! nothing under `refs/nodal/`. So eight of the ten witness topologies hold a copy of the
//! work when the [`Refs`] axis put it on a branch, a tag or a detached `HEAD`, and hold no
//! copy of it when the axis put it in the stash, in a note or in a `wip` record. That is
//! not a defect of the fixture: it is the fact that a clone of your repository is not a
//! copy of your stash, and the grid asserts that both answerers know it.
//!
//! The two shallow topologies are the exception. A depth needs a ref to count from, so
//! those two shapes make one in the home for the clone to name
//! ([`TIP`] and [`ABOVE`]), and then the work is on a branch whatever the [`Refs`] axis did
//! with it. The shapes are still worth their place: one store holds the work and not its
//! history, and the other holds a commit and not the work it stands on.
//!
//! ## Reproduction
//!
//! [`Steps`] runs every `git` call and records it. [`Built::script`] is therefore a shell
//! script, in order, that makes the shape again, and a disagreement prints it.

use std::path::{Path, PathBuf};

use nodal_safety::process::Owned;
use nodal_safety::{Machine, git};

use crate::check::Check;
use crate::oracle::{self, Answer};
use crate::shape::{Observed, Occupant, Refs, Shape, Tree, Witness};

/// The unit every shape is built around. One of the fixture's own handles.
pub const SLUG: &str = "worker-import";

/// The file the work is in. No ignore rule of the fixture covers it, so a commit of it is
/// work and an uncommitted copy of it is work.
const WORK: &str = "only-here.txt";

/// A tracked path of the fixture, which the stash and the modified-tree values need.
const TRACKED: &str = "apps/web/app/page.tsx";

/// A path an ignore rule of the fixture covers.
///
/// The ignored value of the [`Tree`] axis and the write-descriptor value of the [`Occupant`]
/// axis both use it, because both are about something no other conjunct can see.
const IGNORED: &str = "dist/dev.sqlite";

/// The branch the work is pushed to on the remote.
const TOPIC: &str = "topic";

/// The ref a shallow clone of the work counts a depth from.
const TIP: &str = "refs/heads/adversarial-tip";

/// The ref a shallow clone that cuts the work off counts a depth from.
const ABOVE: &str = "refs/heads/adversarial-above";

/// Where the witness store goes, beside the checkout, which is where the sibling scan looks.
const WITNESS: &str = "witness";

/// The URL every shape's remote is named by.
///
/// A server and not a path, and that is deliberate. A remote that is a directory on this disk
/// is a remote Nodal reads **directly**: the relation is `IsTheRemote`, the refs under
/// `refs/heads/` of that directory are the remote's own refs, and the whole of the witness and
/// freshness rule is never reached. Every test in the suite that used a local path therefore
/// asserted the short path, and the absence of a server-shaped remote is what let a
/// regression in the witness path through.
///
/// So the bare repository is moved off the machine, the remote is named by this URL, and
/// `url.<path>.insteadOf` carries the transport back to the directory. Nodal reads the
/// configured URL, sees a server, and takes the witness path. Nothing reaches a network:
/// `insteadOf` is resolved by Git before any transport is chosen.
const SERVED: &str = "ssh://git@example.invalid/project.git";

/// Only the local file transport, so no shape can reach a network.
const ONLY_LOCAL: (&str, &str) = ("GIT_ALLOW_PROTOCOL", "file");

/// No proxy either, for the same reason.
const NO_PROXY: (&str, &str) = ("GIT_PROXY_COMMAND", "false");

/// The four directories a shape is built across, read once.
///
/// Every axis but the tree needs two or three of them, and each of the four is derived rather
/// than given: a builder that took them one at a time would take six arguments and could be
/// handed a home and a checkout of two different machines.
struct Where {
    /// The person's checkout, which is the witness that observes the remote.
    checkout: PathBuf,
    /// Where a store built beside the checkout goes, which is where the sibling scan looks.
    beside: PathBuf,
    /// The bare repository the remote URL resolves to.
    origin: PathBuf,
    /// The home under judgement.
    home: PathBuf,
}

/// One shape, built, with everything a comparison needs to read it.
pub struct Built {
    /// The machine it was built in. Dropped with the shape, and every path goes with it.
    pub machine: Machine,
    /// The directory the bare remote was moved to, outside the machine.
    ///
    /// Held so that it outlives the shape, and outside [`Built::root`] so that the oracle
    /// reads it as the remote it is and not as a second store on this disk.
    _served: tempfile::TempDir,
    /// The bare repository the remote URL resolves to.
    pub origin: PathBuf,
    /// The home under judgement.
    pub home: PathBuf,
    /// The temporary root every store of this machine is under.
    pub root: PathBuf,
    /// The commands that made it, in order.
    pub script: Vec<String>,
    /// The axis value this host would not make, and why. `None` when the shape is whole.
    pub skipped: Option<String>,
    /// What the shape started, held so that it outlives the readings.
    held: Vec<Owned>,
    /// The hidden process, when the shape has one. Owned here and not by the kit, because
    /// the kit's own process type waits for a child to be reaped and nothing reaps this one.
    hidden: Option<Hidden>,
}

impl Built {
    /// Ask the predicate about this home.
    #[must_use]
    pub fn check(&self) -> Check {
        Check::read(&self.machine.nodal(&["reclaim", SLUG, "--check", "--json"]))
    }

    /// Ask the oracle about this home.
    #[must_use]
    pub fn oracle(&self) -> Answer {
        oracle::ask(&self.home, &self.root)
    }

    /// The shape as a shell script, with the temporary paths as they were.
    ///
    /// Printed by a disagreement and by nothing else. A reader pastes it into a terminal and
    /// has the shape; the paths are gone by then, so the first line says what they were.
    #[must_use]
    pub fn reproduction(&self, shape: Shape) -> String {
        let mut lines = vec![
            format!("# shape {shape}"),
            format!("# home     {}", self.home.display()),
            format!("# checkout {}", self.machine.source.display()),
        ];
        lines.extend(self.script.iter().cloned());
        lines.push(format!("nodal reclaim {SLUG} --check --json"));
        lines.join("\n")
    }
}

impl Drop for Built {
    /// Stop what the shape started, before the machine's directories go.
    ///
    /// The kit's own process type does this for each one; the order is what is added here. A
    /// directory removed under a running process leaves that process alive with a working
    /// directory nothing can name, on every run, until the machine is restarted.
    fn drop(&mut self) {
        // The hidden child first. It is in the group the kit's own process leads, so a reclaim
        // of that group signals it — and the kit then waits for it to leave the table, which a
        // child nobody has reaped never does.
        self.hidden = None;
        for owned in &mut self.held {
            owned.reclaim();
        }
    }
}

/// Every `git` call of one build, recorded as it is made.
struct Steps {
    /// The commands, in order, as a shell would take them.
    script: Vec<String>,
}

impl Steps {
    /// Run one `git` call in a directory and record it.
    fn git(&mut self, at: &Path, args: &[&str]) -> String {
        self.script.push(format!("git -C {} {}", at.display(), args.join(" ")));
        git(at, args)
    }

    /// Run one `git` call that is asked because it may fail, and record it.
    ///
    /// One caller: the branch a store keeps of its own accord. A store that did not fetch the
    /// work cannot name a branch at it, and that is a shape and not a failure.
    fn try_git(&mut self, at: &Path, args: &[&str]) -> bool {
        self.script.push(format!("git -C {} {} || true", at.display(), args.join(" ")));
        nodal_safety::try_git(at, args).status.success()
    }

    /// Record a line that is not a `git` call, so the script still makes the shape.
    fn note(&mut self, line: String) {
        self.script.push(line);
    }
}

/// Build one shape.
///
/// # Panics
///
/// If the machine, the home or the work could not be made, which is a shape that was never
/// built rather than a shape that disagreed.
#[must_use]
pub fn build(shape: Shape) -> Built {
    let machine = Machine::with_remote().with_env(ONLY_LOCAL).with_env(NO_PROXY);
    let root = machine.source.parent().expect("the checkout is in the machine").to_path_buf();
    let mut steps = Steps { script: Vec::new() };
    let (served, origin) = serve(&mut steps, &machine, SERVED);
    steps.note(format!("nodal new --name {SLUG} 'one unit of the fixture project'"));
    let home = machine.unit(SLUG);
    reach(&mut steps, &home, &origin, SERVED);
    let at = Where {
        checkout: machine.source.clone(),
        beside: root.clone(),
        origin: origin.clone(),
        home: home.clone(),
    };
    let work = work(&mut steps, &home, shape.refs);
    tree(&mut steps, &home, shape.tree);
    witness(&mut steps, &at, &work, shape.witness);
    observed(&mut steps, &at, &work, shape.observed);
    let started = occupant(&mut steps, &home, shape.occupant);
    Built {
        machine,
        _served: served,
        origin,
        home,
        root,
        script: steps.script,
        skipped: started.skipped,
        held: started.held,
        hidden: started.hidden,
    }
}

/// Move the machine's bare remote off the machine, and name it by a URL.
///
/// The move is what takes the remote out of the oracle's walk. The URL is what takes Nodal
/// down the witness path. [`SERVED`] says why both matter.
///
/// # Panics
///
/// If the directory could not be made, or the bare repository could not be moved.
fn serve(steps: &mut Steps, machine: &Machine, url: &str) -> (tempfile::TempDir, PathBuf) {
    let served = tempfile::TempDir::new().expect("a directory for the remote");
    let origin = served.path().join("origin.git");
    std::fs::rename(machine.origin(), &origin).expect("the bare remote is moved off the machine");
    steps.note(format!("mv {} {}", machine.origin().display(), origin.display()));
    reach(steps, &machine.source, &origin, url);
    (served, origin)
}

/// Name a repository's `origin` by `url`, and carry the transport back to `origin`.
fn reach(steps: &mut Steps, repo: &Path, origin: &Path, url: &str) {
    let path = std::fs::canonicalize(origin).expect("the bare remote is there");
    let key = format!("url.{}.insteadOf", path.display());
    steps.git(repo, &["config", "--local", &key, url]);
    steps.git(repo, &["remote", "set-url", "origin", url]);
}

// ---------------------------------------------------------------------------
// The work, and the ref it goes under.
// ---------------------------------------------------------------------------

/// Put one commit of work in the home, under the ref this axis names, and answer its oid.
fn work(steps: &mut Steps, home: &Path, refs: Refs) -> String {
    match refs {
        Refs::Branch => on_a_branch(steps, home),
        Refs::Tag => on_a_tag(steps, home),
        Refs::Stash => in_the_stash(steps, home),
        Refs::Wip => in_a_record(steps, home),
        Refs::DetachedHead => on_a_detached_head(steps, home),
        Refs::Notes => in_a_note(steps, home),
    }
}

/// Write the work file and commit it where the home stands. Answers the commit.
fn committed(steps: &mut Steps, home: &Path) -> String {
    write(home, WORK, "the only copy\n");
    steps.note(format!("printf 'the only copy\\n' > {}", home.join(WORK).display()));
    steps.git(home, &["add", "--all"]);
    steps.git(home, &["commit", "--quiet", "--message", "work only this home has"]);
    steps.git(home, &["rev-parse", "HEAD"])
}

/// A branch the home is not checked out on.
fn on_a_branch(steps: &mut Steps, home: &Path) -> String {
    let was = steps.git(home, &["rev-parse", "--abbrev-ref", "HEAD"]);
    steps.git(home, &["switch", "--quiet", "--create", "side-work"]);
    let work = committed(steps, home);
    steps.git(home, &["switch", "--quiet", &was]);
    work
}

/// A lightweight tag, and no branch on the commit.
fn on_a_tag(steps: &mut Steps, home: &Path) -> String {
    let was = steps.git(home, &["rev-parse", "--abbrev-ref", "HEAD"]);
    steps.git(home, &["switch", "--quiet", "--detach"]);
    let work = committed(steps, home);
    steps.git(home, &["tag", "only-here"]);
    steps.git(home, &["switch", "--quiet", &was]);
    work
}

/// `refs/stash`, over a change to a tracked file.
fn in_the_stash(steps: &mut Steps, home: &Path) -> String {
    append(home, TRACKED, "// only this home has this line\n");
    steps.note(format!(
        "printf '// only this home has this line\\n' >> {}",
        home.join(TRACKED).display()
    ));
    steps.git(home, &["stash", "push", "--quiet"]);
    steps.git(home, &["rev-parse", "refs/stash"])
}

/// A record under `refs/nodal/`, whose tree is the working tree, as `done` writes one.
///
/// The oracle leaves `refs/nodal/` out of the loss set for the reason
/// `tests/safety/tests/home_refs.rs` gives, and so does Nodal. The value is on the axis so
/// that a record which ever held content the working tree does not would show up as a tree
/// one answerer counts and the other does not.
fn in_a_record(steps: &mut Steps, home: &Path) -> String {
    write(home, WORK, "the only copy\n");
    steps.note(format!("printf 'the only copy\\n' > {}", home.join(WORK).display()));
    steps.git(home, &["add", "--", WORK]);
    let tree = steps.git(home, &["write-tree"]);
    let parent = steps.git(home, &["rev-parse", "HEAD"]);
    let record = steps.git(
        home,
        &["commit-tree", &tree, "-p", &parent, "-m", "the working tree, as a done takes it"],
    );
    steps.git(home, &["update-ref", &format!("refs/nodal/{}/wip", unit_id(home)), &record]);
    steps.git(home, &["rm", "--quiet", "--cached", "--", WORK]);
    std::fs::remove_file(home.join(WORK)).expect("the file the record holds is taken back");
    steps.note(format!("rm {}", home.join(WORK).display()));
    record
}

/// A commit made with `HEAD` detached, which `HEAD` reaches and no branch does.
fn on_a_detached_head(steps: &mut Steps, home: &Path) -> String {
    steps.git(home, &["switch", "--quiet", "--detach"]);
    committed(steps, home)
}

/// A note on a commit every store already has. The note itself is only here.
fn in_a_note(steps: &mut Steps, home: &Path) -> String {
    steps.git(home, &["notes", "add", "--message", "a note only this home has"]);
    steps.git(home, &["rev-parse", "refs/notes/commits"])
}

/// The identifier the home records for itself, which its own records are namespaced under.
fn unit_id(home: &Path) -> String {
    std::fs::read_to_string(home.join(".nodal/id"))
        .expect("a managed home records which unit it is")
        .trim()
        .to_owned()
}

// ---------------------------------------------------------------------------
// The working tree.
// ---------------------------------------------------------------------------

/// Leave the working tree in the state this axis names.
fn tree(steps: &mut Steps, home: &Path, state: Tree) {
    match state {
        Tree::Clean => steps.note(String::from("# the working tree is left clean")),
        Tree::TrackedModified => {
            append(home, TRACKED, "// an edit nothing else has\n");
            steps.note(format!(
                "printf '// an edit nothing else has\\n' >> {}",
                home.join(TRACKED).display()
            ));
        }
        Tree::Untracked => {
            write(home, "scratch.md", "a note to self\n");
            steps.note(format!(
                "printf 'a note to self\\n' > {}",
                home.join("scratch.md").display()
            ));
        }
        Tree::IgnoredOnly => {
            write(home, IGNORED, "a local database\n");
            steps.note(format!("printf 'a local database\\n' > {}", home.join(IGNORED).display()));
        }
    }
}

// ---------------------------------------------------------------------------
// The witness store.
// ---------------------------------------------------------------------------

/// Build the store this axis names, beside the checkout.
fn witness(steps: &mut Steps, at: &Where, work: &str, shape: Witness) {
    let (beside, home) = (at.beside.clone(), at.home.as_path());
    let path = home.to_str().expect("a printable home").to_owned();
    match shape {
        Witness::Nothing => steps.note(String::from("# nothing is built beside the checkout")),
        Witness::FullClone => {
            steps.git(&beside, &["clone", "--quiet", "--no-checkout", &path, WITNESS]);
            keeps(steps, &beside.join(WITNESS), work);
        }
        Witness::Bare => {
            steps.git(&beside, &["clone", "--quiet", "--bare", &path, "witness.git"]);
            keeps(steps, &beside.join("witness.git"), work);
        }
        Witness::WorktreeElsewhere => worktree_elsewhere(steps, &beside, &path, work),
        Witness::WorktreeOfHome => {
            let at = beside.join(WITNESS);
            let named = at.to_str().expect("a printable path").to_owned();
            steps.git(home, &["worktree", "add", "--quiet", "--detach", &named, work]);
        }
        Witness::ShallowAbove => shallow(steps, &beside, home, work, ABOVE),
        Witness::ShallowBelow => shallow(steps, &beside, home, work, TIP),
        Witness::BloblessPartial => blobless(steps, &beside, home, work),
        Witness::AlternatesOutside => {
            steps.git(&beside, &["clone", "--quiet", "--no-checkout", &path, "donor"]);
            let donor = beside.join("donor");
            keeps(steps, &donor, work);
            let named = donor.to_str().expect("a printable path").to_owned();
            steps.git(&beside, &["clone", "--quiet", "--shared", "--no-checkout", &named, WITNESS]);
            keeps(steps, &beside.join(WITNESS), work);
        }
        Witness::AlternatesIntoHome => {
            steps.git(&beside, &["clone", "--quiet", "--shared", "--no-checkout", &path, WITNESS]);
            keeps(steps, &beside.join(WITNESS), work);
        }
    }
}

/// A linked worktree of a clone of the home: a second repository, at a second path.
///
/// The worktree is checked out at the clone's own `HEAD` and not at the work. What holds the
/// work is the clone's object store, which the worktree shares, and a worktree that named a
/// commit its clone had not fetched could not be made at all.
fn worktree_elsewhere(steps: &mut Steps, beside: &Path, home: &str, work: &str) {
    steps.git(beside, &["clone", "--quiet", "--no-checkout", home, "donor"]);
    let donor = beside.join("donor");
    keeps(steps, &donor, work);
    let at = beside.join(WITNESS).to_str().expect("a printable path").to_owned();
    let start = steps.git(&donor, &["rev-parse", "HEAD"]);
    steps.git(&donor, &["worktree", "add", "--quiet", "--detach", &at, &start]);
}

/// A shallow clone that counts its depth from `name`, which is made in the home first.
///
/// [`TIP`] stands at the work, so the clone holds the work and not the history behind it.
/// [`ABOVE`] stands at a commit whose parent is the work, so the clone holds a commit and
/// not the work it stands on.
fn shallow(steps: &mut Steps, beside: &Path, home: &Path, work: &str, name: &str) {
    if name == ABOVE {
        let tree = steps.git(home, &["rev-parse", &format!("{work}^{{tree}}")]);
        let child =
            steps.git(home, &["commit-tree", &tree, "-p", work, "-m", "one commit above the work"]);
        steps.git(home, &["update-ref", name, &child]);
    } else {
        steps.git(home, &["update-ref", name, work]);
    }
    let short = name.strip_prefix("refs/heads/").unwrap_or(name).to_owned();
    let url = url(home);
    steps.git(
        beside,
        &["clone", "--quiet", "--depth", "1", "--branch", &short, "--no-checkout", &url, WITNESS],
    );
}

/// A `--filter=blob:none` clone: every commit, and none of the content.
///
/// The home has to be told to serve a filtered fetch, and the clone has to be given a URL
/// rather than a path: a clone from a path copies or links the whole object store and
/// ignores every filter, which would make this a full clone.
fn blobless(steps: &mut Steps, beside: &Path, home: &Path, work: &str) {
    steps.git(home, &["config", "uploadpack.allowFilter", "true"]);
    let url = url(home);
    steps.git(beside, &["clone", "--quiet", "--filter=blob:none", "--no-checkout", &url, WITNESS]);
    keeps(steps, &beside.join(WITNESS), work);
}

/// The branch a store puts on the work, so the work is under a ref it keeps of its own accord.
///
/// A clone puts what it fetched under `refs/remotes/origin/`, and that is the store's record of
/// a fetch: `git fetch --prune` deletes it the moment the remote drops the branch, so it is not
/// a copy (`crates/nodal-core/src/git/outside.rs`). A person's second clone of their own work
/// has a branch on it, because that is what they are working on.
///
/// So every clone-shaped topology names the work, and the store checks — partial, borrowed,
/// shallow, the home's own worktree — are then read against a store that would otherwise count.
/// The call may fail, and a failure is a shape: a clone fetches no stash, no note and nothing
/// under `refs/nodal/`, so there is nothing in it to name.
const KEPT: &str = "adversarial-copy";

/// Name the work in a store, where the store has it.
fn keeps(steps: &mut Steps, store: &Path, work: &str) {
    let _named = steps.try_git(store, &["branch", KEPT, work]);
}

/// A repository as a URL, so a clone of it uses the transport a filter and a depth need.
fn url(repo: &Path) -> String {
    let resolved = std::fs::canonicalize(repo).expect("the repository is there");
    format!("file://{}", resolved.display())
}

// ---------------------------------------------------------------------------
// What the checkout saw of the remote, and when.
// ---------------------------------------------------------------------------

/// Push the work and leave the checkout's reading of the remote as this axis names it.
fn observed(steps: &mut Steps, at: &Where, work: &str, shape: Observed) {
    let (checkout, origin, home) = (at.checkout.as_path(), at.origin.as_path(), at.home.as_path());
    let pushed = format!("{work}:refs/heads/{TOPIC}");
    match shape {
        Observed::NeverPushed => forget(steps, checkout),
        Observed::FetchedAfter => {
            steps.git(home, &["push", "--quiet", "origin", &pushed]);
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            dated(steps, checkout, 600);
        }
        Observed::FetchedBefore => {
            steps.git(home, &["push", "--quiet", "origin", &pushed]);
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            dated(steps, checkout, -600);
        }
        Observed::DroppedAfterFetch => {
            steps.git(home, &["push", "--quiet", "origin", &pushed]);
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            dated(steps, checkout, 600);
            steps.git(origin, &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
        }
        Observed::Pruned => {
            steps.git(home, &["push", "--quiet", "origin", &pushed]);
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            steps.git(origin, &["update-ref", "-d", &format!("refs/heads/{TOPIC}")]);
            steps.git(checkout, &["fetch", "--quiet", "--prune", "origin"]);
            dated(steps, checkout, 600);
        }
        Observed::ForcePushed => {
            steps.git(home, &["push", "--quiet", "origin", &pushed]);
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            steps.git(
                home,
                &["push", "--quiet", "--force", "origin", &format!("HEAD:refs/heads/{TOPIC}")],
            );
            steps.git(checkout, &["fetch", "--quiet", "origin"]);
            dated(steps, checkout, 600);
        }
    }
}

/// Leave the checkout with no record of a fetch and no reading of the remote.
///
/// Nothing is pushed in this arm. It is the shape in which the work is in the home and
/// nowhere else, so the witness axis alone decides the verdict, and it is how the FS-2 cases
/// state that a commit under a ref `HEAD` does not reach is work.
fn forget(steps: &mut Steps, checkout: &Path) {
    let record = checkout.join(".git/FETCH_HEAD");
    drop(std::fs::remove_file(&record));
    steps.note(format!("rm -f {}", record.display()));
    steps.git(checkout, &["update-ref", "-d", "refs/remotes/origin/main"]);
    steps.git(checkout, &["update-ref", "-d", "refs/remotes/origin/HEAD"]);
}

/// Date the checkout's record of its last fetch, relative to now.
///
/// Git writes `FETCH_HEAD` in whole seconds, and a shape that pushes and then fetches does
/// both inside one. A rule about the order of the two would then assert itself only when the
/// second happened to turn over between the commands, so the order is stated. The kit states
/// the same thing in one direction (`nodal_safety::git::fetched_later`); the axis needs both.
fn dated(steps: &mut Steps, checkout: &Path, seconds: i64) {
    let record = checkout.join(".git/FETCH_HEAD");
    let Ok(handle) = std::fs::File::options().write(true).open(&record) else { return };
    let now = std::time::SystemTime::now();
    let offset = std::time::Duration::from_secs(seconds.unsigned_abs());
    let when = if seconds < 0 { now - offset } else { now + offset };
    handle
        .set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("the record of the fetch is dated");
    steps.note(format!("touch -d '{seconds} seconds from now' {}", record.display()));
}

// ---------------------------------------------------------------------------
// What is running.
// ---------------------------------------------------------------------------

/// What one shape started against its home.
#[derive(Default)]
struct Started {
    /// Processes the kit owns and reclaims.
    held: Vec<Owned>,
    /// The one process this crate owns, when the shape has it.
    hidden: Option<Hidden>,
    /// The axis value this host would not make, and why.
    skipped: Option<String>,
}

/// Start what this axis names, and say so where the host will not make it.
fn occupant(steps: &mut Steps, home: &Path, shape: Occupant) -> Started {
    match shape {
        Occupant::Nothing => {
            steps.note(String::from("# nothing is started against the home"));
            Started::default()
        }
        Occupant::OwnProcess => {
            let unit = unit_id(home);
            steps.note(format!("NODAL_ID={unit} sleep 30 &"));
            Started {
                held: vec![nodal_safety::process::carrying(&unit, home)],
                ..Started::default()
            }
        }
        Occupant::UnrelatedCwd => {
            steps.note(format!("(cd {} && sleep 30 &)", home.display()));
            Started { held: vec![nodal_safety::process::standing_in(home)], ..Started::default() }
        }
        Occupant::WriteDescriptor => {
            let file = home.join(IGNORED);
            steps.note(format!("(cd / && sleep 30 >> {} &)", file.display()));
            Started { held: vec![nodal_safety::process::writing_into(&file)], ..Started::default() }
        }
        Occupant::Unreadable => unreadable(steps, home),
    }
}

/// A process of this account that this account may not read.
///
/// `prctl(PR_SET_DUMPABLE, 0)` is the one way to make one without a second account: the
/// kernel then owns every file under `/proc/<pid>` and refuses this account the working
/// directory, the environment and the descriptors. It has to be set in a child that does not
/// `exec`, because `execve` puts the attribute back.
///
/// The reading is then checked rather than assumed. A host that answered anyway is a host
/// the shape was not built on, and it says so instead of asserting nothing quietly.
#[cfg(target_os = "linux")]
fn unreadable(steps: &mut Steps, home: &Path) -> Started {
    let typed = nodal_safety::process::standing_in(home);
    steps.note(format!("(cd {} && sleep 30 &)", home.display()));
    let Some(hidden) = Hidden::forked_in(home, typed.pid()) else {
        let why = String::from("this host lets the account read a process that hid itself");
        return Started { held: vec![typed], skipped: Some(why), ..Started::default() };
    };
    steps.note(String::from("# and the job it started, with prctl(PR_SET_DUMPABLE, 0)"));
    Started { held: vec![typed], hidden: Some(hidden), skipped: None }
}

/// macOS publishes no such attribute and no such table, so the shape is not built there.
#[cfg(not(target_os = "linux"))]
fn unreadable(_steps: &mut Steps, _home: &Path) -> Started {
    let why = String::from("this host publishes no per-process table for a process to hide from");
    Started { skipped: Some(why), ..Started::default() }
}

/// A child of this process that this account may not read.
///
/// The kit owns every other process a fixture starts, and it cannot own this one. A child
/// that never `exec`s is a child of the test process, so nothing else can reap it: the kit's
/// reclaim signals the group and then waits for the process to leave the table, and a child
/// nobody waited for stays in it as a zombie for ever. So this type signals it and reaps it,
/// in that order, and it is the only process in this crate that is not the kit's.
#[cfg(target_os = "linux")]
struct Hidden {
    /// The child.
    pid: u32,
}

#[cfg(target_os = "linux")]
impl Hidden {
    /// Fork a child that hides itself, stands in `home`, and sleeps.
    ///
    /// `prctl(PR_SET_DUMPABLE, 0)` is the one way to make an unreadable process without a
    /// second account: the kernel then owns every file under `/proc/<pid>` and refuses this
    /// account the working directory, the environment and the descriptors. It has to be set
    /// in a child that does not `exec`, because `execve` puts the attribute back.
    ///
    /// The child joins `leader`'s process group, and that is what makes the shape the one FS-6
    /// is about. A withheld process refuses a move over the one thing still readable about it:
    /// its lineage. A hidden process descended from nothing near the home is a residual the
    /// report states and refuses nothing; a hidden process whose group leader stands in the
    /// home is the command a person typed there and the job it started, and that is occupancy.
    ///
    /// The child calls `prctl`, `chdir`, `nanosleep` and `_exit` and allocates nothing. Each
    /// of those is safe to call between a fork and an exec in a process that has threads, and
    /// this child never execs and never returns into Rust. The path is made into a C string
    /// before the fork for the same reason.
    fn forked_in(home: &Path, leader: u32) -> Option<Self> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;

        let at = CString::new(home.as_os_str().as_bytes()).ok()?;
        let group = i32::try_from(leader).ok()?;
        // SAFETY: the child below calls only functions that are safe between a fork and an
        // exec, and it ends with `_exit`, which runs no handler and no destructor.
        let forked = unsafe { libc::fork() };
        if forked == 0 {
            // SAFETY: as above.
            unsafe {
                libc::setpgid(0, group);
                libc::prctl(libc::PR_SET_DUMPABLE, 0);
                libc::chdir(at.as_ptr());
                let held = libc::timespec { tv_sec: 30, tv_nsec: 0 };
                libc::nanosleep(std::ptr::addr_of!(held), std::ptr::null_mut());
                libc::_exit(0);
            }
        }
        let pid = u32::try_from(forked).ok()?;
        let hidden = Self { pid };
        hidden.hid()?;
        Some(hidden)
    }

    /// Wait until this account really cannot read the child, and answer nothing where it
    /// still can.
    ///
    /// The attribute is set by the child, after the fork returned in the parent. So a reading
    /// taken the instant the fork returns is a reading of a child that has not hidden yet,
    /// and a shape built on it would assert the opposite of what it names. The wait is
    /// bounded: a host that never refuses the reading is a host the shape is not built on.
    fn hid(&self) -> Option<()> {
        let cwd = format!("/proc/{}/cwd", self.pid);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < until {
            if std::fs::read_link(&cwd).is_err() {
                return Some(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }
}

#[cfg(target_os = "linux")]
impl Drop for Hidden {
    /// Signal the child and reap it, so it leaves the table rather than staying in it.
    fn drop(&mut self) {
        let pid = i32::try_from(self.pid).unwrap_or_default();
        // SAFETY: both calls take a process identifier this type forked and nothing else
        // holds, and neither writes through a pointer.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Files.
// ---------------------------------------------------------------------------

/// Write a file in the home, making the directory above it.
fn write(home: &Path, at: &str, content: &str) {
    let path = home.join(at);
    if let Some(above) = path.parent() {
        std::fs::create_dir_all(above).expect("the directory the file goes in is made");
    }
    std::fs::write(&path, content).expect("the file is written");
}

/// Add a line to a file the fixture tracks.
fn append(home: &Path, at: &str, line: &str) {
    let path = home.join(at);
    let mut held = std::fs::read_to_string(&path).unwrap_or_default();
    held.push_str(line);
    std::fs::write(&path, held).expect("the file is written");
}
