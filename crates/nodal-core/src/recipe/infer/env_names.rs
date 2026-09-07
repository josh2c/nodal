//! The environment variables a working copy needs, sorted into who supplies them.
//!
//! Every declared name lands in exactly one of three places. A name Nodal generates per
//! unit — a port, an application URL, a connection string — is one it must generate,
//! because two units sharing it is the contamination units exist to prevent. A name
//! that reads as a credential is one a secret source supplies, and whose value is never
//! written into a manifest, a bundle or a log.
//!
//! What is left is the honest gap. A declared file is a production superset: it lists
//! what the deployed application reads, not what a working copy needs, and no file in
//! the repository says which is which. So the names that are neither generated nor
//! credentials are offered as candidates and a person picks. When there are none —
//! every declared name is accounted for — there is nothing to ask and no gap.

use crate::model::recipe::{EnvName, Recipe};
use crate::recipe::gap::{Gap, GapKey};
use crate::recipe::infer::{Confidence, Project, Proposal};

/// Files that declare the names an application reads.
const DECLARATIONS: &[&str] = &[".env.example", ".env.sample", "apps/web/.env.example"];

/// Name endings that mean the value is per-unit: a port, a URL that points at this
/// unit's own services, or a key minted with them.
const GENERATED_ENDINGS: &[&str] = &[
    "PORT",
    "APP_URL",
    "WEB_APP_URL",
    "DATABASE_URL",
    "DB_URL",
    "SUPABASE_URL",
    "SUPABASE_ANON_KEY",
];

/// Fragments that mean the value is a credential.
const SECRET_FRAGMENTS: &[&str] = &["KEY", "SECRET", "TOKEN", "PASSWORD", "SID", "DSN"];

/// Propose `env.generated` and `env.secrets`, and raise [`GapKey::EnvRequiredLocal`]
/// for whatever is left over.
#[must_use]
pub fn infer(project: &Project, _so_far: &Recipe) -> Proposal {
    let declared = declared_names(project);
    let (generated, rest): (Vec<_>, Vec<_>) =
        declared.iter().cloned().partition(|name| is_generated(name.as_str()));
    let (secrets, other): (Vec<_>, Vec<_>) =
        rest.into_iter().partition(|name| is_secret(name.as_str()));

    let mut proposal = Proposal::default();
    proposal.recipe.env.generated = generated;
    proposal.recipe.env.secrets = secrets;
    if !proposal.recipe.env.generated.is_empty() {
        proposal = proposal.sure("env.generated", Confidence::High);
    }
    if !proposal.recipe.env.secrets.is_empty() {
        proposal = proposal.sure("env.secrets", Confidence::Medium);
    }
    if other.is_empty() {
        return proposal;
    }
    let note = format!(
        "{declared} names are declared; {other} of them are neither generated per unit nor \
         credentials",
        declared = declared.len(),
        other = other.len(),
    );
    proposal.gap(
        Gap::new(GapKey::EnvRequiredLocal)
            .note(note)
            .candidates(other.iter().map(ToString::to_string).collect()),
    )
}

/// Every name declared by any declaration file, sorted and without duplicates.
fn declared_names(project: &Project) -> Vec<EnvName> {
    let mut names: Vec<EnvName> = DECLARATIONS
        .iter()
        .filter_map(|file| project.read(file))
        .flat_map(|text| names_in(&text))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The names assigned in one dotenv file: the text left of the first `=` on a line.
fn names_in(text: &str) -> Vec<EnvName> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter_map(|(name, _)| EnvName::parse(name).ok())
        .collect()
}

/// Whether `name` ends in one of [`GENERATED_ENDINGS`], on an underscore boundary.
fn is_generated(name: &str) -> bool {
    GENERATED_ENDINGS.iter().any(|ending| {
        name == *ending
            || (name.len() > ending.len()
                && name.ends_with(ending)
                && name.as_bytes()[name.len() - ending.len() - 1] == b'_')
    })
}

/// Whether `name` reads as a credential.
fn is_secret(name: &str) -> bool {
    SECRET_FRAGMENTS.iter().any(|fragment| name.contains(fragment))
}

#[cfg(test)]
mod tests {
    use super::{is_generated, is_secret};

    #[test]
    fn generated_matches_on_an_underscore_boundary_only() {
        assert!(is_generated("PORT"));
        assert!(is_generated("NEXT_PUBLIC_SUPABASE_URL"));
        assert!(is_generated("SUPABASE_DB_URL"));
        assert!(is_generated("WEB_APP_URL"));
        assert!(!is_generated("PORTAL"));
        assert!(!is_generated("SUPABASE_SERVICE_ROLE_KEY"));
    }

    #[test]
    fn credentials_are_recognised_anywhere_in_the_name() {
        assert!(is_secret("RESEND_API_KEY"));
        assert!(is_secret("TWILIO_ACCOUNT_SID"));
        assert!(is_secret("SENTRY_DSN"));
        assert!(!is_secret("LOG_LEVEL"));
    }
}
