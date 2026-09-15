//! Which tool versions the project pins.
//!
//! Two kinds of evidence, kept apart on purpose. A pin file is what a version manager
//! reads, so it is what a shell in the home will actually select. A manifest field is
//! what a package manager checks at install time; `engines` in `package.json`,
//! `rust-version` in `Cargo.toml`, `requires-python` in `pyproject.toml` and the `go`
//! directive in `go.mod` are all of that kind, and each one is recorded under a prefix
//! naming the file it came from, so that a mismatch between a manifest and a pin file
//! is visible rather than silently merged.

use crate::model::recipe::{Recipe, ToolName, ToolVersion};
use crate::recipe::gap::{Gap, GapKey};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Pin files whose whole content is the version, and the tool each one pins.
const PIN_FILES: &[(&str, &str)] = &[
    (".nvmrc", "node"),
    (".node-version", "node"),
    (".tool-versions", "asdf"),
    ("mise.toml", "mise"),
    (".mise.toml", "mise"),
    (".python-version", "python"),
];

/// The two spellings of the file `rustup` reads. Both may hold either a bare channel
/// or a `[toolchain]` table, so neither can be read as a plain pin file.
const RUST_TOOLCHAIN: &[&str] = &["rust-toolchain.toml", "rust-toolchain"];

/// The name a `rust-toolchain` channel is recorded under.
const RUST: &str = "rust";

/// Propose `toolchain`, or raise [`GapKey::Toolchain`] when the project pins nothing.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    let mut pin_files: Vec<(String, String)> = PIN_FILES
        .iter()
        .filter_map(|(file, tool)| Some(((*tool).to_owned(), project.read_pin(file)?)))
        .collect();
    if let Some(channel) = rust_channel(project) {
        pin_files.push((RUST.to_owned(), channel));
    }
    for (tool, version) in pin_files {
        if let Some(pin) = pin(&tool, &version) {
            proposal.recipe.toolchain.insert(pin.0, pin.1);
            proposal = proposal.sure("toolchain", Confidence::High);
        }
    }
    for (name, version) in manifest_pins(project) {
        if let Some(pin) = pin(&name, &version) {
            proposal.recipe.toolchain.insert(pin.0, pin.1);
            proposal.confidence.entry(String::from("toolchain")).or_insert(Confidence::Medium);
        }
    }
    if proposal.recipe.toolchain.is_empty() {
        proposal = proposal.gap(Gap::new(GapKey::Toolchain));
    }
    proposal
}

/// The channel `rustup` would select, from either spelling of its file.
///
/// The file is a `[toolchain]` table when it has one and a bare channel otherwise, and
/// both spellings accept both forms. Reading it as a plain pin file would record the
/// text `[toolchain]` as a version, because a table header is a line like any other.
fn rust_channel(project: &Project) -> Option<String> {
    let file = project.first_existing(RUST_TOOLCHAIN)?;
    let stated = project
        .read_toml(file)
        .and_then(|table| Some(table.get("toolchain")?.get("channel")?.as_str()?.to_owned()));
    // A bare channel is the whole of the file, and a table with no channel states no
    // version: the `[toolchain]` line the plain read would take is a header, not a pin.
    stated.or_else(|| project.read_pin(file).filter(|line| !line.starts_with('[')))
}

/// The version constraints the project's manifests state, by the name each is recorded
/// under.
///
/// A Cargo `rust-version`, a `requires-python` and a `go` directive are the same kind
/// of claim `engines` makes: what the tool checks, not what a version manager selects.
fn manifest_pins(project: &Project) -> Vec<(String, String)> {
    let mut pins: Vec<(String, String)> = project
        .engines()
        .into_iter()
        .map(|(tool, version)| (format!("engines.{tool}"), version))
        .collect();
    for (name, read) in MANIFEST_PINS {
        if let Some(version) = read(project) {
            pins.push(((*name).to_owned(), version));
        }
    }
    pins
}

/// One manifest constraint: the name it is recorded under, and how it is read.
type ManifestPin = (&'static str, fn(&Project) -> Option<String>);

/// Every manifest constraint that is not an `engines` row. The name carries the file,
/// so two manifests that disagree are two rows rather than one.
const MANIFEST_PINS: &[ManifestPin] = &[
    ("cargo.rust", rust_version),
    ("pyproject.python", requires_python),
    ("gomod.go", go_directive),
];

/// `rust-version` from `Cargo.toml`, from the crate's own table or the workspace's.
///
/// A workspace states it once in `workspace.package`, and a member crate in `package`,
/// so both are read and the crate's own answer wins.
fn rust_version(project: &Project) -> Option<String> {
    let manifest = project.read_toml("Cargo.toml")?;
    let read = |table: &str| {
        manifest
            .get(table)
            .and_then(|value| if table == "workspace" { value.get("package") } else { Some(value) })
            .and_then(|package| package.get("rust-version"))
            .and_then(|version| version.as_str())
            .map(str::to_owned)
    };
    read("package").or_else(|| read("workspace"))
}

/// `requires-python` from `pyproject.toml`, which every packaging backend reads.
fn requires_python(project: &Project) -> Option<String> {
    project
        .read_toml("pyproject.toml")?
        .get("project")?
        .get("requires-python")?
        .as_str()
        .map(str::to_owned)
}

/// The language version `go.mod` states.
///
/// `go.mod` is not TOML and has no parser here, so the one directive that is needed is
/// read as a line: `go <version>` at the start of a line, which is the only form the
/// file format allows.
fn go_directive(project: &Project) -> Option<String> {
    let text = project.read("go.mod")?;
    text.lines()
        .filter_map(|line| line.strip_prefix("go "))
        .map(str::trim)
        .find(|version| !version.is_empty())
        .map(str::to_owned)
}

/// A pin, when both halves have the shape their types require.
fn pin(tool: &str, version: &str) -> Option<(ToolName, ToolVersion)> {
    Some((ToolName::parse(tool).ok()?, ToolVersion::parse(version).ok()?))
}
