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
use crate::doctor::{Registry, inspect};
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
    for path in found.repositories {
        match inspect::one(&path) {
            Ok(inspected) => {
                entries += inspected.entries;
                rows.push(inspected.row);
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
fn groups(rows: Vec<CloneRow>) -> Vec<Group> {
    let mut grouped: BTreeMap<String, Vec<CloneRow>> = BTreeMap::new();
    for row in rows {
        grouped.entry(key_of(&row)).or_default().push(row);
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

/// Totals and the "nothing unique" signal for one group.
fn group(name: String, mut repositories: Vec<CloneRow>) -> Group {
    repositories.sort_by(|left, right| {
        right.bytes.cmp(&left.bytes).then_with(|| left.path.cmp(&right.path))
    });
    let unpushed: usize = repositories.iter().map(|row| row.unpushed).sum();
    let dirty = repositories.iter().filter(|row| row.dirty > 0).count();
    let bytes: u64 = repositories.iter().map(|row| row.bytes).sum();
    let nothing_unique = repositories.iter().all(|row| row.unpushed == 0 && row.dirty == 0);
    let committed = repositories.iter().filter_map(|row| row.committed).max();
    let ignored = fold_ignored(&repositories);
    Group {
        name,
        clones: repositories.len(),
        unpushed,
        dirty,
        bytes,
        ignored,
        committed,
        nothing_unique,
        repositories,
    }
}

/// The three largest ignored directories in the group, summed by relative path.
fn fold_ignored(rows: &[CloneRow]) -> Vec<IgnoredDir> {
    let mut sums: BTreeMap<String, u64> = BTreeMap::new();
    for row in rows {
        for dir in &row.ignored {
            *sums.entry(dir.path.clone()).or_default() += dir.bytes;
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
