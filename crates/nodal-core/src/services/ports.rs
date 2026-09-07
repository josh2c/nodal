//! The port allocator: a block per project, a port per name, and who holds a fixed one.
//!
//! Ports are the resource two units collide on first, and the collision is silent: the
//! second unit's dev server takes the next port it can find, the browser goes to the
//! first one, and the answers come from the wrong unit. So a port here is granted, not
//! chosen. A project is given a block once and keeps it; an environment takes ports from
//! that block; reclaim gives them back.
//!
//! Two kinds of port, two records, for one reason. A port from the block is a row in
//! `port_allocation`, held for as long as the environment exists. A port the project
//! pins — `db.fixed_ports` in the recipe, a Postgres on 54322 that its own configuration
//! names — cannot be moved, so only one environment may hold it at a time and the claim
//! has to lapse if that environment's process dies. That is a lease, and the lease table
//! is where it lives.
//!
//! Nothing here reads before it writes. Both records are claimed with one statement
//! whose uniqueness the database enforces, so two `nodal new` runs in two terminals are
//! separated by SQLite rather than by whichever of them read first.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::model::{
    EnvId, Lease, PortAllocation, PortBlock, PortName, Ports, ProjectId, ResourceKey, Slug,
    Timestamp, UnitId,
};
use crate::store::{Store, environments, leases, port_allocations, port_blocks, row, units};
use crate::{Error, Result};

/// The lowest port any block starts at.
///
/// The range sits above the ports development tools pick by habit (3000, 5432, 8080)
/// and below the range Linux allocates ephemeral ports from, which starts at 32768. A
/// granted port therefore cannot collide with a port the kernel hands to an outgoing
/// connection of some other process.
pub const RANGE_FIRST: u16 = 20_000;

/// The highest port any block covers.
pub const RANGE_LAST: u16 = 29_999;

/// How many ports one project's block holds. One hundred services in one unit is far
/// past what a project has, and it leaves a hundred blocks in the range.
pub const BLOCK_SPAN: u16 = 100;

/// The kind a port's resource key carries in the lease table.
const RESOURCE_KIND: &str = "port:";

/// How many times a fixed-port claim is retried when the holder lets go between the
/// refusal and the read that says who held it. One retry settles a race that has
/// already resolved; a port that changes hands on every attempt is reported, not
/// retried forever.
const CLAIM_ATTEMPTS: usize = 3;

/// One port a recipe pins, under the name the recipe gives it.
#[derive(Debug, Clone, Copy)]
struct Fixed<'a> {
    /// What the recipe calls the port.
    name: &'a PortName,
    /// The port itself.
    port: u16,
}

/// A fixed port another environment holds, and the unit that environment belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedPortConflict {
    /// What the recipe calls the port.
    pub name: PortName,
    /// The port itself.
    pub port: u16,
    /// The unit that holds it. This is the answer a person needs.
    pub unit: UnitId,
    /// That unit's slug, which is what `nodal ls` shows.
    pub slug: Slug,
    /// The environment of that unit which took the lease.
    pub environment: EnvId,
    /// When the claim lapses unless the holder renews it.
    pub expires_at: Timestamp,
}

/// What reclaim gave back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Released {
    /// Ports returned to the project's block, lowest first.
    pub allocated: Vec<u16>,
    /// Fixed ports whose lease was given up, lowest first.
    pub fixed: Vec<u16>,
}

/// The block a project hands ports out from, giving it one if it has none.
///
/// Two processes doing this at once give the project one block, not two: the second
/// finds the first's row inside the same transaction it would have written in.
///
/// # Errors
/// [`Error::PortBlockRangeFull`] when every block is taken, [`Error::Store`] on a
/// failed statement.
pub fn ensure_block(store: &mut Store, project_id: ProjectId) -> Result<PortBlock> {
    if let Some(block) = port_blocks::get(store.conn(), project_id)? {
        return Ok(block);
    }
    let tx = store.transaction()?;
    let block = if let Some(block) = port_blocks::get(&tx, project_id)? {
        block
    } else {
        let block = free_block(&port_blocks::list(&tx)?, project_id)?;
        port_blocks::insert(&tx, &block)?;
        block
    };
    tx.commit().map_err(row::store_error(store.conn()))?;
    Ok(block)
}

