//! `nodal done`: send the unit's work to the remote and put it up for review.
//!
//! Two refs go: the unit's branch, and the work-in-progress snapshot of everything the
//! home holds that no commit does. The snapshot is what makes `done` honest about a
//! home a person walked away from mid-edit — the branch carries the commits, the
//! snapshot carries the rest — and it is taken here rather than pushed from wherever it
//! happened to be, so what reaches the remote is the home as it is now.
//!
//! **This is the only operation in Nodal that touches a network**, and it does so with
//! exactly one `git push` ([`crate::git::push`]), run as the user's own `git`. There is
//! no host API here, no token, and no pull request: `done` prints the page a person
//! opens the change on and stops there. Opening one is the person's, and there is no
//! code path in this crate that could.
//!
//! **Why this is not a journalled [`Plan`](crate::lifecycle::Plan).** A push cannot be
//! undone, and a step is required to have an undo that works. Giving the push an empty
//! one would let the runner report a `done` as rolled back after the branch had already
//! reached the remote, which is a lie about the one act here that other people can see.
//! So `done` is written as what it is: a check, one push, and one registry write. It is
//! safe to be killed at any point because every part of it is idempotent — pushing the
//! same refs again sends nothing, and flipping a unit that is already in review changes
//! nothing — so the recovery for an interrupted `done` is to run it again.

use std::path::PathBuf;

use rusqlite::Connection;

use crate::git::{Git, host, push, refs};
use crate::model::{
    EnvState, Environment, Epistemic, Event, EventId, EventKind, RefName, Timestamp, Unit,
    UnitStatus,
};
use crate::output::view::Done;
use crate::runtime::{actor, entry};
use crate::store::{Store, environments, events, units};
use crate::{Error, Result};

/// The message the work-in-progress snapshot commit carries.
const SNAPSHOT_MESSAGE: &str = "nodal: work in progress at done";

/// The remote a push goes to when the repository names one by this name.
const DEFAULT_REMOTE: &str = "origin";

/// What a person asked `nodal done` for.
#[derive(Debug, Clone)]
pub struct Request {
    /// The unit's handle, or nothing to mean the unit the working directory is in.
    pub target: Option<String>,
    /// The remote to push to, or nothing to let the repository decide.
    pub remote: Option<String>,
    /// Where the command was run, which decides the unit when no target was given.
    pub cwd: PathBuf,
}

/// Push the unit's branch and its work-in-progress ref, and put it up for review.
///
/// # Errors
/// [`Error::UnitNotFound`] when no unit has that handle, [`Error::UnitNotMaterialized`]
/// or [`Error::AlreadyReclaimed`] when there is no home to push from,
/// [`Error::GitNoRemote`] and [`Error::GitRemoteAmbiguous`] when the repository does not
/// decide where a push goes, [`Error::Git`] when the push itself was refused, and
/// [`Error::Store`] when the registry could not be written.
pub fn done(store: &mut Store, request: &Request) -> Result<Done> {
    let unit = entry::unit_named(store.conn(), request.target.as_deref(), &request.cwd)?;
    let environment = home_of(store.conn(), &unit)?;
    let git = Git::open(&environment.home)?;
    let remote = chosen(&git, request.remote.as_deref())?;
    let sent = send(&git, &unit, &remote)?;
    let now = Timestamp::now();
    record(store, &unit, &environment, &sent, now)?;
    Ok(report(&unit, &remote, &sent, git.remote_url(&remote)?.as_deref(), now))
}

/// The refs one `done` sent, and the snapshot it took first.
struct Sent {
    /// The refs that went, in the order they were given to `git push`.
    names: Vec<String>,
    /// The work-in-progress ref, when the home had a commit to build one on.
    snapshot: Option<String>,
}

/// Take the snapshot and push both refs, in that order.
///
/// The order matters: a snapshot taken after the push would be of a home nothing had
/// sent, and the report would name a ref the remote does not have.
fn send(git: &Git, unit: &Unit, remote: &str) -> Result<Sent> {
    let branch = format!("refs/heads/{}", unit.branch);
    let snapshot = git.snapshot(&refs::wip(&unit.id.to_string()), SNAPSHOT_MESSAGE)?;
    let snapshot = snapshot.map(|taken| taken.reference);
    git.push(remote, &refspecs(&branch, snapshot.as_deref()))?;
    let mut names = vec![branch];
    names.extend(snapshot.clone());
    Ok(Sent { names, snapshot })
}

