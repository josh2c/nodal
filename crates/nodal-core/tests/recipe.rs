//! Acceptance for the recipe engine (T0.4).
//!
//! Two checks, and they ask different things.
//!
//! The one CI runs is the fixture: a project of the shape the engine was built for,
//! generated in a temporary directory, whose recipe must come out with **zero gaps**.
//! Every key it needs is somewhere in the project, so anything left unanswered is the
//! engine failing to read a file it should have read.
//!
//! The second compares inference on a real project against a reference recorded
//! elsewhere, and runs only when `NODAL_RECIPE_ROOT` and `NODAL_RECIPE_REFERENCE` point
//! at one. CI never sets them, so CI never depends on a project it does not have; the
//! check is for a developer holding the reference, and it is a plain equality against
//! the serialised recipe, so a drift anywhere shows up as a diff.

#![allow(clippy::expect_used)]

mod fixture;

use std::collections::BTreeMap;
use std::path::PathBuf;

use nodal_core::model::recipe::{
    Backend, DbKind, MigrationTool, PackageManager, Recipe, TaskCache,
};
use nodal_core::recipe::gap::GapKey;
use nodal_core::recipe::{self, Effective};

/// Infer over a freshly written fixture in its own temporary directory.
fn fixture_recipe() -> (tempfile::TempDir, Effective) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    fixture::write(directory.path());
    let effective = recipe::load(directory.path()).expect("the fixture is a readable project");
    (directory, effective)
}

