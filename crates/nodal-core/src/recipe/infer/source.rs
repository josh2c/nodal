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
