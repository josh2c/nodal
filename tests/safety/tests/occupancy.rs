//! Occupancy: what it means for something to be using a home, and what a reclaim refuses
//! over.
//!
//! A reclaim of a managed home **renames the directory**. Everything holding a path inside
//! it goes on holding the inode and never learns that it moved, so the question the
//! refusal rests on is not "is anybody standing in this directory" — it is "would anything
//! keep writing into it afterwards". Those are different questions and Nodal used to ask
//! only the first.
//!
//! The shape that made the difference matters, because it is completely ordinary: a test
//! runner or a dev server started from a terminal that has since changed directory,
//! writing its SQLite file into a git-ignored corner of the home. Its working directory is
//! `/`. It carries no `NODAL_ID`. The file it writes is ignored, so the content rule does
//! not catch it either. The verdict was `safe`, with no bystander and no note; the home
//! was renamed into the trash under the writer, the ignored file was dropped as
//! reconstructable, and the writer went on writing into nothing.
//!
//! So occupancy here is `cwd ∪ fd(write) ∪ mmap(write, shared) ∪ root`, which is what
//! `lsof +D` and `fuser -m` have meant by it for thirty years.
//!
//! **And the read-only half is what makes that affordable.** An editor, a language server,
//! a `tail` and a `grep` hold descriptors on files inside a home all day. A rule that
//! refused over those would refuse every reclaim on a working machine, and a rule nobody
//! can satisfy is a rule people turn off. `/proc/<pid>/fdinfo/<n>` carries the open flags,
//! so a descriptor opened for reading refuses nothing. Both halves are asserted here; the
//! second is not a nicety, it is the reason the first can ship.
//!
//! | property | test |
//! |---|---|
//! | a write descriptor into an ignored file refuses the move | `a_write_descriptor_inside_the_home_refuses_the_move` |
//! | and the reclaim refuses the same way, naming the path | `a_write_descriptor_inside_the_home_refuses_the_move` |
//! | a read-only descriptor refuses nothing | `a_read_only_descriptor_is_not_occupancy` |
//! | a writable shared mapping is occupancy | `a_writable_shared_mapping_is_read_as_a_hold` |
//! | the predicate itself, on a table a test states | `the_predicate_counts_writes_and_ignores_reads_on_every_host` |
//!
//! **Hosts.** The predicate is a function of the table
//! ([`nodal_core::lifecycle::assess::sort`]), so the last test above states a table and
//! runs on both runners. The *filling* of that table is `/proc`, and macOS publishes no
//! per-descriptor open flags for a vnode — the read/write split that makes the rule
//! affordable is not available there — so the three live tests name macOS and skip it,
//! and [`nodal_core::runtime::processes::OCCUPANCY`] says on every host which readings it
//! answered. FS-8 is closed on Linux and stated-open on macOS rather than guessed at.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nodal_core::lifecycle::assess::{Own, sort};
use nodal_core::model::UnitId;
use nodal_core::runtime::processes::{Held, How, Live, Processes, Running};
use nodal_safety::{InState as _, Machine, answer, platform, stderr};
use serde_json::Value;

/// The unit every property here asks about. One of the fixture's own handles.
const SLUG: &str = "worker-import";

/// A path inside the home that the fixture's `.gitignore` covers.
///
/// Ignored on purpose, and it is the whole point of the case. An untracked file that is
/// *not* ignored is already caught, by the content rule and not by the process rule
/// (`unique_loss`), so a test written against one would pass while the process rule went
/// on missing everything. This file is invisible to every other conjunct.
const IGNORED: &str = "dist/dev.sqlite";

/// The machine every property here runs on.
fn machine() -> Machine {
    Machine::with_remote()
}

/// The preflight for one unit.
fn check(machine: &Machine, slug: &str) -> Value {
    let asked = machine.nodal(&["reclaim", slug, "--check", "--json"]);
    let printed = answer(&asked);
    let read: Value = serde_json::from_str(&printed)
        .unwrap_or_else(|_| panic!("--check --json is one document: {printed}{}", stderr(&asked)));
    assert_eq!(
        read["safe_to_reclaim"],
        Value::Bool(asked.status.success()),
        "the exit code and the verdict disagree: {printed}"
    );
    read
}

