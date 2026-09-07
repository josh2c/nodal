//! Attribution: what is running on this machine, and which unit it belongs to.
//!
//! Two units of one project run at the same time, on one host, out of two homes. A port
//! is bound, a container is up, a dev server is burning a core. The question a person
//! asks is not "what is running" — `ps` answers that — but "which of my units is that".
//!
//! Nothing reports the answer. Nodal reads it, from four signals, and each signal says
//! how sure it is:
//!
//! | signal | reads | confidence |
//! |---|---|---|
//! | [`process_env`] | `NODAL_ID` in a process's environment | certain |
//! | [`cwd`] | the directory a process stands in | probable |
//! | [`docker`] | a container's labels, then the home it mounts | certain by label, probable by mount |
//! | [`listeners`] | a bound port that the registry granted to a home | probable |
//!
//! The two levels are the whole of the vocabulary, and the difference between them is
//! not a matter of degree. A process carrying `NODAL_ID` was started from that home's
//! environment: the variable is the home's own, written by Nodal, and nothing else puts
//! it there. Everything else is inference from a coincidence that is usually true — a
//! terminal that stands in a home is usually working on it, a container that mounts a
//! home is usually its, a granted port that is bound is usually bound by its grantee —
//! and a person reading a row needs to know which of the two they are looking at before
//! they act on it. So the level is a column, not a footnote.
//!
//! A signal that cannot run returns a note instead of an error ([`Reading`]). A machine
//! with no Docker daemon, and a macOS host whose process table Nodal cannot read yet,
//! both still answer with everything the other signals see. `nodal ps` prints the notes
//! under the table, so an empty answer is never mistaken for a quiet machine.
//!
//! Every signal is pure over a [`Scope`], which is the registry's part of the answer:
//! the homes on this host, their units and the ports they were granted. Reading the
//! registry happens once, in [`ps`](crate::runtime::ps), so a signal is a function of
//! its inputs and a test gives it a scope rather than a machine.

pub mod cwd;
pub mod docker;
pub mod listeners;
pub mod process_env;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{EnvId, Ports, Slug, UnitId};

/// One home on this host, and what the registry says belongs to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    /// The unit the home materialises.
    pub unit: UnitId,
    /// That unit's handle, which is what a row shows.
    pub slug: Slug,
    /// The materialisation itself.
    pub environment: EnvId,
    /// Where it is on disk.
    pub root: PathBuf,
    /// The ports it was granted.
    pub ports: Ports,
}

/// Every home a signal may attribute something to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The homes, in the order the registry gave them.
    pub homes: Vec<Home>,
}

impl Scope {
    /// A scope over these homes.
    #[must_use]
    pub fn new(homes: Vec<Home>) -> Self {
        Self { homes }
    }

    /// The home at exactly this path.
    #[must_use]
    pub fn at(&self, root: &Path) -> Option<&Home> {
        self.homes.iter().find(|home| home.root == root)
    }

    /// The home of this unit. When a unit has more than one materialisation here, the
    /// first the registry gave is used: a caller that knows which one it means says so
    /// with [`Scope::at`].
    #[must_use]
    pub fn of(&self, unit: UnitId) -> Option<&Home> {
        self.homes.iter().find(|home| home.unit == unit)
    }

    /// The home a path is inside, or `None` when it is inside none.
    ///
    /// The deepest match wins, so a home inside another home — which the placement guard
    /// refuses, but an adopted directory can still produce — attributes to the inner
    /// one, which is the more specific answer.
    #[must_use]
    pub fn containing(&self, path: &Path) -> Option<&Home> {
        self.homes
            .iter()
            .filter(|home| path.starts_with(&home.root))
            .max_by_key(|home| home.root.as_os_str().len())
    }

    /// The home that was granted this port, and the name its recipe gives it.
    #[must_use]
    pub fn granted(&self, port: u16) -> Option<(&Home, &str)> {
        self.homes.iter().find_map(|home| {
            let (name, _) = home.ports.0.iter().find(|(_, granted)| **granted == port)?;
            Some((home, name.as_str()))
        })
    }
}

/// How sure a row is.
///
/// Two levels, and no third: either the thing said which unit it belongs to, or Nodal
/// inferred it from where the thing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// The thing itself names the unit: `NODAL_ID` in a process, a label on a container.
    Certain,
    /// Nodal inferred it: a directory, a mount, a granted port.
    Probable,
}

impl Confidence {
    /// The word a row shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Certain => "certain",
            Self::Probable => "probable",
        }
    }
}

/// Which signal produced a row, which is the `by` column and the name in a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The variables a process carries.
    Environment,
    /// The directory a process stands in.
    Cwd,
    /// A container's labels or its mounts.
    Docker,
    /// A bound port the registry granted.
    Listener,
}

impl Source {
    /// The word a row and a note show.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Environment => "env",
            Self::Cwd => "cwd",
            Self::Docker => "docker",
            Self::Listener => "listener",
        }
    }
}

/// What kind of thing a row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A process of this machine.
    Process,
    /// A container.
    Container,
    /// A port with something listening on it.
    Listener,
}

impl Kind {
    /// The word a row shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Container => "container",
            Self::Listener => "listener",
        }
    }
}

