//! The `WorkUnit`: a branch with a home directory and a memory.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::ids::{ProjectId, UnitId};
use crate::model::scalar::{self, string_newtype};
use crate::model::timestamp::Timestamp;

string_newtype! {
    /// The CLI handle for a unit, derived from its branch.
    Slug, kind = "slug", shape = scalar::SLUG
}

string_newtype! {
    /// A Git branch name.
    BranchName, kind = "branch name", shape = scalar::BRANCH
}

string_newtype! {
    /// What the unit is for, in one line, as the person or agent stated it.
    Objective, kind = "objective", shape = scalar::LINE
}

/// Where a unit is in its life. The branch, not the directory, is the identity a
/// reviewer or another tool sees, so a unit outlives any one materialisation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum UnitStatus {
    /// Being worked on; holds its branch against other units.
    Open,
    /// Pushed and waiting on review.
    Review,
    /// Merged; runtime may be stopped, the delta is still kept.
    Merged,
    /// Kept for the record only; nothing of it is running.
    Archived,
}

/// One PR-sized piece of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Unit {
    /// Identity of the unit; stable across machines and re-materialisations.
    pub id: UnitId,
    /// The project the unit belongs to.
    pub project_id: ProjectId,
    /// The CLI handle.
    pub slug: Slug,
    /// The stated intent, when there is one. Adoption may recover it later.
    pub objective: Option<Objective>,
    /// The branch this unit owns. Unique among open units, enforced by the registry.
    pub branch: BranchName,
    /// The branch the work started from, when it is known.
    pub parent_branch: Option<BranchName>,
    /// Where the unit is in its life.
    pub status: UnitStatus,
    /// When the unit was created.
    pub created_at: Timestamp,
    /// When any of the above last changed.
    pub updated_at: Timestamp,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::UnitStatus;

    #[test]
    fn status_is_snake_case_in_json() {
        let json = serde_json::to_string(&UnitStatus::Review).expect("serialises");
        assert_eq!(json, r#""review""#);
    }
}
