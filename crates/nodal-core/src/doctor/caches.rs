//! Build caches nothing has written to for a long time.
//!
//! Generated state is not a problem while a person is using it. A `target` directory of
//! a project built this morning is the reason the next build is fast. The same directory
//! in a branch abandoned in July is disk and nothing else, and a machine that has been
//! worked on for a year holds a lot of the second kind.
//!
//! So the rule is one word, `stale`, and one measurement: nothing under the directory
//! has been written for [`STALE_AFTER`]. The walk that measures the size reads the most
//! recent modification under the directory at the same time ([`super::size`]), so the
//! test costs nothing beyond the size the report needs anyway.
//!
//! The table below is the whole policy. A row is a directory name and the tool that
//! writes it. Adding a stack is adding a row.
//!
//! Doctor reports these. It does not remove them, and it is not the thing that decides
//! whether a unit home keeps one — that is [`crate::workspace::exclude`], a different
//! table with a different question.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::doctor::size;
use crate::model::Timestamp;
use crate::output::human;
use crate::output::view::doctor::{Finding, Kind};

/// How long a build cache goes unwritten before doctor calls it stale.
///
/// Two weeks. Long enough that a branch a person came back to last week is not on the
/// list; short enough that a machine's real leftovers are.
pub const STALE_AFTER: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// How deep under a checkout the walk looks for a cache directory.
///
/// A monorepo keeps one under each package, which is two levels; a worktree inside the
/// checkout adds two more. Past that a directory is inside something else's tree.
const DEPTH: usize = 6;

/// Directory names the walk never enters. They hold no cache of their own and reading
/// them costs more than everything else here together.
const NEVER: &[&str] = &[".git", "node_modules"];

/// One kind of build cache: the directory a tool writes, and which tool writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cache {
    /// The directory's name, matched exactly.
    pub name: &'static str,
    /// What writes it, in the words a report uses.
    pub tool: &'static str,
}

/// Every build cache doctor knows by name.
pub const CACHES: &[Cache] = &[
    Cache { name: "target", tool: "a Cargo build" },
    Cache { name: ".next", tool: "a Next.js build" },
    Cache { name: ".nuxt", tool: "a Nuxt build" },
    Cache { name: ".svelte-kit", tool: "a SvelteKit build" },
    Cache { name: ".turbo", tool: "a Turborepo task cache" },
    Cache { name: ".parcel-cache", tool: "a Parcel task cache" },
    Cache { name: ".vite", tool: "a Vite task cache" },
    Cache { name: "__pycache__", tool: "compiled Python modules" },
];

/// Every stale build cache under `root`, measured and dated from `now`.
#[must_use]
pub fn find(root: &Path, now: Timestamp) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue };
            if NEVER.contains(&name) {
                continue;
            }
            match CACHES.iter().find(|cache| cache.name == name) {
                Some(cache) => findings.extend(stale(root, &path, cache, now)),
                None if depth + 1 < DEPTH => queue.push((path, depth + 1)),
                None => {}
            }
        }
    }
    findings.sort_by(|left, right| left.what.cmp(&right.what));
    findings
}

/// One cache directory as a row, when nothing has written to it for [`STALE_AFTER`].
fn stale(root: &Path, path: &Path, cache: &Cache, now: Timestamp) -> Option<Finding> {
    let measured = size::measure(path);
    let written = seconds_of(measured.newest?)?;
    let age = now.unix_seconds().checked_sub(written)?;
    if age < i64::try_from(STALE_AFTER.as_secs()).unwrap_or(i64::MAX) {
        return None;
    }
    let name = path.strip_prefix(root).unwrap_or(path);
    let finding = Finding::new(Kind::StaleCache, name.display().to_string())
        .sized(measured.bytes, measured.complete)
        .says(cache.tool.to_owned());
    Some(match Timestamp::from_unix_seconds(written) {
        Ok(then) => finding.says(format!("written {}", human::since(now, then))),
        Err(_) => finding,
    })
}

/// An instant as whole seconds since the epoch, `None` when it is before it.
fn seconds_of(time: SystemTime) -> Option<i64> {
    i64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use super::{STALE_AFTER, find};
    use crate::model::Timestamp;
    use crate::output::view::doctor::Kind;

    /// A checkout with one cache last written `age` ago, and one written now.
    fn checkout(age: Duration) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        write(root.path(), "apps/web/.next/build.json", age);
        write(root.path(), ".turbo/log", Duration::ZERO);
        write(root.path(), "src/main.rs", age);
        root
    }

    fn write(root: &Path, relative: &str, age: Duration) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "x").unwrap();
        let when = SystemTime::now() - age;
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(when).unwrap();
    }

    fn now() -> Timestamp {
        Timestamp::now()
    }

    #[test]
    fn a_cache_nothing_has_written_to_for_a_fortnight_is_stale() {
        let root = checkout(STALE_AFTER + Duration::from_secs(60 * 60 * 24));
        let found = find(root.path(), now());
        let names: Vec<&str> = found.iter().map(|finding| finding.what.as_str()).collect();
        assert_eq!(names, ["apps/web/.next"], "only the one nothing has written to");
        assert_eq!(found[0].kind, Kind::StaleCache);
        assert_eq!(found[0].bytes, Some(1));
        assert!(found[0].state.iter().any(|word| word.contains("Next.js")), "{:?}", found[0].state);
    }

    #[test]
    fn a_cache_written_to_this_week_is_not_reported() {
        let root = checkout(Duration::from_secs(60 * 60 * 24));
        assert!(find(root.path(), now()).is_empty());
    }

    #[test]
    fn a_directory_that_holds_no_cache_reports_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert!(find(root.path(), now()).is_empty());
    }
}
