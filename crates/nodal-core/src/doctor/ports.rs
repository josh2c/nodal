//! How much of this project's block of ports is held, and by which units.
//!
//! Every other row of a report is something a tool left behind. This one is a ceiling.
//! A project is granted one block of [`crate::services::ports::BLOCK_SPAN`] ports and
//! keeps it, and every unit takes one port for itself and one for each service name its
//! recipe declares. So the ports are the resource a project runs out of first, and the
//! port a `nodal new` cannot get is the first a person hears of it.
//!
//! The reading is therefore in the report a person runs before they are stopped, and it
//! is the same two numbers the refusal carries ([`crate::Error::PortBlockFull`]): how
//! much of the block is held, and who holds it. It reads three tables and writes
//! nothing.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::doctor::Scope;
use crate::model::{EnvId, ProjectId, Slug};
use crate::output::view::doctor::{PortHolder, PortUse};
use crate::store::{environments, port_allocations, port_blocks, units};

/// What this project's block holds, and `None` where it has no block.
///
/// A block is granted the first time a project needs a port, so a project that has
/// never made a unit has none and nothing is reported about it.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read, and
/// [`crate::Error::StoreRow`] when a column does not hold a value the model accepts.
pub fn find(conn: &Connection, scope: &Scope) -> crate::Result<Option<PortUse>> {
    let Some(project) = here(scope) else {
        return Ok(None);
    };
    let Some(block) = port_blocks::get(conn, project)? else {
        return Ok(None);
    };
    let allocations = port_allocations::list_in_range(conn, block.first, block.last)?;
    let names = slugs(conn, project)?;
    let mut by_unit: BTreeMap<Slug, usize> = BTreeMap::new();
    for allocation in &allocations {
        if let Some(slug) = names.get(&allocation.environment_id) {
            *by_unit.entry(slug.clone()).or_default() += 1;
        }
    }
    let mut holders: Vec<PortHolder> =
        by_unit.into_iter().map(|(unit, ports)| PortHolder { unit, ports }).collect();
    holders.sort_by(|left, right| {
        right.ports.cmp(&left.ports).then_with(|| left.unit.cmp(&right.unit))
    });
    Ok(Some(PortUse {
        first: block.first,
        last: block.last,
        span: block.span(),
        held: allocations.len(),
        holders,
    }))
}

/// The project of the checkout the command was run in.
///
/// The project is found among the roots the scope already resolved, for the reason
/// [`crate::doctor::survey`] resolves them: a registry row holds the path whoever wrote
/// it used, which on a host with a linked temporary directory is not the path this
/// command resolved.
fn here(scope: &Scope) -> Option<ProjectId> {
    let root = scope.root.as_ref()?;
    scope.projects.iter().find(|known| &known.root == root).map(|known| known.project.id)
}

/// The handle of the unit behind each environment of the project.
///
/// A unit of several materialisations holds ports under each of them, and the row a
/// person reads is the unit. So the environments are named here and counted by the
/// handle they lead to.
fn slugs(conn: &Connection, project: ProjectId) -> crate::Result<BTreeMap<EnvId, Slug>> {
    let units: BTreeMap<_, _> =
        units::list(conn, project)?.into_iter().map(|unit| (unit.id, unit.slug)).collect();
    Ok(environments::list_for_project(conn, project)?
        .into_iter()
        .filter_map(|environment| {
            units.get(&environment.unit_id).map(|slug| (environment.id, slug.clone()))
        })
        .collect())
}
