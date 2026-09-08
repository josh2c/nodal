//! How long nothing has happened in a unit, worked out from what the registry already
//! records.
//!
//! This is arithmetic over timestamps and nothing else: no process is read, no
//! directory is walked, and nothing here can act. That is deliberate, and it is the
//! shape the rule for idle units was settled with. `nodal gc` **reports** a live unit
//! that has gone quiet and never stops it, because a development server, a database
//! container or a debugger left running for a fortnight is somebody's work, not
//! garbage, and a command that ends one on a timer without being asked is the same
//! hazard `nodal doctor` was already ruled out of. Reclaimed units are the other case
//! and the only one `gc` acts on: nothing of a unit whose home has been taken away
//! should still be running at all.
//!
//! The clock reads two things and takes the later of them. The **session rows** are the
//! record of who was in the unit and when they left, which is what "nobody has touched
//! this" means. The environment's own `last_active` is the fallback, for a unit nobody
//! has ever attached to and for whatever else touches it.
//!
//! A unit with a session still open is never idle, whatever the clock says. An open row
//! is a claim that somebody is in there now.

use crate::model::{Session, Timestamp};

/// Seconds in a day, which is the unit a threshold is given in.
const DAY: i64 = 24 * 60 * 60;

/// Whether anybody is still attached.
#[must_use]
pub fn attached(sessions: &[Session]) -> bool {
    sessions.iter().any(|session| session.ended_at.is_none())
}

/// The last instant anything is recorded as having happened in the unit.
///
/// The later of the newest session instant and the environment's own. A session that is
/// still open counts as now, which never happens in practice because [`attached`] has
/// already answered for that unit; it is here so that this function cannot report a
/// past instant for a unit somebody is sitting in.
#[must_use]
pub fn last_seen(sessions: &[Session], last_active: Timestamp, now: Timestamp) -> Timestamp {
    sessions
        .iter()
        .map(|session| session.ended_at.unwrap_or(now))
        .chain(std::iter::once(last_active))
        .max_by_key(|stamp: &Timestamp| stamp.unix_seconds())
        .unwrap_or(last_active)
}

/// Whether `last` is more than `days` before `now`.
#[must_use]
pub fn is_idle(last: Timestamp, now: Timestamp, days: u32) -> bool {
    let threshold = i64::from(days).saturating_mul(DAY);
    now.unix_seconds().saturating_sub(last.unix_seconds()) >= threshold
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use super::{attached, is_idle, last_seen};
    use crate::model::{Actor, ActorKind, ActorName, EnvId, Session, SessionId, Timestamp};

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    fn session(started: &str, ended: Option<&str>) -> Session {
        Session {
            id: SessionId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            environment_id: EnvId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(),
            actor: Actor { kind: ActorKind::Human, name: ActorName::parse("someone").unwrap() },
            pid: None,
            pgid: None,
            started_at: at(started),
            ended_at: ended.map(at),
        }
    }

    #[test]
    fn a_unit_somebody_is_still_in_is_never_idle() {
        let open = [session("2026-08-01T09:00:00Z", None)];
        assert!(attached(&open));
        assert_eq!(
            last_seen(&open, at("2026-08-01T09:00:00Z"), at("2026-09-07T09:00:00Z")),
            at("2026-09-07T09:00:00Z")
        );
    }

    #[test]
    fn the_clock_reads_from_whichever_signal_is_later() {
        let now = at("2026-09-07T09:00:00Z");
        let ended = [session("2026-08-01T09:00:00Z", Some("2026-09-01T09:00:00Z"))];
        assert_eq!(last_seen(&ended, at("2026-08-20T09:00:00Z"), now), at("2026-09-01T09:00:00Z"));
        assert_eq!(last_seen(&ended, at("2026-09-05T09:00:00Z"), now), at("2026-09-05T09:00:00Z"));
        assert_eq!(last_seen(&[], at("2026-09-05T09:00:00Z"), now), at("2026-09-05T09:00:00Z"));
    }

    #[test]
    fn a_threshold_of_days_is_that_many_days_of_quiet() {
        let now = at("2026-09-07T09:00:00Z");
        assert!(is_idle(at("2026-08-31T08:00:00Z"), now, 7));
        assert!(!is_idle(at("2026-09-01T10:00:00Z"), now, 7));
        assert!(is_idle(now, now, 0), "a threshold of nothing is every unit");
    }
}
