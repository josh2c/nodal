//! The registry rows a test writes by hand, in the one shape they all had.
//!
//! Six test files wrote a project, a unit and a materialisation into a registry before
//! asserting on what a command made of them. Every copy set the same fields to the same
//! values, because a row a test writes is a row that says nothing beyond what the test
//! is about: one attempt, managed, stopped, on this host, nothing fingerprinted.
//!
//! Each builder here answers with the row rather than writing it, so a test that needs
//! one field of it to be different says so, in the test, next to the assertion the
//! difference is for. [`record`] is the one exception: a suite that builds a project of
//! ten or twelve units writes every one of them the same way, and nothing about any of
//! them is what that suite is asserting.

use std::path::{Path, PathBuf};

use nodal_core::model::{
    BranchName, Digest, EnvId, EnvState, Environment, HostName, Project, ProjectId, ProjectName,
    Slug, Timestamp, Unit, UnitId, UnitStatus,
};
use nodal_core::store::{Store, environments, units};

/// The host every materialisation a test writes stands on: this one.
#[must_use]
pub fn host() -> HostName {
    nodal_core::lifecycle::owner::current_host()
}

/// A project row whose recipe nothing has hashed and whose checkout has no remote.
///
/// A test that writes its own rows is naming the project by its path, which is what a
/// repository with no `origin` is keyed by.
///
/// # Panics
///
/// If `name` is not a project name.
#[must_use]
pub fn project(id: ProjectId, root: PathBuf, name: &str, at: Timestamp) -> Project {
    Project {
        id,
        root,
        name: ProjectName::parse(name).expect("a project name"),
        recipe_hash: Digest::parse("0".repeat(64)).expect("a digest"),
        created_at: at,
        remote_url: None,
    }
}

/// An open unit row on its own branch.
///
/// # Panics
///
/// If `slug` is not a handle or `branch` is not a branch name.
#[must_use]
pub fn unit(id: UnitId, project: ProjectId, slug: &str, branch: &str, at: Timestamp) -> Unit {
    Unit {
        id,
        project_id: project,
        slug: Slug::parse(slug).expect("a handle"),
        objective: None,
        objective_epistemic: None,
        branch: BranchName::parse(branch).expect("a branch name"),
        parent_branch: None,
        base_commit: None,
        status: UnitStatus::Open,
        created_at: at,
        updated_at: at,
    }
}

/// The first materialisation of a unit: managed, stopped, holding no port.
#[must_use]
pub fn environment(id: EnvId, unit: UnitId, home: &Path, at: Timestamp) -> Environment {
    Environment {
        id,
        unit_id: unit,
        attempt: 1,
        home: home.to_path_buf(),
        managed: true,
        base_id: None,
        ws_fp_materialized: None,
        schema_fp_materialized: None,
        host: host(),
        db_name: None,
        ports: nodal_core::model::Ports::default(),
        fixed_port: None,
        state: EnvState::Stopped,
        created_at: at,
        last_active: at,
    }
}

/// One unit of a fixture project of many.
///
/// Three suites build a project of ten or twelve units and then read the list, the
/// context or the table it prints. Each row is written the same way, and the identifiers
/// are fixed by the row's own number so that two runs of one suite write the same
/// registry.
pub struct Row<'a> {
    /// Which unit of the fixture this is. It fixes both identifiers.
    pub index: usize,
    /// The unit's handle.
    pub slug: &'a str,
    /// The branch its home stands on.
    pub branch: &'a str,
    /// The clone the unit is materialised in.
    pub home: &'a Path,
    /// The host the materialisation stands on.
    pub host: HostName,
}

/// Write one unit and the home it is materialised in, and answer with the unit.
///
/// # Panics
///
/// If the registry refused either row.
pub fn record(store: &Store, project: ProjectId, row: &Row<'_>) -> Unit {
    let now = Timestamp::now();
    let index = row.index;
    let unit = unit(
        UnitId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5F{index:02}")).expect("an identifier"),
        project,
        row.slug,
        row.branch,
        now,
    );
    let mut environment = environment(
        EnvId::parse(format!("01ARZ3NDEKTSV4RRFFQ69G5E{index:02}")).expect("an identifier"),
        unit.id,
        row.home,
        now,
    );
    environment.host = row.host.clone();
    units::insert(store.conn(), &unit).expect("the unit row is written");
    environments::insert(store.conn(), &environment).expect("the materialisation row is written");
    unit
}
