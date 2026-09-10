//! Why a unit is as it is: the decisions behind one home, read back out of the record.
//!
//! Five questions, and each is answered from what was written down at the time rather
//! than from what the same code would decide now. Which tree the home came from is the
//! environment's `base_id`. What the clone left out is the exclusion table
//! ([`crate::workspace::exclude`]) with the project's own `base.exclude` added, which is
//! the same pair the materialiser was given. What was removed from the copy afterwards
//! is the relocation the create recorded in the unit's log. Where the ports came from is
//! the project's block and the grants against it.
//!
//! Nothing here asks Git or walks a disk, so an explanation can be given for a unit
//! whose home has been reclaimed and for one on a host that is not this one.

use rusqlite::Connection;

use crate::env::files;
use crate::model::manifest::Origin as EnvOrigin;
use crate::model::{Environment, Project, RefName, Timestamp, Unit};
use crate::output::view::explain::{
    Exclusion, Explained, Invalidation, Origin, PortLine, StandInLine,
};
use crate::recipe;
use crate::store::{bases, environments, events, port_blocks};
use crate::workspace::exclude;
use crate::workspace::relocate::INVALIDATE;
use crate::{Error, Result};

/// The name the relocation event puts its relocator under, which is how the events that
/// are relocations are told from the rest of a unit's log.
const RELOCATOR: &str = "relocator";

/// Who decided an exclusion: Nodal's own table, or the project's recipe.
const BY_NODAL: &str = "nodal";
/// The other one.
const BY_PROJECT: &str = "project";

/// Why the project's own rows are left out, in the words the recipe reader uses.
const RECIPE_REASON: &str = "named by the project in base.exclude";

/// Why one unit is as it is.
///
/// # Errors
/// [`Error::UnitNotMaterialized`] when the unit has no home on any host, and whatever
/// the registry or the recipe reader reported.
pub fn explain(
    conn: &Connection,
    project: &Project,
    unit: &Unit,
    now: Timestamp,
) -> Result<Explained> {
    let environment = environments::latest_for_unit(conn, unit.id)?
        .ok_or_else(|| Error::UnitNotMaterialized { slug: unit.slug.to_string() })?;
    let origin = origin_of(conn, &environment)?;
    Ok(Explained {
        now,
        slug: unit.slug.clone(),
        home: Some(environment.home.clone()),
        excluded: excluded(project, &origin)?,
        invalidated: invalidated(conn, unit)?,
        ports: ports(conn, project, &environment)?,
        stand_ins: stand_ins(conn, project, &environment),
        origin,
    })
}

/// Where the home came from: the checkout it already was, or the base it is a clone of.
fn origin_of(conn: &Connection, environment: &Environment) -> Result<Origin> {
    if !environment.managed {
        return Ok(Origin::Adopted { root: true });
    }
    let Some(id) = environment.base_id else {
        return Ok(Origin::Unrecorded {
            why: String::from("the registry records no base for this home"),
        });
    };
    let Some(base) = bases::get(conn, id)? else {
        return Ok(Origin::Unrecorded {
            why: format!("base {id} is no longer in the registry; it has been collected"),
        });
    };
    Ok(Origin::Cloned {
        base: base.id,
        path: base.path,
        fingerprint: base.ws_fingerprint,
        platform: base.platform,
        commit: base.commit,
        built_at: base.built_at,
    })
}

/// What a clone of this project leaves out, and who decided each row.
///
/// A checkout adopted in place was never cloned, so it has no exclusions at all rather
/// than the ones a clone would have had.
fn excluded(project: &Project, origin: &Origin) -> Result<Vec<Exclusion>> {
    if matches!(origin, Origin::Adopted { .. }) {
        return Ok(Vec::new());
    }
    let mut rows: Vec<Exclusion> = exclude::ROWS
        .iter()
        .filter(|row| !row.keep)
        .map(|row| Exclusion {
            path: row.path.to_owned(),
            reason: row.reason.to_owned(),
            decided_by: String::from(BY_NODAL),
        })
        .collect();
    for path in &recipe::load(&project.root)?.recipe.base.exclude {
        rows.push(Exclusion {
            path: path.display().to_string(),
            reason: String::from(RECIPE_REASON),
            decided_by: String::from(BY_PROJECT),
        });
    }
    Ok(rows)
}

/// What was removed from the copy after it was made, as the log recorded it.
fn invalidated(conn: &Connection, unit: &Unit) -> Result<Vec<Invalidation>> {
    let relocator = RefName::parse(RELOCATOR)?;
    let from = RefName::parse("from")?;
    let removed = RefName::parse("removed")?;
    Ok(events::list_for_unit(conn, unit.id)?
        .into_iter()
        .filter(|event| event.refs.get(&relocator).is_some_and(|name| name == INVALIDATE))
        .map(|event| Invalidation {
            at: event.ts,
            removed: event.refs.get(&removed).cloned().unwrap_or_default(),
            from: event.refs.get(&from).cloned().unwrap_or_default(),
            body: event.body,
        })
        .collect())
}

/// The ports the unit holds, and the block each came out of.
fn ports(conn: &Connection, project: &Project, environment: &Environment) -> Result<Vec<PortLine>> {
    let block = port_blocks::get(conn, project.id)?;
    let source = match block {
        Some(block) => format!(
            "the block {first}–{last} this project was granted",
            first = block.first,
            last = block.last
        ),
        None => String::from("a block this project no longer holds"),
    };
    Ok(environment
        .ports
        .0
        .iter()
        .map(|(name, port)| PortLine {
            name: name.to_string(),
            port: *port,
            source: source.clone(),
        })
        .collect())
}

/// Which of the unit's names hold a stand-in, and where each value came from.
///
/// `None` says the manifest was not read. A home on another host, or one that has been
/// reclaimed, gives that rather than an empty list, because an empty list is the claim
/// that every generated name has a real value.
fn stand_ins(
    conn: &Connection,
    project: &Project,
    environment: &Environment,
) -> Option<Vec<StandInLine>> {
    let manifest = files::read_manifest(&environment.home).ok()?;
    let source = match port_blocks::get(conn, project.id) {
        Ok(Some(block)) => format!(
            "nodal, at create: no adapter produced it, so the value is derived from the \
             unit's handle and a port of the block {first}\u{2013}{last}",
            first = block.first,
            last = block.last
        ),
        Ok(None) | Err(_) => String::from(
            "nodal, at create: no adapter produced it, so the value is derived from the \
             unit's handle and a port of the project's block",
        ),
    };
    Some(
        manifest
            .env
            .iter()
            .filter(|(_, origin)| **origin == EnvOrigin::StandIn)
            .map(|(name, _)| StandInLine { name: name.to_string(), source: source.clone() })
            .collect(),
    )
}
