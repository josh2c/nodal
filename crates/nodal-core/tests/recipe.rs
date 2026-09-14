//! Acceptance for the recipe engine over the fixture project.
//!
//! Two checks, and they ask different things.
//!
//! The one CI runs is the fixture ([`nodal_fixture`]): a project of the shape the engine
//! was built for, generated in a temporary directory, whose recipe must come out with
//! **zero gaps**. Every key it needs is stated somewhere in the project, so anything
//! left unanswered is the engine failing to read a file it should have read. All but one
//! of those keys is read out of the project's own files; the exception is which Compose
//! services are safe to share, which the fixture's own `nodal.toml` answers because no
//! file of a project ever states it. Deleting that file must leave exactly that one gap,
//! which is what holds the rest of the fixture to being inferred rather than declared.
//!
//! The second compares inference on a real project against a reference recorded
//! elsewhere, and runs only when `NODAL_RECIPE_ROOT` and `NODAL_RECIPE_REFERENCE` point
//! at one. CI never sets them, so CI never depends on a project it does not have; the
//! check is for a developer holding the reference, and it is a plain equality against
//! the serialised recipe, so a drift anywhere shows up as a diff.

#![allow(clippy::expect_used, reason = "tests fail by panicking")]

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
    let root = nodal_fixture::write(directory.path());
    let effective = recipe::load(&root).expect("the fixture is a readable project");
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
    assert!(effective.written, "the fixture carries the recipe that answers its one judgement");
}

#[test]
fn the_only_thing_the_fixture_declares_is_the_one_no_file_states() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = nodal_fixture::write(directory.path());
    std::fs::remove_file(root.join(nodal_fixture::RECIPE)).expect("the fixture's recipe");

    let effective = recipe::load(&root).expect("a readable project");
    let unanswered: Vec<GapKey> = effective.gaps.iter().map(|gap| gap.key).collect();
    assert_eq!(
        unanswered,
        [GapKey::Services],
        "everything but the service split is read out of the project"
    );
    assert_eq!(
        effective.recipe.compose,
        [PathBuf::from("compose.yaml")],
        "the gap names the file it could not split"
    );
}

#[test]
fn the_fixture_infers_the_shape_of_the_repository() {
    let (_directory, effective) = fixture_recipe();
    let recipe = effective.recipe;

    assert_eq!(recipe.backend(), Backend::Native);
    assert_eq!(recipe.package_manager, [PackageManager::Pnpm]);
    assert_eq!(
        recipe.package_manager_pin.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm@9.12.3")
    );
    assert!(recipe.monorepo());
    assert_eq!(recipe.task_cache, Some(TaskCache::Turborepo));
    assert_eq!(recipe.dockerfile, Some(PathBuf::from("Dockerfile")));
    assert_eq!(recipe.compose, [PathBuf::from("compose.yaml")]);

    let toolchain: BTreeMap<String, String> =
        recipe.toolchain.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(toolchain.get("node").map(String::as_str), Some("22.11.0"));
    assert_eq!(toolchain.get("engines.node").map(String::as_str), Some("22.11.0"));
}

/// A Cargo project states its minimum toolchain in `Cargo.toml`, and nowhere else.
///
/// `rust-version` is the same kind of claim `engines` makes: what the tool checks, not
/// what a version manager selects. Reading it is what stops `nodal init` on a Rust
/// project from reporting a toolchain gap the project has already answered.
#[test]
fn a_cargo_manifest_answers_the_toolchain_gap_with_its_rust_version() {
    for table in ["package", "workspace.package"] {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let root = directory.path();
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[{table}]\nname = \"crate-of-one\"\nrust-version = \"1.88\"\n"),
        )
        .expect("a manifest");

        let opened = recipe::load(root).expect("a readable project");
        let pinned: BTreeMap<String, String> =
            opened.recipe.toolchain.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(
            pinned.get("cargo.rust").map(String::as_str),
            Some("1.88"),
            "the manifest's rust-version was not read from [{table}]"
        );
        assert!(
            !opened.gaps.iter().any(|gap| gap.key == GapKey::Toolchain),
            "the toolchain gap is still open on a project that pins one: {:?}",
            opened.gaps
        );
    }
}

