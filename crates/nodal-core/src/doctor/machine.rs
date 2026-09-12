//! `nodal doctor --machine`: every repository under the roots, grouped by remote.
//!
//! The survey writes nothing. It walks for `.git`, groups clones by the URL of
//! `origin`, and prints the facts a person needs to see whether a group is safe to
//! delete: unpushed commits, dirty clones, size, ignored directories, last commit age.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::Result;
use crate::doctor::origin::normalize;
use crate::doctor::scan::{self, Avoid};
use crate::doctor::unique::{Evidence, Subject};
use crate::doctor::{Registry, inspect, unique};
use crate::model::Timestamp;
use crate::output::view::doctor::Note;
use crate::output::view::machine::{CloneRow, Group, IgnoredDir, MachineReport, Skip, Walked};
use crate::store::environments;

/// How many directory levels the walk descends when the caller does not say.
pub const DEFAULT_DEPTH: usize = scan::DEFAULT_DEPTH;

/// What the machine survey reads.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Directories to walk. Each is resolved before it is entered.
    pub roots: &'a [PathBuf],
    /// Directory levels under each root.
    pub depth: usize,
    /// Nodal's state directory, which the walk does not enter.
    pub state_dir: &'a Path,
}

/// Read every repository under the roots and group them by remote.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn survey(
    registry: &Registry<'_>,
    request: &Request<'_>,
    now: Timestamp,
) -> Result<MachineReport> {
    let started = Instant::now();
    let (avoid, mut skipped) = avoid_of(registry, request)?;
    skipped.extend(registry.note().into_iter().map(note_as_skip));
    let roots: Vec<PathBuf> = request.roots.iter().map(|root| scan::resolve(root)).collect();
    let found = scan::walk(&roots, request.depth, &avoid);
    skipped.extend(found.skipped);
    let mut entries = found.entries;
    let mut rows = Vec::new();
    let mut links = inspect::Links::default();
    for path in found.repositories {
        match inspect::one(&path, &mut links) {
            Ok(inspected) => {
                entries += inspected.entries;
                rows.push((inspected.row, inspected.evidence));
            }
            Err(error) => skipped.push(Skip::new(&path, error.to_string())),
        }
    }
    let millis = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(MachineReport {
        now,
        roots,
        depth: request.depth,
        walked: Walked { entries, millis },
        skipped,
        groups: groups(rows),
    })
}

/// The directories the walk must not enter, and skips that are known before the walk.
fn avoid_of(registry: &Registry<'_>, request: &Request<'_>) -> Result<(Vec<Avoid>, Vec<Skip>)> {
    let mut avoid = vec![Avoid {
        path: scan::resolve(request.state_dir),
        why: String::from("nodal's state directory"),
    }];
    if let Some(conn) = registry.connection() {
        for environment in environments::list_all(conn)? {
            avoid.push(Avoid {
                path: scan::resolve(&environment.home),
                why: String::from("a unit's home"),
            });
        }
    }
    Ok((avoid, Vec::new()))
}

/// A registry note as a skip row, so the machine report has one list of reasons.
fn note_as_skip(note: Note) -> Skip {
    Skip::new(note.source, note.why)
}

/// Group clones by normalised origin URL. A clone with no remote is its own group.
fn groups(rows: Vec<(CloneRow, Evidence)>) -> Vec<Group> {
    let mut grouped: BTreeMap<String, Vec<(CloneRow, Evidence)>> = BTreeMap::new();
    for row in rows {
        grouped.entry(key_of(&row.0)).or_default().push(row);
    }
    let mut groups: Vec<Group> =
        grouped.into_iter().map(|(name, rows)| group(name, rows)).collect();
    groups.sort_by(|left, right| {
        right.bytes.cmp(&left.bytes).then_with(|| left.name.cmp(&right.name))
    });
    groups
}

/// The grouping key: origin's normalised URL, or the clone's path.
fn key_of(row: &CloneRow) -> String {
    row.origin.as_deref().map_or_else(|| row.path.display().to_string(), normalize)
}

