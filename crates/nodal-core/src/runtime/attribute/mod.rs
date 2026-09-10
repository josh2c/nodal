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
//! The level is also what a teardown acts on. `nodal reclaim` and `nodal gc` signal the
//! certain level and report the probable one ([`Standing`]). A process carrying
//! `NODAL_ID` was started from the home. A process that only stands in the home may be
//! a tmux pane, an editor server over SSH, or a teammate's shell, and Nodal never
//! signals one of those.
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
//!
//! ## One name per path
//!
//! Two of the four signals answer with a path: `/proc/<pid>/cwd` is a link, so reading
//! it gives the directory with every link on the way to it already followed, and Docker
//! resolves a mount before it prints one. A registry row holds the path whoever created
//! the home used, which nothing resolved. On a host whose temporary directory is a link
//! — macOS names `/var/folders` and means `/private/var/folders` — one home therefore
//! arrives under two names, and `starts_with` between them is false: the dev server in
//! the home gets no row at all.
//!
//! So a path is resolved once, at the edge: every home root on the way into [`Scope`],
//! and every path handed to [`Scope::at`] or [`Scope::containing`] on the way in from a
//! signal. Past that edge every path here is the name the filesystem itself uses. The
//! resolver is [`guard::resolve`](crate::lifecycle::guard::resolve), which is the one
//! place in Nodal a path is normalised before it is compared with another; attribution
//! does not have a rule of its own about this.

pub mod cwd;
pub mod docker;
pub mod listeners;
pub mod process_env;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lifecycle::guard;
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
///
/// Every root here is resolved. Nothing in this type is the name a registry row happened
/// to hold for a home; it is the name the filesystem uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    /// The homes, in the order the registry gave them.
    pub homes: Vec<Home>,
}

impl Scope {
    /// A scope over these homes, each root resolved.
    ///
    /// This is the edge the registry's answer comes in at. A row holds the path whoever
    /// created the home used; a signal answers with the path the operating system gives
    /// back, which has every link on it followed.
    #[must_use]
    pub fn new(homes: Vec<Home>) -> Self {
        let homes = homes
            .into_iter()
            .map(|home| Home { root: guard::resolve(&home.root), ..home })
            .collect();
        Self { homes }
    }

    /// The home at exactly this path.
    ///
    /// The path is resolved first: `NODAL_ROOT` is the home's own name for itself,
    /// written when the home was created and never resolved.
    #[must_use]
    pub fn at(&self, root: &Path) -> Option<&Home> {
        let root = guard::resolve(root);
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
    ///
    /// The path is resolved first, so that a signal which followed every link and a
    /// registry row which followed none cannot name one home two ways.
    #[must_use]
    pub fn containing(&self, path: &Path) -> Option<&Home> {
        let path = guard::resolve(path);
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

/// The sentence a report uses for a process at the probable level.
///
/// One string, because a reclaim's report and a sweep's report make the same claim
/// about the same kind of process, and a person who has read one has read the other.
pub const NOT_SIGNALLED: &str = "standing in the home; not signalled";

/// A process that only stands in a home, which is the probable level of one signal.
///
/// This is the level nothing is ever signalled at. A tmux pane, an editor server over
/// SSH and a teammate's shell all reach a home the same way — they stand in it — and
/// none of the three says which unit it is working on. So a teardown reports one of
/// these by name and process, and leaves it running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Its process identifier, which is what a person acts on.
    pub pid: u32,
    /// A short form of its command, when the process table had one.
    pub command: Option<String>,
}

impl Standing {
    /// The process a scan saw, at the probable level.
    #[must_use]
    pub const fn new(pid: u32, command: Option<String>) -> Self {
        Self { pid, command }
    }

    /// What it is and which process it is, which is all a person needs to find it.
    #[must_use]
    pub fn label(&self) -> String {
        let what = self.command.as_deref().unwrap_or("unnamed command");
        format!("{what} (pid {})", self.pid)
    }

    /// The line a report prints: the process, and what was not done to it.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{}; {NOT_SIGNALLED}", self.label())
    }

    /// Every one of them on one line, for a message that has no room for a table.
    #[must_use]
    pub fn summarise(standing: &[Self]) -> String {
        standing.iter().map(Self::label).collect::<Vec<String>>().join(", ")
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

    /// The macOS condition, made on any host.
    ///
    /// On macOS a home under the temporary directory is created at `/var/folders/…` and
    /// is `/private/var/folders/…`; the registry keeps the first name and `/proc`, or
    /// Docker, answers with the second. Both names must reach one home.
    #[test]
    fn a_home_named_through_a_link_is_the_same_home() {
        let directory = tempfile::TempDir::new().unwrap();
        let real = directory.path().join("real");
        std::fs::create_dir_all(real.join("apps").join("web")).unwrap();
        let link = directory.path().join("by-another-name");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // The row holds the linked name, which is the name the home was created under.
        let linked = link.to_str().unwrap();
        let scope = Scope::new(vec![super::fixture::home(UNIT, "worker-import", linked, 41_230)]);

        // A signal answers with the resolved name, and still means this home.
        let standing = scope.containing(&real.join("apps").join("web")).unwrap();
        assert_eq!(standing.unit, UnitId::parse(UNIT).unwrap());

        // `NODAL_ROOT` carries the linked name, and still names this home.
        assert!(scope.at(&link).is_some(), "the home cannot find itself by its own name");
        assert!(scope.at(&real).is_some(), "the home cannot be found by its resolved name");
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
