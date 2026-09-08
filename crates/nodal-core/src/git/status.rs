//! `git status --porcelain=v2` parsing into a summary the rest of Nodal can reason about.
//!
//! Dirtiness is computed from Git at the moment it matters and never stored, so this
//! parser is on the hot path of every uniqueness check.

use std::path::PathBuf;

use super::oid::Oid;
use crate::error::{Error, Result};

/// What HEAD points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// A branch, by short name.
    Branch(String),
    /// A commit, with no branch attached.
    Detached(Oid),
    /// A branch with no commits yet.
    Unborn(String),
}

/// How one path differs, on the index side or the worktree side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// No difference on this side.
    Unmodified,
    /// Added to the index.
    Added,
    /// Content or mode changed.
    Modified,
    /// Removed.
    Deleted,
    /// Renamed from another path.
    Renamed,
    /// Copied from another path.
    Copied,
    /// The type of the entry changed (file, symlink, submodule).
    TypeChanged,
    /// A state this version of Nodal does not model.
    Other(char),
}

impl Change {
    /// Map one status character of the `XY` field, or of a `--name-status` record.
    #[must_use]
    pub fn parse(code: char) -> Self {
        match code {
            '.' => Self::Unmodified,
            'A' => Self::Added,
            'M' => Self::Modified,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            'T' => Self::TypeChanged,
            other => Self::Other(other),
        }
    }
}

/// The status of one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// A tracked path that differs from HEAD or from the index.
    Tracked {
        /// How the index differs from HEAD.
        index: Change,
        /// How the worktree differs from the index.
        worktree: Change,
    },
    /// A path with unresolved merge stages.
    Unmerged,
    /// A path Git does not track and no ignore rule covers.
    Untracked,
    /// A path an ignore rule covers.
    Ignored,
}

/// One entry of a status listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the repository root.
    pub path: PathBuf,
    /// The path this one was renamed or copied from.
    pub origin: Option<PathBuf>,
    /// What is different about it.
    pub state: State,
}

impl Entry {
    /// Whether this entry represents work that a commit would capture.
    #[must_use]
    pub fn is_uncommitted(&self) -> bool {
        !matches!(self.state, State::Ignored)
    }
}

/// A whole `git status` reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// What HEAD points at.
    pub head: Head,
    /// The upstream branch, when one is configured.
    pub upstream: Option<String>,
    /// Commits on HEAD that the upstream does not have.
    pub ahead: u32,
    /// Commits on the upstream that HEAD does not have.
    pub behind: u32,
    /// One entry per changed, unmerged, untracked or ignored path.
    pub entries: Vec<Entry>,
}

impl Summary {
    /// Whether nothing at all differs, ignored paths aside.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.entries.iter().any(Entry::is_uncommitted)
    }

    /// Paths that carry work a commit would capture, ignored paths aside.
    pub fn uncommitted(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|entry| entry.is_uncommitted())
    }

    /// Whether any path has unresolved merge stages.
    #[must_use]
    pub fn has_conflicts(&self) -> bool {
        self.entries.iter().any(|entry| entry.state == State::Unmerged)
    }
}

/// The header fields collected while parsing, before they become a [`Summary`].
#[derive(Default)]
struct Headers {
    oid: Option<String>,
    branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
}

impl Headers {
    /// Absorb one `# key value` header line.
    fn absorb(&mut self, record: &str) {
        let Some((key, value)) = record.trim_start_matches("# ").split_once(' ') else {
            return;
        };
        match key {
            "branch.oid" => self.oid = Some(value.to_owned()),
            "branch.head" => self.branch = Some(value.to_owned()),
            "branch.upstream" => self.upstream = Some(value.to_owned()),
            "branch.ab" => self.absorb_ab(value),
            _ => {}
        }
    }

    /// Absorb the `+N -M` ahead/behind counts.
    fn absorb_ab(&mut self, value: &str) {
        let mut counts = value.split(' ').filter_map(|count| {
            let (sign, digits) = count.split_at(1);
            digits.parse::<u32>().ok().map(|number| (sign, number))
        });
        for (sign, number) in &mut counts {
            match sign {
                "+" => self.ahead = number,
                "-" => self.behind = number,
                _ => {}
            }
        }
    }

    /// Resolve the headers into a [`Head`].
    fn head(&self, args: &[String]) -> Result<Head> {
        let branch = self.branch.as_deref().unwrap_or("(detached)");
        let oid = self.oid.as_deref().unwrap_or("(initial)");
        match (branch, oid) {
            ("(detached)", "(initial)") => {
                Err(Error::GitParse { args: args.to_vec(), record: "# branch.head".to_owned() })
            }
            ("(detached)", commit) => Ok(Head::Detached(Oid::parse(commit)?)),
            (name, "(initial)") => Ok(Head::Unborn(name.to_owned())),
            (name, _) => Ok(Head::Branch(name.to_owned())),
        }
    }
}

/// Parse the NUL-separated records of `git status --porcelain=v2 --branch -z`.
///
/// # Errors
/// [`Error::GitParse`] when a record does not have the documented shape, or when the
/// header lines do not identify HEAD.
pub(super) fn parse(args: &[String], records: &[&str]) -> Result<Summary> {
    let mut headers = Headers::default();
    let mut entries = Vec::new();
    let mut rest = records.iter().copied();
    while let Some(record) = rest.next() {
        let (code, body) = record.split_at(1);
        match code {
            "#" => headers.absorb(record),
            "1" => entries.push(ordinary(args, body.trim_start())?),
            "2" => entries.push(renamed(args, body.trim_start(), rest.next())?),
            "u" => entries.push(unmerged(args, body.trim_start())?),
            "?" => entries.push(plain(body.trim_start(), State::Untracked)),
            "!" => entries.push(plain(body.trim_start(), State::Ignored)),
            _ => {
                return Err(Error::GitParse { args: args.to_vec(), record: record.to_owned() });
            }
        }
    }
    Ok(Summary {
        head: headers.head(args)?,
        upstream: headers.upstream.clone(),
        ahead: headers.ahead,
        behind: headers.behind,
        entries,
    })
}

