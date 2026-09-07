//! The one place inference touches the filesystem.
//!
//! Every inference source reads the project through [`Project`], so the whole engine
//! has a single IO seam: a test builds a project on disk once and every source is
//! exercised against it, and no source can reach outside the root it was given.
//!
//! Reading is deliberately tolerant. A candidate file that is missing, unreadable or
//! malformed is treated as absent, because inference is a proposal, not a validation
//! pass; the file a person wrote is the one that is parsed strictly
//! ([`crate::recipe::parse`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::git::Git;

/// The revision [`Project::tracked`] asks about: what the project is checked out at.
const HEAD: &str = "HEAD";

/// Pathspec magic that makes Git read a path as characters and not as a pattern.
const LITERAL: &str = ":(literal)";

/// A project root, with the manifests inference reads more than once already parsed.
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    package_json: Value,
}

impl Project {
    /// Open `root` for inference. Absent or unreadable manifests are treated as absent,
    /// so this cannot fail on a project that simply does not have them.
    #[must_use]
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let package_json = read_json(&root, "package.json").unwrap_or(Value::Null);
        Self { root, package_json }
    }

    /// The project root every relative path in a recipe is resolved against.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether `relative` exists in the project.
    #[must_use]
    pub fn exists(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }

    /// The first of `candidates` that exists.
    #[must_use]
    pub fn first_existing<'a>(&self, candidates: &[&'a str]) -> Option<&'a str> {
        candidates.iter().copied().find(|candidate| self.exists(candidate))
    }

    /// Every one of `candidates` that exists, in the order given.
    #[must_use]
    pub fn all_existing<'a>(&self, candidates: &[&'a str]) -> Vec<&'a str> {
        candidates.iter().copied().filter(|candidate| self.exists(candidate)).collect()
    }

    /// Which of `candidates` the project's checked-out commit tracks.
    ///
    /// This is the one question inference asks Git. A directory the project tracks is
    /// the project's own content, whatever its name says, so no source may propose that
    /// a copy leaves it out; [`crate::workspace::tracked`] is the gate that enforces
    /// the same rule at the copy itself.
    ///
    /// Reading is tolerant here as everywhere in this type. A root that is not a
    /// repository, and one with no commit yet, both track nothing.
    #[must_use]
    pub fn tracked<'a>(&self, candidates: &[&'a str]) -> Vec<&'a str> {
        if candidates.is_empty() {
            return Vec::new();
        }
        let entries = self.ls_tree(candidates).unwrap_or_default();
        candidates
            .iter()
            .copied()
            .filter(|candidate| {
                entries.iter().any(|entry| entry.path.starts_with(Path::new(candidate)))
            })
            .collect()
    }

    /// The tree entries of `HEAD` under `candidates`, or `None` when Git cannot answer.
    fn ls_tree(&self, candidates: &[&str]) -> Option<Vec<crate::git::tree::Entry>> {
        let git = Git::open(&self.root).ok()?;
        git.rev_parse_opt(HEAD).ok()??;
        let pathspecs: Vec<String> =
            candidates.iter().map(|path| format!("{LITERAL}{path}")).collect();
        let borrowed: Vec<&str> = pathspecs.iter().map(String::as_str).collect();
        match git.ls_tree(HEAD, false, &borrowed) {
            Ok(entries) => Some(entries),
            Err(error) => {
                tracing::debug!(%error, "recipe inference: the tracked paths were not read");
                None
            }
        }
    }

    /// The text of `relative`, or `None` when it is absent or not readable as UTF-8.
    #[must_use]
    pub fn read(&self, relative: &str) -> Option<String> {
        let path = self.root.join(relative);
        match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "recipe inference: not read");
                None
            }
        }
    }

    /// The first non-empty line of `relative`, trimmed: the shape of every pin file.
    #[must_use]
    pub fn read_pin(&self, relative: &str) -> Option<String> {
        let text = self.read(relative)?;
        text.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
    }

    /// `relative` parsed as TOML, or `None` when it is absent or malformed.
    #[must_use]
    pub fn read_toml(&self, relative: &str) -> Option<toml::Value> {
        let text = self.read(relative)?;
        match toml::from_str(&text) {
            Ok(value) => Some(value),
            Err(error) => {
                tracing::debug!(file = relative, %error, "recipe inference: not valid TOML");
                None
            }
        }
    }

    /// The project's `package.json`, or [`Value::Null`] when it has none.
    #[must_use]
    pub fn package_json(&self) -> &Value {
        &self.package_json
    }

    /// The `scripts` table of `package.json`, by name.
    #[must_use]
    pub fn scripts(&self) -> BTreeMap<String, String> {
        string_table(self.package_json.get("scripts"))
    }

    /// The `engines` table of `package.json`, by tool.
    #[must_use]
    pub fn engines(&self) -> BTreeMap<String, String> {
        string_table(self.package_json.get("engines"))
    }
}

/// A JSON object flattened to the string-valued entries it has, ignoring the rest.
fn string_table(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|text| (key.clone(), text.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_json(root: &Path, relative: &str) -> Option<Value> {
    let text = std::fs::read_to_string(root.join(relative)).ok()?;
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::debug!(file = relative, %error, "recipe inference: not valid JSON");
            None
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::Project;

    /// A project with `keep` committed and `ignored` present but not committed.
    fn project(keep: &str, ignored: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for path in [keep, ignored] {
            let file = root.path().join(path).join("report.xml");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "<r/>").unwrap();
        }
        git(root.path(), &["init", "--quiet", "."]);
        git(root.path(), &["add", "--", keep]);
        git(
            root.path(),
            &[
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "user.name=test",
                "commit",
                "--quiet",
                "--message=fixture",
            ],
        );
        root
    }

    fn git(dir: &Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(dir).args(args).status().unwrap().success());
    }

    #[test]
    fn only_the_directory_the_commit_holds_is_reported_as_tracked() {
        let root = project("coverage", "test-results");
        let project = Project::open(root.path());
        assert_eq!(project.tracked(&["coverage", "test-results"]), ["coverage"]);
    }

    #[test]
    fn a_root_that_is_not_a_repository_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("coverage")).unwrap();
        let project = Project::open(root.path());
        assert!(project.tracked(&["coverage"]).is_empty());
        assert!(project.tracked(&[]).is_empty());
    }

    #[test]
    fn a_repository_with_no_commit_yet_tracks_nothing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("coverage")).unwrap();
        git(root.path(), &["init", "--quiet", "."]);
        let project = Project::open(root.path());
        assert!(project.tracked(&["coverage"]).is_empty());
    }
}