fn strings<T: ToString>(values: &[T]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn the_fixture_infers_with_zero_gaps() {
    let (_directory, effective) = fixture_recipe();
    let unanswered: Vec<GapKey> = effective.gaps.iter().map(|gap| gap.key).collect();
    assert_eq!(unanswered, Vec::<GapKey>::new(), "the fixture should leave nothing to a person");
    assert!(!effective.written, "the fixture has no nodal.toml of its own");
}

#[test]
fn the_fixture_infers_the_shape_of_the_repository() {
    let (_directory, effective) = fixture_recipe();
    let recipe = effective.recipe;

    assert_eq!(recipe.backend(), Backend::Native);
    assert_eq!(recipe.package_manager, Some(PackageManager::Pnpm));
    assert_eq!(
        recipe.package_manager_pin.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm@9.12.3")
    );
    assert!(recipe.monorepo());
    assert_eq!(recipe.task_cache, Some(TaskCache::Turborepo));
    assert_eq!(recipe.dockerfile, Some(PathBuf::from("Dockerfile")));

    let toolchain: BTreeMap<String, String> =
        recipe.toolchain.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(toolchain.get("node").map(String::as_str), Some("22.11.0"));
    assert_eq!(toolchain.get("engines.node").map(String::as_str), Some("22.11.0"));
}

#[test]
fn the_fixture_infers_every_command_its_scripts_state() {
    let (_directory, effective) = fixture_recipe();
    let commands = &effective.recipe.commands;
    assert_eq!(commands.dev.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run dev"));
    assert_eq!(commands.test.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run test"));
    assert_eq!(
        commands.typecheck.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run typecheck")
    );
    assert_eq!(
        commands.migrate.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run db:migrate")
    );
    assert_eq!(
        commands.seed.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run db:seed")
    );
    assert_eq!(
        commands.reset.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run db:reset")
    );
}

#[test]
fn the_fixture_infers_its_database_and_the_ports_the_stack_pins() {
    let (_directory, effective) = fixture_recipe();
    let db = effective.recipe.db;
    assert_eq!(db.kind, Some(DbKind::SupabaseLocal));
    assert_eq!(db.tool, Some(MigrationTool::Supabase));
    assert_eq!(db.migrations_dir, Some(PathBuf::from("supabase/migrations")));
    assert_eq!(strings(&db.url_var), ["SUPABASE_DB_URL", "DATABASE_URL"]);

    let ports: BTreeMap<String, u16> =
        db.fixed_ports.iter().map(|(name, port)| (name.to_string(), *port)).collect();
    assert_eq!(
        ports,
        BTreeMap::from([
            (String::from("api"), 54321),
            (String::from("db"), 54322),
            (String::from("db.pooler"), 54329),
            (String::from("studio"), 54323),
        ]),
        "only the tables that pin a port are ports"
    );
}

#[test]
fn the_fixture_sorts_every_declared_name_into_who_supplies_it() {
    let (_directory, effective) = fixture_recipe();
    let env = effective.recipe.env;
    assert_eq!(
        strings(&env.generated),
        [
            "APP_URL",
            "DATABASE_URL",
            "NEXT_PUBLIC_SUPABASE_ANON_KEY",
            "NEXT_PUBLIC_SUPABASE_URL",
            "PORT",
        ]
    );
    assert_eq!(
        strings(&env.secrets),
        ["CRON_SECRET", "RESEND_API_KEY", "SUPABASE_SERVICE_ROLE_KEY"]
    );
    assert!(env.required_local.is_empty(), "nothing is left for a person to name");
}

#[test]
fn the_fixture_excludes_the_directories_it_regenerates() {
    let (_directory, effective) = fixture_recipe();
    assert_eq!(
        effective.recipe.base.exclude,
        [PathBuf::from("test-results"), PathBuf::from("coverage")],
        "in the order of the exclusion table, and only what is there"
    );
    assert_eq!(
        effective.recipe.services.per_unit.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["postgrest", "gotrue"]
    );
}

#[test]
fn what_init_writes_reads_back_as_the_same_recipe() {
    let (directory, effective) = fixture_recipe();
    let plan = recipe::plan_init(directory.path()).expect("a plan");
    let reparsed = recipe::parse::parse(&plan.contents, &plan.path).expect("the file it wrote");

    // The written file states the defaults the accessors would have supplied, so it is
    // equal to the inferred recipe once those are filled in on both sides.
    let mut inferred = effective.recipe;
    inferred.backend = Some(inferred.backend());
    inferred.monorepo = Some(inferred.monorepo());
    assert_eq!(reparsed, inferred);
}

#[test]
fn init_writes_once_refuses_twice_and_keeps_what_a_person_wrote() {
    let (directory, _) = fixture_recipe();
    let root = directory.path();

    let plan = recipe::plan_init(root).expect("a plan");
    assert!(!plan.existed);
    recipe::apply_init(&plan, false).expect("the first write");

    let second = recipe::plan_init(root).expect("a plan over the written file");
    assert!(second.existed);
    assert!(recipe::apply_init(&second, false).is_err(), "an existing recipe is not overwritten");
    recipe::apply_init(&second, true).expect("--force rewrites it");

    let written = std::fs::read_to_string(root.join(recipe::FILE_NAME)).expect("the recipe");
    assert_eq!(written, second.contents, "applying the same plan twice is the same bytes");
}

#[test]
fn a_written_key_wins_over_the_inferred_one_and_closes_its_gap() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    fixture::write(root);
    // Remove every toolchain pin, which opens the toolchain gap.
    std::fs::remove_file(root.join(".node-version")).expect("the pin file");
    std::fs::write(
        root.join("package.json"),
        "{ \"scripts\": { \"build\": \"turbo run build\", \"test\": \"vitest run\" } }\n",
    )
    .expect("a manifest with no engines field");

    let opened = recipe::load(root).expect("a readable project");
    assert_eq!(opened.gaps.iter().map(|gap| gap.key).collect::<Vec<_>>(), [GapKey::Toolchain]);
    assert_eq!(
        opened.recipe.commands.test.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run test"),
        "the package manager still comes from the lockfile"
    );

    std::fs::write(
        root.join(recipe::FILE_NAME),
        "[toolchain]\nnode = \"20.0.0\"\n\n[commands]\ntest = \"just test\"\n",
    )
    .expect("a hand-written recipe");

    let answered = recipe::load(root).expect("a readable project");
    assert!(answered.gaps.is_empty(), "the file answered the gap");
    assert_eq!(
        answered.recipe.commands.test.as_ref().map(ToString::to_string).as_deref(),
        Some("just test")
    );
    assert_eq!(
        answered.recipe.commands.build.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run build"),
        "a file that sets one command keeps the inferred ones around it"
    );
}

/// Compare inference on a real project against a recorded reference.
///
/// Set `NODAL_RECIPE_ROOT` to the project and `NODAL_RECIPE_REFERENCE` to a JSON file
/// holding the recipe that project should infer to, with the gap keys under `gaps`.
/// Without both, there is nothing to compare and the test passes.
#[test]
fn inference_matches_the_recorded_reference_for_a_real_project() {
    let (Ok(root), Ok(reference)) =
        (std::env::var("NODAL_RECIPE_ROOT"), std::env::var("NODAL_RECIPE_REFERENCE"))
    else {
        eprintln!("skipped: set NODAL_RECIPE_ROOT and NODAL_RECIPE_REFERENCE to run this");
        return;
    };

    let text = std::fs::read_to_string(&reference).expect("the reference file");
    let mut expected: serde_json::Value =
        serde_json::from_str(&text).expect("the reference is JSON");
    let expected_gaps = expected
        .as_object_mut()
        .and_then(|object| object.remove("gaps"))
        .unwrap_or(serde_json::Value::Array(Vec::new()));

    let effective = recipe::load(&root).expect("a readable project");
    let expected_recipe: Recipe =
        serde_json::from_value(expected).expect("the reference is a recipe");
    assert_eq!(effective.recipe, expected_recipe, "inferred recipe differs from the reference");

    let gaps = serde_json::to_value(effective.gaps.iter().map(|gap| gap.key).collect::<Vec<_>>())
        .expect("gap keys serialise");
    assert_eq!(gaps, expected_gaps, "gaps differ from the reference");
}