/// Grant an environment one port for each name, and record every grant.
///
/// Asking twice returns the same ports: a name the environment already holds a port
/// under is answered from the record, so the call is a step that can be repeated.
///
/// # Errors
/// [`Error::PortBlockFull`] when every port in the block is held, [`Error::Store`] on a
/// failed statement.
pub fn allocate(
    conn: &Connection,
    block: PortBlock,
    environment: EnvId,
    names: &[PortName],
) -> Result<Ports> {
    let mut granted = BTreeMap::new();
    for name in names {
        granted.insert(name.clone(), allocate_one(conn, block, environment, name)?);
    }
    Ok(Ports(granted))
}

/// Give back every port an environment holds: the block ports and the fixed leases.
///
/// Releasing what is not held is not a failure, so reclaim can run this twice.
///
/// # Errors
/// [`Error::Store`] on a failed statement, [`Error::StoreRow`] when a column does not
/// hold a value the model accepts.
pub fn release(conn: &Connection, environment: EnvId) -> Result<Released> {
    let allocated: Vec<u16> =
        port_allocations::list_for_environment(conn, environment)?.iter().map(|a| a.port).collect();
    port_allocations::release_all(conn, environment)?;
    Ok(Released { allocated, fixed: release_fixed(conn, environment)? })
}

/// Report every fixed port another environment holds, without claiming anything.
///
/// An empty answer means the claimant may take them all. A claim that has lapsed is not
/// a conflict, and neither is a port the claimant already holds.
///
/// # Errors
/// [`Error::Store`] on a failed statement, [`Error::StoreMissingRow`] when the holding
/// environment or its unit is not in the registry.
pub fn check_fixed(
    conn: &Connection,
    claimant: EnvId,
    fixed: &BTreeMap<PortName, u16>,
    now: Timestamp,
) -> Result<Vec<FixedPortConflict>> {
    let mut conflicts = Vec::new();
    for (name, port) in fixed {
        if let Some(conflict) = holder_of(conn, claimant, Fixed { name, port: *port }, now)? {
            conflicts.push(conflict);
        }
    }
    Ok(conflicts)
}

/// Take every fixed port for the claimant, or take none and say who holds one.
///
/// All or nothing on purpose: a unit that holds three of a stack's four pinned ports
/// has a broken stack and has taken three ports off the unit that could have used them.
/// The leases it took in this call are given back before the conflict is reported.
///
/// # Errors
/// [`Error::PortFixedContended`] when a port changed hands on every attempt,
/// [`Error::Store`] on a failed statement, [`Error::StoreMissingRow`] when the holding
/// environment or its unit is not in the registry.
pub fn hold_fixed(
    conn: &Connection,
    claimant: EnvId,
    fixed: &BTreeMap<PortName, u16>,
    now: Timestamp,
    until: Timestamp,
) -> Result<Option<FixedPortConflict>> {
    let mut taken: Vec<ResourceKey> = Vec::new();
    for (name, port) in fixed {
        match take_one(conn, claimant, Fixed { name, port: *port }, now, until)? {
            Ok(key) => taken.push(key),
            Err(conflict) => {
                for key in &taken {
                    leases::release(conn, key, claimant)?;
                }
                return Ok(Some(conflict));
            }
        }
    }
    Ok(None)
}

/// The resource key a port is leased under.
///
/// # Errors
/// [`Error::InvalidValue`] never happens for a number, and is propagated rather than
/// hidden so that the key's shape stays the model's rule and not this module's.
pub fn resource(port: u16) -> Result<ResourceKey> {
    ResourceKey::parse(format!("{RESOURCE_KIND}{port}"))
}