/// Split the `XY <fields…> <path>` tail of a v2 record into its `XY` field and the path.
fn split_fields(args: &[String], body: &str, fields: usize) -> Result<(State, PathBuf)> {
    let malformed = || Error::GitParse { args: args.to_vec(), record: body.to_owned() };
    let parts: Vec<&str> = body.splitn(fields + 1, ' ').collect();
    let [codes, .., path] = parts.as_slice() else { return Err(malformed()) };
    if parts.len() != fields + 1 || path.is_empty() {
        return Err(malformed());
    }
    let mut chars = codes.chars();
    let (Some(index), Some(worktree), None) = (chars.next(), chars.next(), chars.next()) else {
        return Err(malformed());
    };
    let state = State::Tracked { index: Change::parse(index), worktree: Change::parse(worktree) };
    Ok((state, PathBuf::from(path)))
}

/// `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`
fn ordinary(args: &[String], body: &str) -> Result<Entry> {
    let (state, path) = split_fields(args, body, 7)?;
    Ok(Entry { path, origin: None, state })
}

/// `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <Xscore> <path>` with the origin path next.
fn renamed(args: &[String], body: &str, origin: Option<&str>) -> Result<Entry> {
    let (state, path) = split_fields(args, body, 8)?;
    let origin =
        origin.ok_or_else(|| Error::GitParse { args: args.to_vec(), record: body.to_owned() })?;
    Ok(Entry { path, origin: Some(PathBuf::from(origin)), state })
}

/// `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`
fn unmerged(args: &[String], body: &str) -> Result<Entry> {
    let (_, path) = split_fields(args, body, 9)?;
    Ok(Entry { path, origin: None, state: State::Unmerged })
}

/// `? <path>` and `! <path>`
fn plain(body: &str, state: State) -> Entry {
    Entry { path: PathBuf::from(body), origin: None, state }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;

    use super::{Change, Head, State, parse};

    const OID: &str = "1e2f3a4b5c6d7e8f90112233445566778899aabb";

    #[test]
    fn reads_branch_upstream_and_divergence() {
        let records = [
            format!("# branch.oid {OID}"),
            "# branch.head feature/x".to_owned(),
            "# branch.upstream origin/feature/x".to_owned(),
            "# branch.ab +3 -4".to_owned(),
        ];
        let borrowed: Vec<&str> = records.iter().map(String::as_str).collect();
        let summary = parse(&[], &borrowed).unwrap();
        assert_eq!(summary.head, Head::Branch("feature/x".to_owned()));
        assert_eq!(summary.upstream.as_deref(), Some("origin/feature/x"));
        assert_eq!((summary.ahead, summary.behind), (3, 4));
        assert!(summary.is_clean());
    }

    #[test]
    fn reads_detached_and_unborn_heads() {
        let detached = [format!("# branch.oid {OID}"), "# branch.head (detached)".to_owned()];
        let borrowed: Vec<&str> = detached.iter().map(String::as_str).collect();
        assert!(matches!(parse(&[], &borrowed).unwrap().head, Head::Detached(_)));

        let unborn = ["# branch.oid (initial)", "# branch.head main"];
        assert_eq!(parse(&[], &unborn).unwrap().head, Head::Unborn("main".to_owned()));
    }

    #[test]
    fn reads_every_entry_shape_including_renames_with_spaces() {
        let records = [
            format!("# branch.oid {OID}"),
            "# branch.head main".to_owned(),
            format!("1 .M N... 100644 100644 100644 {OID} {OID} src/a.rs"),
            format!("2 R. N... 100644 100644 100644 {OID} {OID} R100 to b.rs"),
            "from a.rs".to_owned(),
            format!("u UU N... 100644 100644 100644 100644 {OID} {OID} {OID} c.rs"),
            "? untracked d.rs".to_owned(),
            "! node_modules/".to_owned(),
        ];
        let borrowed: Vec<&str> = records.iter().map(String::as_str).collect();
        let summary = parse(&[], &borrowed).unwrap();
        assert_eq!(summary.entries.len(), 5);
        assert_eq!(
            summary.entries[0].state,
            State::Tracked { index: Change::Unmodified, worktree: Change::Modified }
        );
        assert_eq!(summary.entries[1].path, Path::new("to b.rs"));
        assert_eq!(summary.entries[1].origin.as_deref(), Some(Path::new("from a.rs")));
        assert_eq!(summary.entries[2].state, State::Unmerged);
        assert_eq!(summary.entries[3].path, Path::new("untracked d.rs"));
        assert_eq!(summary.entries[4].state, State::Ignored);
        assert!(summary.has_conflicts());
        assert!(!summary.is_clean());
        assert_eq!(summary.uncommitted().count(), 4);
    }

    #[test]
    fn rejects_unknown_and_truncated_records() {
        assert!(parse(&[], &["x nonsense"]).is_err());
        assert!(parse(&[], &["1 .M N... 100644"]).is_err());
        assert!(parse(&[], &["# branch.head (detached)"]).is_err());
    }
}