/// Totals and the uniqueness verdict for one group.
///
/// The verdict is drawn here rather than in [`inspect`] because it needs the group: a
/// clone's commits are safe when another clone of the same remote holds them, and
/// whether a remote-tracking ref still means anything is decided by the freshest clone
/// of that remote on this machine. See [`unique`].
fn group(name: String, mut entries: Vec<(CloneRow, Evidence)>) -> Group {
    entries.sort_by(|left, right| {
        right.0.bytes.cmp(&left.0.bytes).then_with(|| left.0.path.cmp(&right.0.path))
    });
    let subjects: Vec<Subject> = entries
        .iter()
        .map(|(row, evidence)| Subject { path: row.path.clone(), evidence: evidence.clone() })
        .collect();
    for ((row, _), proof) in entries.iter_mut().zip(unique::prove(&subjects)) {
        row.unpushed = proof.off_remote;
        row.only_copy = proof.only_copy;
        row.unchecked = proof.unchecked;
        row.witnesses = proof.witnesses;
    }
    let repositories: Vec<CloneRow> = entries.into_iter().map(|(row, _)| row).collect();
    totals(name, repositories)
}

/// Fold the rows of one group into the line the table prints.
fn totals(name: String, repositories: Vec<CloneRow>) -> Group {
    let unpushed: usize = repositories.iter().filter_map(|row| row.unpushed).sum();
    let dirty = repositories.iter().filter(|row| row.dirty > 0).count();
    let unchecked = repositories.iter().filter(|row| row.unchecked.is_some()).count();
    let unwitnessed =
        repositories.iter().filter(|row| row.unpushed.is_none() && row.unchecked.is_none()).count();
    let unique_commits: usize = repositories.iter().filter_map(|row| row.only_copy).sum();
    let unique_clones =
        repositories.iter().filter(|row| row.only_copy.is_some_and(|only| only > 0)).count();
    Group {
        name,
        clones: repositories.len(),
        unpushed,
        unwitnessed,
        dirty,
        bytes: distinct_bytes(&repositories),
        ignored: fold_ignored(&repositories),
        committed: repositories.iter().filter_map(|row| row.committed).max(),
        nothing_unique: unchecked == 0 && unique_commits == 0 && dirty == 0,
        unique_clones,
        unique_commits,
        unchecked,
        repositories,
    }
}

/// The bytes the group holds, counting a file shared between clones once.
///
/// Cargo hardlinks one built file into every target directory that needs it, and
/// `git clone --local` hardlinks the object store. Adding the clones up counts those
/// files as many times as there are links to them, which is how a group of clones came
/// to be reported several gigabytes larger than the filesystem holds. Each row keeps the
/// apparent size of its own directory, which is what that directory holds; the group
/// takes off what a row had already been counted for somewhere else in this survey.
///
/// ## What the figure is, and how close it is
///
/// It is **apparent bytes with each file counted once**, which is what `du -c
/// --apparent-size` over the same paths reports. On the machine this was written for, a
/// group of 55 clones of one repository measured 67,526,393,859 bytes here against
/// 67,529,956,352 from `du`, a difference of 0.005%. The tolerance the figure is held to
/// is **1% of `du -c --apparent-size`**, and the gap that is left is rounding: `du`
/// counts in blocks of its own and this counts in bytes.
///
/// It is not what the filesystem allocated. The same group allocated 67,764,117,504
/// bytes, 0.4% more, because a file occupies whole blocks. A filesystem that compresses
/// or shares extents holds less than either figure, and neither number is a promise
/// about how much a disk gets back. The report says the size of what is there.
fn distinct_bytes(repositories: &[CloneRow]) -> u64 {
    let total: u64 = repositories.iter().map(|row| row.bytes).sum();
    let repeated: u64 = repositories.iter().map(|row| row.repeated).sum();
    total.saturating_sub(repeated)
}

/// The three largest ignored directories in the group, summed by relative path.
///
/// A file hardlinked into several clones is counted once here, as it is in the group's
/// size, so the two figures answer with the same bytes.
fn fold_ignored(rows: &[CloneRow]) -> Vec<IgnoredDir> {
    let mut sums: BTreeMap<String, u64> = BTreeMap::new();
    for row in rows {
        for dir in &row.ignored {
            let entry = sums.entry(dir.path.clone()).or_default();
            *entry = entry.saturating_add(dir.bytes).saturating_sub(dir.repeated);
        }
    }
    let mut dirs: Vec<IgnoredDir> =
        sums.into_iter().map(|(path, bytes)| IgnoredDir::new(path, bytes)).collect();
    dirs.sort_by(|left, right| {
        right.bytes.cmp(&left.bytes).then_with(|| left.path.cmp(&right.path))
    });
    dirs.truncate(3);
    dirs
}