/// The port a resource key names, `None` when the key is not a port.
#[must_use]
pub fn port_of(resource: &ResourceKey) -> Option<u16> {
    resource.as_str().strip_prefix(RESOURCE_KIND)?.parse().ok()
}

/// The lowest block in the range that no project holds.
///
/// Blocks are aligned to [`BLOCK_SPAN`] from [`RANGE_FIRST`], so two blocks either
/// start at the same port or do not overlap at all, and a block is free exactly when
/// its first port is not one that is taken.
fn free_block(taken: &[PortBlock], project_id: ProjectId) -> Result<PortBlock> {
    let starts: BTreeSet<u16> = taken.iter().map(|block| block.first).collect();
    let mut first = RANGE_FIRST;
    while u32::from(first) + u32::from(BLOCK_SPAN) - 1 <= u32::from(RANGE_LAST) {
        if !starts.contains(&first) {
            return Ok(PortBlock { project_id, first, last: first + BLOCK_SPAN - 1 });
        }
        first += BLOCK_SPAN;
    }
    Err(Error::PortBlockRangeFull { first: RANGE_FIRST, last: RANGE_LAST })
}

/// The port an environment holds under one name.
fn held(conn: &Connection, environment: EnvId, name: &PortName) -> Result<Option<u16>> {
    Ok(port_allocations::find_by_name(conn, environment, name)?.map(|held| held.port))
}

/// Grant one port, or report that the block is full.
///
/// The ports already held are read first so that the common case is one write rather
/// than one write per port that is taken. That read is a hint and nothing more: the
/// grant itself is the write, and a port taken between the read and the write is a
/// refusal that moves on to the next port.
fn allocate_one(
    conn: &Connection,
    block: PortBlock,
    environment: EnvId,
    name: &PortName,
) -> Result<u16> {
    if let Some(port) = held(conn, environment, name)? {
        return Ok(port);
    }
    let taken: BTreeSet<u16> = port_allocations::list_in_range(conn, block.first, block.last)?
        .iter()
        .map(|allocation| allocation.port)
        .collect();
    for port in block.ports().filter(|port| !taken.contains(port)) {
        let allocation = PortAllocation {
            port,
            project_id: block.project_id,
            environment_id: environment,
            name: name.clone(),
        };
        if port_allocations::claim(conn, &allocation)? {
            return Ok(port);
        }
    }
    // A refusal is also what a second grant under one name meets, so the record has the
    // last word on whether this environment came away with a port.
    held(conn, environment, name)?.ok_or(Error::PortBlockFull {
        environment,
        first: block.first,
        last: block.last,
    })
}

/// Give up every port lease an environment holds.
fn release_fixed(conn: &Connection, environment: EnvId) -> Result<Vec<u16>> {
    let mut released = Vec::new();
    for lease in leases::list_for_environment(conn, environment)? {
        if let Some(port) = port_of(&lease.resource)
            && leases::release(conn, &lease.resource, environment)?
        {
            released.push(port);
        }
    }
    released.sort_unstable();
    Ok(released)
}

/// Take one fixed port, or say who holds it.
fn take_one(
    conn: &Connection,
    claimant: EnvId,
    fixed: Fixed<'_>,
    now: Timestamp,
    until: Timestamp,
) -> Result<std::result::Result<ResourceKey, FixedPortConflict>> {
    let key = resource(fixed.port)?;
    for _ in 0..CLAIM_ATTEMPTS {
        let lease = Lease { resource: key.clone(), environment_id: claimant, expires_at: until };
        if leases::acquire(conn, &lease, now)? {
            return Ok(Ok(key));
        }
        if let Some(conflict) = holder_of(conn, claimant, fixed, now)? {
            return Ok(Err(conflict));
        }
    }
    Err(Error::PortFixedContended { port: fixed.port, attempts: CLAIM_ATTEMPTS })
}