/// One thing that is running, and the unit it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attributed {
    /// The unit it belongs to.
    pub unit: UnitId,
    /// That unit's handle.
    pub slug: Slug,
    /// The materialisation it belongs to.
    pub environment: EnvId,
    /// What kind of thing it is.
    pub kind: Kind,
    /// What it is called: a command, a container name, the recipe's name for a port.
    pub what: String,
    /// Its process identifier, when it has one this machine can see.
    pub pid: Option<u32>,
    /// The port it holds, when the row is about one.
    pub port: Option<u16>,
    /// How sure the row is.
    pub confidence: Confidence,
    /// Which signal produced it.
    pub signal: Source,
}

/// Something a signal could not do, in the words a person reads under the table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// The signal that could not run.
    pub signal: Source,
    /// Why, as one line.
    pub why: String,
}

impl Note {
    /// A note from one signal.
    pub fn new(signal: Source, why: impl Into<String>) -> Self {
        Self { signal, why: why.into() }
    }
}

/// What one signal saw, and what it could not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reading {
    /// The rows it produced.
    pub rows: Vec<Attributed>,
    /// Why it produced fewer than it might have, when there is a reason worth printing.
    pub note: Option<Note>,
}

impl Reading {
    /// A reading that saw these rows and has nothing to report.
    #[must_use]
    pub fn saw(rows: Vec<Attributed>) -> Self {
        Self { rows, note: None }
    }

    /// A reading that saw nothing, for this reason.
    pub fn nothing(signal: Source, why: impl Into<String>) -> Self {
        Self { rows: Vec::new(), note: Some(Note::new(signal, why)) }
    }
}

/// One signal: a thing this machine can be read for, and what it says about a scope.
///
/// A signal never fails. What it cannot do it reports as a note, because a machine with
/// no Docker daemon is a machine a person still wants an answer about.
pub trait Attributor {
    /// Which signal this is.
    fn source(&self) -> Source;

    /// What this signal says about the homes in `scope`.
    fn read(&self, scope: &Scope) -> Reading;
}

/// A row about a process, which three of the four signals produce in the same shape.
fn process_row(
    home: &Home,
    pid: u32,
    what: String,
    confidence: Confidence,
    signal: Source,
) -> Attributed {
    Attributed {
        unit: home.unit,
        slug: home.slug.clone(),
        environment: home.environment,
        kind: Kind::Process,
        what,
        pid: Some(pid),
        port: None,
        confidence,
        signal,
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{Home, Scope};
    use crate::model::{EnvId, PortName, Ports, Slug, UnitId};

    /// The unit every fixture home belongs to.
    pub const UNIT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    /// A second unit, so a test can show a row going to the right one of two.
    pub const OTHER: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB1";

    /// One home, with one granted port.
    pub fn home(unit: &str, slug: &str, root: &str, port: u16) -> Home {
        let mut ports = BTreeMap::new();
        ports.insert(PortName::parse("app").unwrap(), port);
        Home {
            unit: UnitId::parse(unit).unwrap(),
            slug: Slug::parse(slug).unwrap(),
            environment: EnvId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(),
            root: PathBuf::from(root),
            ports: Ports(ports),
        }
    }

    /// Two homes of two units, which is the shape every signal has to get right.
    pub fn scope() -> Scope {
        Scope::new(vec![
            home(UNIT, "worker-import", "/homes/worker-import", 41_230),
            home(OTHER, "payroll-export", "/homes/payroll-export", 41_231),
        ])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::{Path, PathBuf};

    use super::fixture::{OTHER, UNIT, scope};
    use super::{Confidence, Scope};
    use crate::model::UnitId;

    #[test]
    fn a_path_belongs_to_the_home_it_is_inside() {
        let scope = scope();
        let inside = scope.containing(Path::new("/homes/worker-import/apps/web")).unwrap();
        assert_eq!(inside.unit, UnitId::parse(UNIT).unwrap());
        assert!(scope.containing(Path::new("/homes")).is_none());
        assert!(scope.containing(Path::new("/tmp")).is_none());
    }

    #[test]
    fn the_deepest_home_wins_over_one_that_contains_it() {
        let mut scope = scope();
        scope.homes.push(super::fixture::home(OTHER, "nested", "/homes/worker-import/inner", 1));
        let found = scope.containing(Path::new("/homes/worker-import/inner/src")).unwrap();
        assert_eq!(found.root, PathBuf::from("/homes/worker-import/inner"));
    }

    #[test]
    fn a_granted_port_names_its_home_and_the_recipes_name_for_it() {
        let scope = scope();
        let (home, name) = scope.granted(41_231).unwrap();
        assert_eq!(home.slug.as_str(), "payroll-export");
        assert_eq!(name, "app");
        assert!(scope.granted(9_999).is_none());
    }

    #[test]
    fn an_empty_scope_attributes_nothing() {
        let empty = Scope::default();
        assert!(empty.containing(Path::new("/homes/worker-import")).is_none());
        assert!(empty.of(UnitId::parse(UNIT).unwrap()).is_none());
    }

    #[test]
    fn certain_sorts_before_probable_so_the_better_row_wins_a_tie() {
        assert!(Confidence::Certain < Confidence::Probable);
    }
}