/// Why the live half of this file does not run on macOS.
fn only_linux(claim: &str) -> bool {
    cfg!(target_os = "macos")
        && platform::skipped(
            claim,
            "macos publishes no open flags for a vnode descriptor, so a read-only hold \
             cannot be told from a writing one and occupancy there is the working \
             directory alone (processes::OCCUPANCY says so)",
        )
}

/// A process writing a file inside the home occupies it, wherever it is standing.
///
/// The severe form of FS-8, reproduced as E4 on the released binary: working directory
/// `/`, no `NODAL_ID`, a descriptor open for writing on a git-ignored file two directories
/// inside the home. Every other conjunct is silent about it. The verdict was `safe`.
///
/// The refusal has to **name the path**, and that is not presentation. A person told that
/// process 4711 is standing in the home will go and look in the home and find nothing:
/// the process is standing in `/`. The path is the only thing that leads them to it.
#[test]
fn a_write_descriptor_inside_the_home_refuses_the_move() {
    if only_linux("a write descriptor inside the home refuses the move") {
        return;
    }
    let machine = machine();
    let home = machine.unit(SLUG);
    let writer = nodal_safety::process::writing_into(&home.join(IGNORED));

    let answer = check(&machine, SLUG);
    assert_eq!(
        answer["safe_to_reclaim"],
        Value::Bool(false),
        "a process writing into the home did not block the move: {answer:#}"
    );
    assert_eq!(answer["reasons"][0]["needs"], Value::from("blocking_runtime"), "{answer:#}");
    let standing = answer["runtime"]["bystanders"].as_array().unwrap();
    let named = standing
        .iter()
        .find(|row| row["pid"] == writer.pid())
        .unwrap_or_else(|| panic!("the writer is not named: {answer:#}"));
    assert!(
        named["holding"].as_str().is_some_and(|held| held.contains(IGNORED)),
        "the row does not say what it holds, so nobody can find it: {named:#}"
    );
    assert!(nodal_safety::process::alive(writer.pid()), "the check signalled the writer");

    let refused = machine.nodal(&["reclaim", SLUG]);
    assert!(!refused.status.success(), "the reclaim moved the home out from under a writer");
    let told = stderr(&refused);
    assert!(told.contains(&writer.pid().to_string()), "the refusal does not name it: {told}");
    assert!(told.contains(IGNORED), "the refusal does not say what it holds: {told}");
    assert!(home.is_dir(), "the refusal moved the home");
    assert!(machine.trashed().is_empty(), "the refusal trashed the home");
    assert!(nodal_safety::process::alive(writer.pid()), "the refusal signalled the writer");
}

/// A process **reading** a file inside the home does not refuse anything.
///
/// The control, and the reason the rule above is affordable rather than merely correct. An
/// editor with the file open, a language server indexing it, a `tail -f` on a log: all of
/// them hold a descriptor on a path inside a home and none of them writes through it, so a
/// rename costs them nothing a person would miss.
///
/// The claim is asserted on both hosts. On macOS no descriptor is read at all, so the
/// verdict is safe there for a weaker reason than it is on Linux — which is exactly what
/// the evidence record is for, and `evidence_record.rs` holds it.
#[test]
fn a_read_only_descriptor_is_not_occupancy() {
    let machine = machine();
    let home = machine.unit(SLUG);
    std::fs::create_dir_all(home.join(IGNORED).parent().unwrap()).unwrap();
    std::fs::write(home.join(IGNORED), b"state\n").unwrap();
    let reader = nodal_safety::process::reading_from(&home.join(IGNORED));

    let answer = check(&machine, SLUG);
    assert_eq!(
        answer["safe_to_reclaim"],
        Value::Bool(true),
        "a process merely reading a file in the home refused a reclaim: {answer:#}"
    );
    assert!(
        answer["runtime"]["bystanders"].as_array().unwrap().is_empty(),
        "a reader was named as standing in the home: {answer:#}"
    );
    assert!(nodal_safety::process::alive(reader.pid()), "the check signalled the reader");
}