#[test]
fn the_fixture_infers_every_command_its_scripts_state() {
    let (_directory, effective) = fixture_recipe();
    let commands = &effective.recipe.commands;
    assert_eq!(commands.dev.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run dev"));
    assert_eq!(commands.build.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run build"));
    assert_eq!(commands.test.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run test"));
    assert_eq!(commands.lint.as_ref().map(ToString::to_string).as_deref(), Some("pnpm run lint"));
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
fn the_fixture_infers_its_database_from_the_directory_and_the_scripts() {
    let (_directory, effective) = fixture_recipe();
    let db = effective.recipe.db;
    assert_eq!(
        db.tool,
        Some(MigrationTool::Unknown),
        "a plain migrations directory is a convention several tools share"
    );
    assert_eq!(db.migrations_dir, Some(PathBuf::from("migrations")));
    assert!(db.fixed_ports.is_empty(), "nothing in the project pins a port");

    // The two keys the project's files cannot state, which its recipe does.
    assert_eq!(db.kind, Some(DbKind::Postgres));
    assert_eq!(strings(&db.url_var), ["DATABASE_URL"]);
}

/// The Supabase stack states things a plain Postgres project does not, and the fixture
/// is not one. This is the smallest project that exercises those readings.
#[test]
fn a_supabase_stack_states_its_kind_its_ports_and_where_it_publishes_its_url() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    let config = "[api]\nport = 54321\n\n[db]\nport = 54322\n\n[db.pooler]\nport = 54329\n";
    std::fs::create_dir_all(root.join("supabase/migrations")).expect("the migrations directory");
    std::fs::write(root.join("supabase/config.toml"), config).expect("the stack configuration");
    std::fs::write(root.join("supabase/migrations/0001_init.sql"), "select 1;\n")
        .expect("a migration");

    let effective = recipe::load(root).expect("a readable project");
    assert_eq!(
        effective.recipe.commands.migrate.as_ref().map(ToString::to_string).as_deref(),
        Some("supabase db push"),
        "with no script to prefer, the tool's own command stands"
    );
    assert_eq!(
        strings(&effective.recipe.services.per_unit),
        ["postgrest", "gotrue"],
        "the stack's split is known, so it is proposed rather than asked"
    );

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
        ["APP_URL", "DATABASE_URL", "NEXT_PUBLIC_APP_URL", "PORT"],
        "the union of both declaration files, with PORT named once"
    );
    assert_eq!(
        strings(&env.secrets),
        ["CRON_SECRET", "POSTGRES_PASSWORD", "RESEND_API_KEY", "SENTRY_DSN", "SESSION_SECRET"]
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
    assert_eq!(strings(&effective.recipe.services.shared), ["db", "mailpit"]);
    assert_eq!(
        strings(&effective.recipe.services.per_unit),
        ["redis"],
        "the split the fixture's own recipe states, because Compose does not"
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
    // The fixture as it was before anyone adopted it: its own recipe removed.
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = nodal_fixture::write(directory.path());
    std::fs::remove_file(root.join(nodal_fixture::RECIPE)).expect("the fixture's recipe");
    let root = root.as_path();

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
    let root = nodal_fixture::write(directory.path());
    let root = root.as_path();
    std::fs::remove_file(root.join(nodal_fixture::RECIPE)).expect("the fixture's recipe");
    // Remove every toolchain pin, which opens the toolchain gap beside the services one.
    std::fs::remove_file(root.join(".node-version")).expect("the pin file");
    std::fs::write(
        root.join("package.json"),
        "{ \"scripts\": { \"build\": \"turbo run build\", \"test\": \"vitest run\" } }\n",
    )
    .expect("a manifest with no engines field");

    let opened = recipe::load(root).expect("a readable project");
    assert_eq!(
        opened.gaps.iter().map(|gap| gap.key).collect::<Vec<_>>(),
        [GapKey::Toolchain, GapKey::Services]
    );
    assert_eq!(
        opened.recipe.commands.test.as_ref().map(ToString::to_string).as_deref(),
        Some("pnpm run test"),
        "the package manager still comes from the lockfile"
    );

    std::fs::write(
        root.join(recipe::FILE_NAME),
        "[toolchain]\nnode = \"20.0.0\"\n\n[commands]\ntest = \"just test\"\n\n\
         [services]\nshared = [\"db\"]\n",
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

/// The toolchain of a project, by the name each pin is recorded under.
fn pins(root: &std::path::Path) -> BTreeMap<String, String> {
    let opened = recipe::load(root).expect("a readable project");
    opened.recipe.toolchain.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A project whose only file is `relative`, with `contents` in it.
fn project_of(relative: &str, contents: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path().to_path_buf();
    std::fs::write(root.join(relative), contents).expect("the file is written");
    (directory, root)
}

/// A Python project states the interpreter it needs in `pyproject.toml`, which is the
/// file every packaging backend reads, and often in no other file at all.
#[test]
fn a_pyproject_answers_the_toolchain_gap_with_its_requires_python() {
    let (_directory, root) =
        project_of("pyproject.toml", "[project]\nname = \"t\"\nrequires-python = \">=3.12\"\n");
    assert_eq!(pins(&root).get("pyproject.python").map(String::as_str), Some(">=3.12"));
}

/// A Go module states its language version in the one directive `go.mod` has for it.
#[test]
fn a_go_module_answers_the_toolchain_gap_with_its_go_directive() {
    let (_directory, root) = project_of("go.mod", "module example.com/m\n\ngo 1.23.4\n");
    assert_eq!(pins(&root).get("gomod.go").map(String::as_str), Some("1.23.4"));
}

/// `rustup` reads either spelling of its file, and each may hold either form.
///
/// The table form is why this file cannot be read as a plain pin file: the first line
/// of it is `[toolchain]`, which has the shape of a version without being one.
#[test]
fn either_spelling_of_the_rust_toolchain_file_is_read_in_either_form() {
    for file in ["rust-toolchain", "rust-toolchain.toml"] {
        let (_bare, root) = project_of(file, "1.88.0\n");
        assert_eq!(pins(&root).get("rust").map(String::as_str), Some("1.88.0"), "{file} as a line");

        let (_table, root) = project_of(file, "[toolchain]\nchannel = \"1.88.0\"\n");
        assert_eq!(
            pins(&root).get("rust").map(String::as_str),
            Some("1.88.0"),
            "{file} as a table"
        );
    }
}

/// A table with no channel states no version, and a version is never invented from the
/// text of a table header.
#[test]
fn a_rust_toolchain_table_without_a_channel_pins_nothing() {
    let (_directory, root) =
        project_of("rust-toolchain.toml", "[toolchain]\ncomponents = [\"clippy\"]\n");
    assert_eq!(pins(&root).get("rust"), None);
}

/// Infer over the polyglot fixture: a Rust binary, a Node CLI and a Python tool.
fn polyglot() -> (tempfile::TempDir, Effective) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = nodal_fixture::polyglot::write(directory.path());
    let effective = recipe::load(&root).expect("a readable project");
    (directory, effective)
}

/// A repository of three ecosystems installs with three managers.
///
/// Reading the first lockfile only is what left the Rust and Python halves of such a
/// project cold: the base installed the Node dependencies and nothing else.
#[test]
fn a_polyglot_project_names_every_manager_it_carries() {
    let (_directory, effective) = polyglot();
    assert_eq!(
        effective.recipe.package_manager,
        [PackageManager::Pnpm, PackageManager::Cargo, PackageManager::Uv]
    );
    assert_eq!(effective.recipe.package_manager(), Some(PackageManager::Pnpm));
}

/// Two lockfiles of one ecosystem are a repository mid-way through changing manager.
/// Installing with both would write two dependency trees over each other.
#[test]
fn only_one_manager_of_an_ecosystem_is_proposed() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = nodal_fixture::polyglot::write(directory.path());
    std::fs::write(root.join("package-lock.json"), "{}\n").expect("a second node lockfile");

    let effective = recipe::load(&root).expect("a readable project");
    assert_eq!(
        effective.recipe.package_manager,
        [PackageManager::Pnpm, PackageManager::Cargo, PackageManager::Uv],
        "npm was proposed beside pnpm"
    );
}

/// Every half of a polyglot project states its own toolchain, in its own file.
#[test]
fn a_polyglot_project_states_a_toolchain_for_every_half() {
    let (_directory, effective) = polyglot();
    let pinned: BTreeMap<String, String> =
        effective.recipe.toolchain.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for (key, version) in [
        ("engines.node", "22.11.0"),
        ("cargo.rust", "1.88"),
        ("rust", "1.88.0"),
        ("pyproject.python", ">=3.12"),
        ("gomod.go", "1.23.4"),
    ] {
        assert_eq!(pinned.get(key).map(String::as_str), Some(version), "{key}");
    }
}

/// The commands come from whichever half states them, and the Node half owns the names
/// its own script table declares.
#[test]
fn a_polyglot_project_takes_each_command_from_the_half_that_states_it() {
    let (_directory, effective) = polyglot();
    let commands = &effective.recipe.commands;
    let line = |value: &Option<nodal_core::model::recipe::CommandLine>| {
        value.as_ref().map(ToString::to_string)
    };
    assert_eq!(line(&commands.dev).as_deref(), Some("pnpm run dev"), "the Node half declares dev");
    assert_eq!(line(&commands.lint).as_deref(), Some("pnpm run lint"), "and lint");
    assert_eq!(
        line(&commands.build).as_deref(),
        Some("cargo build"),
        "no script declares build, so the Rust half fills it"
    );
    assert_eq!(line(&commands.test).as_deref(), Some("cargo test"), "and test");
}

/// Python states nothing by convention, so every Python command needs the project to
/// have named the tool it runs.
#[test]
fn a_python_command_is_proposed_only_where_the_project_names_the_tool() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    std::fs::write(root.join("uv.lock"), "version = 1\n").expect("a lockfile");
    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"t\"\ndependencies = []\n")
        .expect("a manifest");
    let bare = recipe::load(root).expect("a readable project");
    assert_eq!(bare.recipe.commands.test, None, "pytest is not declared");
    assert_eq!(bare.recipe.commands.lint, None, "no tool section names a linter");

    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"t\"\ndependencies = [\"pytest>=8\"]\n\n[tool.mypy]\nstrict = true\n",
    )
    .expect("a manifest");
    let named = recipe::load(root).expect("a readable project");
    assert_eq!(
        named.recipe.commands.test.as_ref().map(ToString::to_string).as_deref(),
        Some("uv run pytest")
    );
    assert_eq!(
        named.recipe.commands.typecheck.as_ref().map(ToString::to_string).as_deref(),
        Some("uv run mypy")
    );
    assert_eq!(named.recipe.commands.lint, None, "mypy is not a linter");
}

/// `cargo clippy` is a component a toolchain may not have, so it is proposed only where
/// the tree states that this project lints with it.
#[test]
fn a_cargo_lint_command_needs_the_project_to_state_clippy() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    std::fs::write(root.join("Cargo.lock"), "version = 3\n").expect("a lockfile");
    std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"c\"\nversion = \"0.1.0\"\n")
        .expect("a manifest");
    assert_eq!(recipe::load(root).expect("a project").recipe.commands.lint, None);

    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n\n[lints.clippy]\npedantic = \"deny\"\n",
    )
    .expect("a manifest");
    assert_eq!(
        recipe::load(root)
            .expect("a project")
            .recipe
            .commands
            .lint
            .map(|l| l.to_string())
            .as_deref(),
        Some("cargo clippy")
    );
}
