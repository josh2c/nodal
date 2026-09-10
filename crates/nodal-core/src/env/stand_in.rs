//! Stand-ins: a value for a generated name that no adapter answered.
//!
//! A project's generate step reads a name and fails when the name is unset. Prisma
//! reads `DATABASE_URL` and refuses to run without one, so a fresh unit of such a
//! project could not run `pnpm run generate` until its services were up. The step does
//! not connect to anything. It only needs the name to hold a value of the right shape.
//!
//! So Nodal mints one at create. A stand-in is a value derived from the unit's handle
//! and the project's port block. It points at no service. An adapter that produces the
//! name replaces it at the next activation, and until then the manifest, `nodal env`,
//! `nodal explain` and the unit's memory all say the value is a stand-in.
//!
//! # Why it is never silent
//!
//! A value that looks real and is not is worse than no value, because a person reads a
//! connection refused and looks for a service rather than for a placeholder. Every
//! surface that shows the name shows that it is a stand-in, and the create names every
//! one it made.
//!
//! # Why the port is derived and not granted
//!
//! The create grants its ports in the registry write that ends the operation, which is
//! after the step that writes `.nodal/env`. A stand-in therefore cannot hold a granted
//! port. It holds a port of the project's own block, derived from the unit's handle and
//! the name ([`crate::services::ports::derived`]), so it is the same value on every run
//! of the same unit and it never names another project's service.
//!
//! # Why a name that asks for a bare port gets nothing
//!
//! A port inside a URL is an address of a service that does not exist. Nothing binds
//! it. A bare port is different: a dev server reads `PORT` and binds it. A derived port
//! there could be a port this project granted to another unit, and the second server to
//! start would take the socket of the first — which is the silent collision
//! [`crate::services::ports`] exists to prevent. So such a name gets no stand-in and
//! stays on the missing list. A port a process binds is granted, never derived.

use std::collections::BTreeMap;

use crate::Result;
use crate::model::recipe::EnvName;
use crate::model::unit::Slug;
use crate::model::{PortBlock, Recipe};
use crate::services::ports;

/// The template name a recipe fills with the unit's handle.
const SLUG: &str = "{slug}";

/// The template name a recipe fills with the derived port.
const PORT: &str = "{port}";

/// The role a stand-in database URL is written for. It is not a credential: no database
/// answers on the derived port, and the value exists so that a generate step can parse
/// it.
const ROLE: &str = "nodal";

/// Name endings that ask for a Postgres connection string.
const DATABASE: &[&str] = &["DATABASE_URL", "DB_URL"];

/// Name endings that ask for a Redis connection string.
const REDIS: &[&str] = &["REDIS_URL"];

/// The ending that asks for a plain HTTP URL, once the two above have had their turn.
const URL: &str = "URL";

/// The ending that asks for a bare port number, which gets no stand-in.
const PORT_ENDING: &str = "PORT";

/// What a unit's stand-ins are minted from.
///
/// One of these is built where the project's port block and the unit's handle are both
/// in hand, which is every place activation is assembled.
#[derive(Debug, Clone)]
pub struct StandIns {
    /// The unit's handle. It names the database and the fallback value.
    slug: Slug,
    /// The project's port block. Every derived port comes out of it.
    block: PortBlock,
    /// The templates the recipe pins, by name.
    templates: BTreeMap<EnvName, String>,
}

impl StandIns {
    /// The stand-ins one unit of one project can mint.
    #[must_use]
    pub fn new(slug: Slug, block: PortBlock, recipe: &Recipe) -> Self {
        Self { slug, block, templates: recipe.env.stand_in.clone() }
    }

    /// The value `name` takes when nothing produced it, and `None` when it takes none.
    ///
    /// The recipe's own template wins, whatever the name is: a project that pins one
    /// has said what it wants. A name the recipe does not pin takes the value its shape
    /// asks for — a Postgres URL, a Redis URL, an HTTP URL, or the unit's handle behind
    /// a `nodal-` prefix when the name says nothing about shape.
    ///
    /// A name that asks for a bare port takes nothing, for the reason the module
    /// documentation gives: a port a process binds is granted, never derived.
    ///
    /// # Errors
    /// [`crate::Error::InvalidValue`] when the port could not be derived.
    pub fn mint(&self, name: &EnvName) -> Result<Option<String>> {
        let port = self.port(name)?;
        if let Some(template) = self.templates.get(name) {
            return Ok(Some(fill(template, self.slug.as_str(), port)));
        }
        let text = name.as_str();
        if ends_with_any(text, DATABASE) {
            return Ok(Some(format!(
                "postgresql://{ROLE}:{ROLE}@localhost:{port}/{slug}",
                slug = self.slug
            )));
        }
        if ends_with_any(text, REDIS) {
            return Ok(Some(format!("redis://localhost:{port}")));
        }
        if ends_with(text, PORT_ENDING) {
            return Ok(None);
        }
        if ends_with(text, URL) {
            return Ok(Some(format!("http://localhost:{port}")));
        }
        Ok(Some(format!("nodal-{slug}", slug = self.slug)))
    }