/// A file mapped writably and shared is a hold on it, and the scan reads it.
///
/// The other half of `fd ∪ mmap`, and the shape a memory-mapped database has: no write
/// descriptor is kept open at all, the process writes through memory, and the file changes
/// underneath anyone who moves it. This test maps the file itself rather than starting a
/// helper, because mapping is what has to be observed and the test process is a process
/// the scan reads like any other.
///
/// **Writable and shared, not writable alone.** A private writable mapping is
/// copy-on-write and the file never changes through it, which is what every loaded
/// library's data segment is on every process on the machine; counting those would make
/// every home look occupied by everything. The assertion below is that the shared mapping
/// is seen, and `the_predicate_counts_writes_and_ignores_reads_on_every_host` holds the
/// rest.
#[test]
fn a_writable_shared_mapping_is_read_as_a_hold() {
    if only_linux("a writable shared mapping is read as a hold") {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("mapped.db");
    std::fs::write(&file, vec![0_u8; 4096]).unwrap();
    let mapped = Mapping::of(&file);

    let table = Processes::scan(&Live).unwrap();
    let ours = table
        .iter()
        .find(|process| process.pid == std::process::id())
        .unwrap_or_else(|| panic!("this process is not in the table it is reading"));
    assert!(
        ours.held.iter().any(|held| held.how == How::Mapping && held.path == file),
        "the writable shared mapping was not read as a hold: {:?}",
        ours.held
    );
    drop(mapped);
}

/// One writable shared mapping, unmapped when the test is done with it.
struct Mapping {
    /// Where the kernel put it.
    at: *mut libc::c_void,
    /// How long it is.
    length: usize,
}

impl Mapping {
    /// Map the whole of `file`, writably and shared.
    fn of(file: &Path) -> Self {
        let length = usize::try_from(std::fs::metadata(file).unwrap().len()).unwrap();
        let handle = std::fs::OpenOptions::new().read(true).write(true).open(file).unwrap();
        // SAFETY: the length is the file's own, the descriptor is open for reading and
        // writing, and the kernel chooses the address. A failure answers `MAP_FAILED`,
        // which is checked below.
        let at = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::fd::AsRawFd::as_raw_fd(&handle),
                0,
            )
        };
        assert!(at != libc::MAP_FAILED, "the file could not be mapped");
        Self { at, length }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: the address and the length are the ones `mmap` answered with, and
        // nothing else holds the mapping.
        unsafe { libc::munmap(self.at, self.length) };
    }
}

/// The rule itself, over a process table a test states, on every host.
///
/// The three tests above need a machine to hold a real process, and two of them therefore
/// run on one host. This one is the predicate — which is a function of a table
/// ([`sort`]) and of nothing else — so it runs everywhere and pins the arm that
/// matters on a runner that cannot make the machine hold the shape.
///
/// Four processes and one home. A writer whose directory is elsewhere is standing in the
/// home. A reader is not — the scan never keeps a read-only descriptor, so the reader's
/// table row has nothing in it, and that absence *is* the rule. A process mapping the home
/// writably is standing in it. A process holding something under a *different* directory
/// is not, which is what keeps the prefix comparison honest.
#[test]
fn the_predicate_counts_writes_and_ignores_reads_on_every_host() {
    let home = PathBuf::from("/homes/one");
    let elsewhere = PathBuf::from("/homes/two");
    let unit = UnitId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let table = vec![
        stranger(11, "writer")
            .holding(vec![Held::new(home.join("dist/dev.sqlite"), How::Descriptor)]),
        stranger(12, "reader"),
        stranger(13, "mapper").holding(vec![Held::new(home.join("var/store.db"), How::Mapping)]),
        stranger(14, "neighbour")
            .holding(vec![Held::new(elsewhere.join("dist/dev.sqlite"), How::Descriptor)]),
    ];

    let (certain, standing) = sort(&table, Own::of(unit, &[]), std::slice::from_ref(&home), &[]);
    assert!(certain.is_empty(), "nothing here carries the unit's identifier: {certain:?}");
    let blocked: Vec<u32> = standing.iter().map(|row| row.pid).collect();
    assert_eq!(blocked, [11, 13], "the rule is writes and mappings inside this home: {standing:?}");
    for row in &standing {
        assert!(
            row.holding.as_deref().is_some_and(|held| held.contains("/homes/one/")),
            "a row that blocks over a held path must name it: {row:?}"
        );
    }
}

/// A process carrying no Nodal variable, standing nowhere, with a name.
fn stranger(pid: u32, command: &str) -> Running {
    Running::new(pid, BTreeMap::new()).running(command)
}
