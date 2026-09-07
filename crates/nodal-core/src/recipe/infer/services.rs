//! Which services a project runs, and which of them a unit needs its own copy of.
//!
//! Compose files are listed but never split: whether a service is safe to share is a
//! judgement about the project, not about the file, so it is raised as a gap. A local
//! Supabase stack is the case Nodal does know: the database and the stack services stay
//! shared, and the per-unit API layer — `postgrest` and `gotrue` — is what each unit gets
//! its own of. That split is a decision about the stack, not a reading of the project,
//! so it is proposed at medium confidence for a person to confirm.

use crate::model::recipe::{Recipe, ServiceName};
use crate::recipe::gap::{Gap, GapKey};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Compose files a project may define services in.
const COMPOSE_FILES: &[&str] = &[
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "compose.yaml",
    "docker-compose.dev.yml",
];

/// The Supabase local stack's services that one instance serves every unit from.
const SUPABASE_SHARED: &[&str] = &["db", "realtime", "storage", "studio", "mail"];

/// The Supabase services each unit gets its own instance of.
const SUPABASE_PER_UNIT: &[&str] = &["postgrest", "gotrue"];

/// Propose `services.*` and `dockerfile`, or raise [`GapKey::Services`].
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let mut proposal = Proposal::default();
    if project.exists("Dockerfile") {
        proposal.recipe.dockerfile = Some("Dockerfile".into());
    }

    let compose = project.all_existing(COMPOSE_FILES);
    if !compose.is_empty() {
        proposal.recipe.compose = compose.iter().map(Into::into).collect();
        return proposal.gap(
            Gap::new(GapKey::Services)
                .note("this project defines its services in Compose")
                .candidates(compose.iter().map(|file| (*file).to_owned()).collect()),
        );
    }

    if project.exists("supabase/config.toml") {
        proposal.recipe.services.shared = names(SUPABASE_SHARED);
        proposal.recipe.services.per_unit = names(SUPABASE_PER_UNIT);
        return proposal.sure("services", Confidence::Medium);
    }
    proposal.gap(Gap::new(GapKey::Services).note("no service definition was found"))
}

fn names(raw: &[&str]) -> Vec<ServiceName> {
    raw.iter().filter_map(|name| name.parse().ok()).collect()
}
