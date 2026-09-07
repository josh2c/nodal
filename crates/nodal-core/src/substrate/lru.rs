//! Which bases a project stops keeping, as a pure function over what it has.
//!
//! A base costs disk and nothing else: it is never in the way, and the only reason to
//! remove one is that the disk is wanted for something else. So the policy is the
//! plainest one that cannot surprise anybody — hold the most recently used, drop the
//! rest — and it is a function of two arguments so that a person can predict it and a
//! test can state it without a filesystem.
//!
//! A pinned base is one a unit is still cloned from. It is never dropped, and it does
//! not count towards the number kept: the number is how many *idle* bases a project
//! holds against the next `nodal new`, and a base with units on it is not idle.

use crate::model::{BaseId, Timestamp};

/// How many idle bases a project keeps when nothing asks for another number.
///
/// Two, because the case this exists for is a person moving between the branch they
/// are on and the one they came from: one base for each, and the third is the one
/// worth the disk it would take.
pub const DEFAULT_KEEP: usize = 2;

/// One base as eviction sees it: when it was last cloned from, and by how many units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Which base.
    pub id: BaseId,
    /// When a unit was last made from it.
    pub last_used: Timestamp,
    /// How many units hold it against eviction.
    pub pins: u32,
}

impl Candidate {
    /// Whether a unit holds this base.
    #[must_use]
    pub const fn is_pinned(&self) -> bool {
        self.pins > 0
    }
}

/// The bases to evict, least recently used first.
///
/// Pinned candidates are never returned. Ties on `last_used` are broken by identifier,
/// which is a ULID and therefore orders by the instant the base was made, so the answer
/// is the same on two machines with the same rows.
#[must_use]
pub fn evictable(candidates: &[Candidate], keep: usize) -> Vec<BaseId> {
    let mut idle: Vec<&Candidate> = candidates.iter().filter(|row| !row.is_pinned()).collect();
    idle.sort_by(|left, right| {
        left.last_used.cmp(&right.last_used).then_with(|| left.id.cmp(&right.id))
    });
    let over = idle.len().saturating_sub(keep);
    idle.into_iter().take(over).map(|row| row.id).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use super::{Candidate, DEFAULT_KEEP, evictable};
    use crate::model::{BaseId, Timestamp};

    fn candidate(id: u128, second: i64, pins: u32) -> Candidate {
        Candidate {
            id: BaseId::from_ulid(ulid::Ulid(id)),
            last_used: Timestamp::from_unix_seconds(second).unwrap(),
            pins,
        }
    }

    #[test]
    fn nothing_is_evicted_while_the_project_is_inside_its_number() {
        let rows = [candidate(1, 10, 0), candidate(2, 20, 0)];
        assert!(evictable(&rows, DEFAULT_KEEP).is_empty());
    }

    #[test]
    fn the_least_recently_used_goes_first() {
        let rows = [candidate(1, 30, 0), candidate(2, 10, 0), candidate(3, 20, 0)];
        assert_eq!(evictable(&rows, 1), vec![rows[1].id, rows[2].id]);
    }

    #[test]
    fn a_pinned_base_is_never_evicted_and_does_not_take_a_place() {
        let rows = [candidate(1, 10, 3), candidate(2, 20, 0), candidate(3, 30, 0)];
        assert!(evictable(&rows, 2).is_empty());
        assert_eq!(evictable(&rows, 1), vec![rows[1].id]);
    }

    #[test]
    fn keeping_none_evicts_every_idle_base_and_no_pinned_one() {
        let rows = [candidate(1, 10, 0), candidate(2, 20, 1)];
        assert_eq!(evictable(&rows, 0), vec![rows[0].id]);
    }

    #[test]
    fn bases_used_at_the_same_instant_go_in_identifier_order() {
        let rows = [candidate(2, 10, 0), candidate(1, 10, 0)];
        assert_eq!(evictable(&rows, 0), vec![rows[1].id, rows[0].id]);
    }
}
