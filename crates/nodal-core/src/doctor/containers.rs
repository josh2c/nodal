//! Containers that stopped and stayed, and volumes nothing refers to.
//!
//! A measured machine held twenty-four containers, thirteen of them stopped for weeks,
//! and 1.84 GB of volumes. None of that is in anybody's way, which is why it is still
//! there. Doctor names it and says how big it is.
//!
//! A container is attributed the way `nodal ps` attributes a running one: by the label
//! Nodal puts on the containers it starts, and where there is no label, by the host path
//! the container mounts.
//!
//! Where neither says anything, the name does. A stopped container mounts nothing and a
//! volume nothing refers to has no mount and no label, and both of them still carry a
//! project's name in their own name: `<project>_db` and `supabase_db_<project>` are what
//! a compose file writes. A measured machine held sixteen such rows, 740 MB, every one
//! of them named for a project the command was not run in, and every one of them was
//! filed under the project the command was run in. So a name that positively matches
//! another project doctor knows puts the row in that project's section
//! ([`super::attribution`]).
//!
//! A name that matches nothing still belongs here. "I cannot say whose this is" is not
//! the same claim as "this is another project's", and only the first section may hold
//! the first.
//!
//! Docker absent, or a daemon this account may not reach, is one note and no rows
//! (`services::docker`). The rest of the report is unaffected.

use rusqlite::Connection;

use crate::doctor::attribution::Names;
use crate::doctor::{Scope, Section, note};
use crate::output::view::doctor::{Finding, Kind, Note};
use crate::services::docker::{self, Docker, ENV_LABEL, Exited, Sweep};
use crate::store::environments;
use crate::{Result, model::EnvId};

/// What the source is called in a note.
const SOURCE: &str = "docker";

/// What a look at this machine's containers produced: the rows, each with the section
/// it belongs to, and the note that stands in for them when Docker could not answer.
pub type Found = (Vec<(Section, Finding)>, Option<Note>);

/// Every exited container and unreferenced volume, with the section each belongs to.
///
/// # Errors
/// [`crate::Error::Store`] when an environment row could not be read, [`crate::Error::Tool`]
/// when the daemon answered and then wrote a document that could not be read.
pub fn find(conn: &Connection, docker: &dyn Docker, scope: &Scope) -> Result<Found> {
    let leftovers = match docker::leftovers(docker)? {
        Sweep::Ran(leftovers) => leftovers,
        Sweep::Unavailable { why } => return Ok((Vec::new(), Some(note(SOURCE, why)))),
    };
    let names = Names::of(scope);
    let mut rows = Vec::new();
    for container in &leftovers.exited {
        rows.push((section_of(conn, scope, &names, container)?, row_of(container)));
    }
    for volume in leftovers.dangling {
        let section = names.section(&volume.name).unwrap_or(Section::Here);
        let finding = Finding::new(Kind::DanglingVolume, volume.name);
        rows.push((
            section,
            match volume.bytes {
                Some(bytes) => finding.sized(bytes, true),
                None => finding,
            },
        ));
    }
    Ok((rows, None))
}

/// One exited container as a row.
fn row_of(container: &Exited) -> Finding {
    let finding = Finding::new(Kind::ExitedContainer, container.name.clone())
        .sized(container.bytes, true)
        .says(container.image.clone());
    if container.finished_at.trim().is_empty() {
        return finding;
    }
    finding.says(format!("stopped {}", container.finished_at))
}

/// Which section a container belongs to: another project's, or this one's.
///
/// Four questions, best evidence first, and the first one that answers wins.
///
/// The label answers first, because a container Nodal started carries the
/// materialisation it serves and that row names a home. A mount inside another
/// project answers next, because a container that mounts a directory is standing in
/// that directory. A mount inside this project answers third, and it is asked before
/// the name so that a name can never move a container that is standing in this
/// project's own tree. The name answers last, because it is evidence and not a
/// location ([`super::attribution`]).
fn section_of(
    conn: &Connection,
    scope: &Scope,
    names: &Names,
    container: &Exited,
) -> Result<Section> {
    if let Some(home) = home_of(conn, container)? {
        return Ok(scope.section(&home));
    }
    let mounts = &container.mounts;
    if mounts.iter().any(|mount| scope.section(mount) == Section::Elsewhere) {
        return Ok(Section::Elsewhere);
    }
    if mounts.iter().any(|mount| scope.owns(mount)) {
        return Ok(Section::Here);
    }
    Ok(names.section(&container.name).unwrap_or(Section::Here))
}

/// The home a labelled container serves, when the registry still has the row.
fn home_of(conn: &Connection, container: &Exited) -> Result<Option<std::path::PathBuf>> {
    let Some(id) = container.label(ENV_LABEL).and_then(|text| EnvId::parse(text).ok()) else {
        return Ok(None);
    };
    Ok(environments::get(conn, id)?.map(|environment| environment.home))
}