    /// The port this name derives to inside the project's block.
    fn port(&self, name: &EnvName) -> Result<u16> {
        ports::derived(self.block, &format!("{slug}\u{0}{name}", slug = self.slug))
    }
}

/// Fill the two template names. Everything else in the text is kept as it was written.
fn fill(template: &str, slug: &str, port: u16) -> String {
    template.replace(SLUG, slug).replace(PORT, &port.to_string())
}

/// Whether `name` ends with any of `endings`, on an underscore boundary.
fn ends_with_any(name: &str, endings: &[&str]) -> bool {
    endings.iter().any(|ending| ends_with(name, ending))
}

/// Whether `name` is `ending`, or ends with it after an underscore.
///
/// The boundary is what keeps `SUPPORT` away from the port rule and `CURL` away from
/// the URL rule. It is the rule `crate::recipe::infer::env_names` sorts names by, and
/// the two agree on purpose: a name inferred into `env.generated` by its ending is a
/// name minted here by the same ending.
fn ends_with(name: &str, ending: &str) -> bool {
    name == ending
        || (name.len() > ending.len()
            && name.ends_with(ending)
            && name.as_bytes()[name.len() - ending.len() - 1] == b'_')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::collections::BTreeMap;

    use super::StandIns;
    use crate::model::recipe::EnvName;
    use crate::model::unit::Slug;
    use crate::model::{PortBlock, ProjectId, Recipe};

    fn name(text: &str) -> EnvName {
        EnvName::parse(text).unwrap()
    }

    fn minter(recipe: &Recipe) -> StandIns {
        let block = PortBlock {
            project_id: ProjectId::from_ulid(ulid::Ulid::nil()),
            first: 20_000,
            last: 20_099,
        };
        StandIns::new(Slug::parse("payroll-export").unwrap(), block, recipe)
    }

    #[test]
    fn a_database_name_gets_a_connection_string_on_a_port_of_the_block() {
        let minted = minter(&Recipe::default()).mint(&name("DATABASE_URL")).unwrap().unwrap();
        assert!(minted.starts_with("postgresql://nodal:nodal@localhost:2"), "{minted}");
        assert!(minted.ends_with("/payroll-export"), "{minted}");
        let port: u16 = minted
            .rsplit_once(':')
            .and_then(|(_, tail)| tail.split_once('/'))
            .and_then(|(port, _)| port.parse().ok())
            .unwrap();
        assert!((20_000..=20_099).contains(&port), "{port} is outside the block");
    }

    #[test]
    fn each_shape_gets_the_value_its_name_asks_for() {
        let minter = minter(&Recipe::default());
        let value = |text: &str| minter.mint(&name(text)).unwrap().unwrap();
        assert!(value("REDIS_URL").starts_with("redis://localhost:"));
        assert!(value("APP_URL").starts_with("http://localhost:"));
        assert!(value("SUPABASE_DB_URL").starts_with("postgresql://"));
        assert_eq!(value("SESSION_KEY"), "nodal-payroll-export");
    }

    #[test]
    fn a_name_that_asks_for_a_bare_port_gets_no_stand_in() {
        let minter = minter(&Recipe::default());
        assert_eq!(minter.mint(&name("PORT")).unwrap(), None);
        assert_eq!(minter.mint(&name("WEB_PORT")).unwrap(), None);
        assert_eq!(
            minter.mint(&name("SUPPORT")).unwrap(),
            Some(String::from("nodal-payroll-export")),
            "a word that ends in the letters of PORT is not a port"
        );
    }

    #[test]
    fn a_pinned_template_wins_even_for_a_name_that_would_get_nothing() {
        let mut recipe = Recipe::default();
        recipe.env.stand_in = BTreeMap::from([(name("PORT"), String::from("3000"))]);
        assert_eq!(minter(&recipe).mint(&name("PORT")).unwrap(), Some(String::from("3000")));
    }

    #[test]
    fn the_same_unit_mints_the_same_value_every_time() {
        let first = minter(&Recipe::default()).mint(&name("DATABASE_URL")).unwrap();
        let again = minter(&Recipe::default()).mint(&name("DATABASE_URL")).unwrap();
        assert!(first.is_some());
        assert_eq!(first, again);
    }

    #[test]
    fn the_recipe_pins_the_template_for_a_name() {
        let mut recipe = Recipe::default();
        recipe.env.stand_in = BTreeMap::from([(
            name("DATABASE_URL"),
            String::from("postgres://someone@127.0.0.1:{port}/{slug}_dev"),
        )]);
        let minted = minter(&recipe).mint(&name("DATABASE_URL")).unwrap().unwrap();
        assert!(minted.starts_with("postgres://someone@127.0.0.1:2"), "{minted}");
        assert!(minted.ends_with("/payroll-export_dev"), "{minted}");
    }
}