/// Who holds a fixed port, `None` when it is free, lapsed, or the claimant's own.
fn holder_of(
    conn: &Connection,
    claimant: EnvId,
    fixed: Fixed<'_>,
    now: Timestamp,
) -> Result<Option<FixedPortConflict>> {
    let Some(lease) = leases::get(conn, &resource(fixed.port)?)? else {
        return Ok(None);
    };
    if lease.environment_id == claimant || lease.expires_at <= now {
        return Ok(None);
    }
    let unit = unit_behind(conn, lease.environment_id)?;
    Ok(Some(FixedPortConflict {
        name: fixed.name.clone(),
        port: fixed.port,
        unit: unit.0,
        slug: unit.1,
        environment: lease.environment_id,
        expires_at: lease.expires_at,
    }))
}

/// The unit an environment materialises, by identity and by the name a person reads.
fn unit_behind(conn: &Connection, environment: EnvId) -> Result<(UnitId, Slug)> {
    let environment = environments::get(conn, environment)?.ok_or_else(|| {
        Error::StoreMissingRow { table: "environment", id: environment.to_string() }
    })?;
    let unit = units::get(conn, environment.unit_id)?.ok_or_else(|| Error::StoreMissingRow {
        table: "unit",
        id: environment.unit_id.to_string(),
    })?;
    Ok((unit.id, unit.slug))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{
        BLOCK_SPAN, PortBlock, ProjectId, RANGE_FIRST, RANGE_LAST, free_block, port_of, resource,
    };

    fn project(last: char) -> ProjectId {
        format!("01J8Z6H000000000000000000{last}").parse().unwrap()
    }

    fn block(first: u16) -> PortBlock {
        PortBlock { project_id: project('1'), first, last: first + BLOCK_SPAN - 1 }
    }

    #[test]
    fn the_first_project_gets_the_first_block() {
        let block = free_block(&[], project('2')).unwrap();
        assert_eq!((block.first, block.last), (RANGE_FIRST, RANGE_FIRST + BLOCK_SPAN - 1));
        assert!(block.contains(RANGE_FIRST));
        assert!(!block.contains(RANGE_FIRST + BLOCK_SPAN));
    }

    #[test]
    fn a_block_is_the_lowest_one_nobody_holds() {
        let taken = [block(RANGE_FIRST), block(RANGE_FIRST + BLOCK_SPAN)];
        let free = free_block(&taken, project('2')).unwrap();
        assert_eq!(free.first, RANGE_FIRST + 2 * BLOCK_SPAN);
    }

    #[test]
    fn a_hole_left_by_a_removed_project_is_filled_before_the_end() {
        let taken = [block(RANGE_FIRST), block(RANGE_FIRST + 2 * BLOCK_SPAN)];
        assert_eq!(free_block(&taken, project('2')).unwrap().first, RANGE_FIRST + BLOCK_SPAN);
    }

    #[test]
    fn a_full_range_is_reported_rather_than_wrapping() {
        let taken: Vec<PortBlock> =
            (RANGE_FIRST..=RANGE_LAST).step_by(BLOCK_SPAN as usize).map(block).collect();
        let error = free_block(&taken, project('2')).unwrap_err();
        assert!(error.to_string().contains("no port block is free"), "{error}");
    }

    #[test]
    fn a_port_key_reads_back_as_the_port_it_names() {
        let key = resource(54_322).unwrap();
        assert_eq!(key.as_str(), "port:54322");
        assert_eq!(port_of(&key), Some(54_322));
    }

    #[test]
    fn a_key_of_another_kind_is_not_a_port() {
        assert_eq!(port_of(&"device:usb0".parse().unwrap()), None);
        assert_eq!(port_of(&"port:notaport".parse().unwrap()), None);
        assert_eq!(port_of(&"port:99999".parse().unwrap()), None);
    }
}