/// What one `done` asks `git push` to send.
///
/// The branch goes as it is: a push that would not fast-forward is refused by the
/// remote and reported, never forced. The snapshot ref is Nodal's own and is replaced,
/// because each snapshot is built from the working tree rather than on the last one.
fn refspecs(branch: &str, snapshot: Option<&str>) -> Vec<String> {
    let mut specs = vec![push::same_name(branch)];
    specs.extend(snapshot.map(push::forced));
    specs
}

/// Which remote the push goes to.
///
/// A name given on the command line is used as it is. Otherwise `origin`, which is what
/// a clone makes and what almost every repository has; then the only remote, when there
/// is exactly one under another name. Anything else is a question rather than a guess,
/// because a push to the wrong remote is not something Nodal can take back.
fn chosen(git: &Git, asked: Option<&str>) -> Result<String> {
    if let Some(name) = asked {
        return Ok(name.to_owned());
    }
    let mut remotes = git.remotes()?;
    if remotes.iter().any(|name| name == DEFAULT_REMOTE) {
        return Ok(String::from(DEFAULT_REMOTE));
    }
    match remotes.len() {
        0 => Err(Error::GitNoRemote { repo: git.root().to_path_buf() }),
        1 => Ok(remotes.remove(0)),
        _ => Err(Error::GitRemoteAmbiguous { repo: git.root().to_path_buf(), remotes }),
    }
}

/// The unit's newest materialisation, refusing one there is nothing left to push from.
fn home_of(conn: &Connection, unit: &Unit) -> Result<Environment> {
    let environment = environments::latest_for_unit(conn, unit.id)?
        .ok_or_else(|| Error::UnitNotMaterialized { slug: unit.slug.to_string() })?;
    if environment.state == EnvState::Absent {
        return Err(Error::AlreadyReclaimed { slug: unit.slug.clone() });
    }
    Ok(environment)
}

/// Put the unit up for review, and write the line a later `nodal explain` reads.
///
/// One transaction, so the state and the record of why it changed cannot come apart.
/// A unit already in review is left in review: `done` run twice is one state, not an
/// error, because the push it repeats is also a no-op.
fn record(
    store: &mut Store,
    unit: &Unit,
    environment: &Environment,
    sent: &Sent,
    now: Timestamp,
) -> Result<()> {
    let tx = store.transaction()?;
    units::update_status(&tx, unit.id, UnitStatus::Review, now)?;
    let mut refs = std::collections::BTreeMap::new();
    if let (Ok(name), Some(reference)) = (RefName::parse("wip"), &sent.snapshot) {
        refs.insert(name, reference.clone());
    }
    events::append(
        &tx,
        &Event {
            id: EventId::from_ulid(ulid::Ulid::new()),
            unit: unit.id,
            environment: Some(environment.id),
            ts: now,
            actor: actor::current()?,
            kind: EventKind::Sync,
            epistemic: Epistemic::Observed,
            body: format!("pushed {} for review", sent.names.join(" and ")),
            refs,
            raw_ref: None,
        },
    )?;
    tx.commit().map_err(crate::store::row::store_error(store.conn()))?;
    Ok(())
}

/// The answer, assembled from what the push sent and what the remote is called.
fn report(unit: &Unit, remote: &str, sent: &Sent, url: Option<&str>, now: Timestamp) -> Done {
    let web = url.and_then(host::web);
    let compare = url.and_then(|url| host::compare(url, unit.branch.as_str()));
    let mut notes = Vec::new();
    if sent.snapshot.is_none() {
        notes.push(String::from("the home has no commit yet, so no work-in-progress ref went"));
    }
    Done {
        now,
        slug: unit.slug.clone(),
        branch: unit.branch.clone(),
        remote: remote.to_owned(),
        host: web.map(|web| web.host),
        pushed: sent.names.clone(),
        snapshot: sent.snapshot.clone(),
        compare,
        status: UnitStatus::Review,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::refspecs;

    #[test]
    fn only_the_ref_nodal_owns_is_ever_forced() {
        let specs = refspecs("refs/heads/topic", Some("refs/nodal/01J/wip"));
        let forced: Vec<&String> = specs.iter().filter(|spec| spec.starts_with('+')).collect();
        assert_eq!(forced.len(), 1, "{specs:?}");
        assert!(forced[0].contains("refs/nodal/"), "{forced:?}");
    }

    #[test]
    fn a_home_with_no_snapshot_pushes_the_branch_and_nothing_else() {
        assert_eq!(refspecs("refs/heads/topic", None), ["refs/heads/topic:refs/heads/topic"]);
    }
}
