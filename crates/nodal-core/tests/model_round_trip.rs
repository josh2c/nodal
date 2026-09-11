//! Acceptance test, first of the two schema halves: every model type survives a JSON
//! round trip, and the JSON it round-trips through is the shape the contracts document
//! names.
//!
//! A sample of each record type is built once here and reused, so a field added to a
//! type without a value here fails to compile rather than going untested.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use nodal_core::model::recipe::{
    Backend, CommandLine, DbKind, EnvName, MigrationTool, PackageManager, ServiceName, TaskCache,
    ToolName, ToolVersion,
};
use nodal_core::model::{
    Actor, ActorKind, ActorName, Base, BaseId, BranchName, CommitId, DbName, DbTemplate, Digest,
    EnvId, EnvState, Environment, Epistemic, Event, EventId, EventKind, HostName, Lease, Lock,
    Objective, Platform, PortName, Ports, Project, ProjectId, ProjectName, RawRef, Recipe, RefName,
    ResourceKey, SchemaFp, Session, SessionId, Slug, TemplateId, Timestamp, Unit, UnitId,
    UnitStatus, WorkspaceFp,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("sample timestamp is valid")
}

fn project_id() -> ProjectId {
    ProjectId::parse("01J8Z6H0000000000000000001").expect("sample id is a ulid")
}

fn unit_id() -> UnitId {
    UnitId::parse("01J8Z6H0000000000000000002").expect("sample id is a ulid")
}

fn env_id() -> EnvId {
    EnvId::parse("01J8Z6H0000000000000000003").expect("sample id is a ulid")
}

fn digest(text: &str) -> Digest {
    Digest::parse(text).expect("sample digest is lowercase hex")
}

fn actor() -> Actor {
    Actor {
        kind: ActorKind::Agent,
        name: ActorName::parse("claude-code").expect("sample actor name is a line"),
    }
}

fn project() -> Project {
    Project {
        id: project_id(),
        root: PathBuf::from("/home/dev/code/example"),
        name: ProjectName::parse("example").expect("sample project name is a line"),
        recipe_hash: digest("9f2c"),
        created_at: at("2026-09-06T09:00:00Z"),
        remote_url: None,
    }
}

fn base() -> Base {
    Base {
        id: BaseId::parse("01J8Z6H0000000000000000004").expect("sample id is a ulid"),
        project_id: project_id(),
        ws_fingerprint: WorkspaceFp(digest("aa01")),
        platform: Platform::parse("x86_64-unknown-linux-gnu").expect("sample triple is a token"),
        commit: CommitId::parse("a".repeat(40)).expect("sample commit is 40 hex characters"),
        path: PathBuf::from("/home/dev/.nodal/example/bases/aa01"),
        built_at: at("2026-09-06T09:05:00Z"),
        last_used: at("2026-09-06T10:00:00Z"),
    }
}

fn db_template() -> DbTemplate {
    DbTemplate {
        id: TemplateId::parse("01J8Z6H0000000000000000005").expect("sample id is a ulid"),
        project_id: project_id(),
        schema_fingerprint: SchemaFp(digest("bb02")),
        db_name: DbName::parse("nodal_example_tpl_bb02").expect("sample db name is an identifier"),
        parent_template_id: None,
        built_at: at("2026-09-06T09:06:00Z"),
    }
}

fn unit() -> Unit {
    Unit {
        id: unit_id(),
        project_id: project_id(),
        slug: Slug::parse("fix-worker-import").expect("sample slug is dash separated"),
        objective: Some(Objective::parse("fix worker import").expect("sample objective is a line")),
        objective_epistemic: Some(Epistemic::Stated),
        branch: BranchName::parse("nodal/fix-worker-import").expect("sample branch is valid"),
        parent_branch: Some(BranchName::parse("main").expect("sample branch is valid")),
        base_commit: None,
        status: UnitStatus::Open,
        created_at: at("2026-09-06T10:00:00Z"),
        updated_at: at("2026-09-06T10:30:00Z"),
    }
}

fn environment() -> Environment {
    let mut ports = BTreeMap::new();
    ports.insert(PortName::parse("app").expect("sample port name is a token"), 31_337);
    Environment {
        id: env_id(),
        unit_id: unit_id(),
        attempt: 1,
        home: PathBuf::from("/home/dev/.nodal/example/e/01j8z6h0/"),
        managed: true,
        base_id: Some(BaseId::parse("01J8Z6H0000000000000000004").expect("sample id is a ulid")),
        ws_fp_materialized: Some(WorkspaceFp(digest("aa01"))),
        schema_fp_materialized: Some(SchemaFp(digest("bb02"))),
        host: HostName::parse("workshop").expect("sample host is a token"),
        db_name: Some(DbName::parse("nodal_example_01j8z6h0").expect("sample db name is valid")),
        ports: Ports(ports),
        fixed_port: None,
        state: EnvState::Running,
        created_at: at("2026-09-06T10:00:02Z"),
        last_active: at("2026-09-06T10:31:00Z"),
    }
}

fn session() -> Session {
    Session {
        id: SessionId::parse("01J8Z6H0000000000000000006").expect("sample id is a ulid"),
        environment_id: env_id(),
        actor: actor(),
        pid: Some(4_242),
        pgid: None,
        started_at: at("2026-09-06T10:00:05Z"),
        ended_at: None,
    }
}

fn event() -> Event {
    let mut refs = BTreeMap::new();
    refs.insert(RefName::parse("commit").expect("sample ref name is a token"), "a".repeat(40));
    Event {
        id: EventId::parse("01J8Z6H0000000000000000007").expect("sample id is a ulid"),
        unit: unit_id(),
        environment: Some(env_id()),
        ts: at("2026-09-06T10:20:00Z"),
        actor: actor(),
        kind: EventKind::TestResult,
        epistemic: Epistemic::Observed,
        body: String::from("14 passed, 1 failed"),
        refs,
        raw_ref: Some(RawRef::parse("runs/01j8z6h0.log").expect("sample raw ref is a line")),
    }
}

fn lease() -> Lease {
    Lease {
        resource: ResourceKey::parse("port:5432").expect("sample resource key is a token"),
        environment_id: env_id(),
        expires_at: at("2026-09-06T11:00:00Z"),
    }
}

fn lock() -> Lock {
    Lock {
        unit_id: unit_id(),
        host: HostName::parse("workshop").expect("sample host is a token"),
        actor: Some(Actor {
            kind: ActorKind::Agent,
            name: ActorName::parse("claude-code").expect("sample actor is a line"),
        }),
        pid: Some(4_120),
        taken_at: at("2026-09-06T09:00:00Z"),
        refreshed_at: at("2026-09-06T10:30:00Z"),
        expires_at: at("2026-09-06T11:00:00Z"),
    }
}

fn recipe() -> Recipe {
    let mut recipe = Recipe {
        backend: Some(Backend::Native),
        package_manager: Some(PackageManager::Pnpm),
        package_manager_pin: Some(ToolVersion::parse("pnpm@9.12.3").unwrap()),
        monorepo: Some(true),
        task_cache: Some(TaskCache::Turborepo),
        dockerfile: Some(PathBuf::from("Dockerfile")),
        compose: vec![PathBuf::from("compose.yml")],
        ..Recipe::default()
    };
    recipe
        .toolchain
        .insert(ToolName::parse("node").unwrap(), ToolVersion::parse("22.11.0").unwrap());
    recipe.commands.test = Some(CommandLine::parse("pnpm run test").unwrap());
    recipe.db.kind = Some(DbKind::SupabaseLocal);
    recipe.db.tool = Some(MigrationTool::Supabase);
    recipe.db.migrations_dir = Some(PathBuf::from("supabase/migrations"));
    recipe.db.url_var = vec![EnvName::parse("DATABASE_URL").unwrap()];
    recipe.db.fixed_ports.insert(PortName::parse("api").unwrap(), 54321);
    recipe.services.shared = vec![ServiceName::parse("db").unwrap()];
    recipe.services.per_unit = vec![ServiceName::parse("postgrest").unwrap()];
    recipe.env.required_local = vec![EnvName::parse("LOG_LEVEL").unwrap()];
    recipe.env.generated = vec![EnvName::parse("PORT").unwrap()];
    recipe.env.secrets = vec![EnvName::parse("RESEND_API_KEY").unwrap()];
    recipe.base.exclude = vec![PathBuf::from(".next")];
    recipe.hooks.post_new = Some(CommandLine::parse("pnpm install").unwrap());
    recipe.sync.auto_irreversible = Some(false);
    recipe.reclaim.trash_retention = Some(14);
    recipe
}

/// Serialise, parse back, and require the value to be unchanged.
fn round_trip<T>(value: &T, label: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).unwrap_or_else(|e| panic!("{label} serialises: {e}"));
    let parsed: T =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{label} parses back: {e}\n{text}"));
    assert_eq!(&parsed, value, "{label} changed across a round trip");
}

#[test]
fn every_record_type_round_trips() {
    round_trip(&project(), "project");
    round_trip(&base(), "base");
    round_trip(&db_template(), "db_template");
    round_trip(&unit(), "unit");
    round_trip(&environment(), "environment");
    round_trip(&session(), "session");
    round_trip(&event(), "event");
    round_trip(&lease(), "lease");
    round_trip(&lock(), "lock");
    round_trip(&recipe(), "recipe");
}

#[test]
fn event_json_uses_the_field_names_the_contract_names() {
    let value = serde_json::to_value(event()).expect("event serialises");
    let object = value.as_object().expect("an event is a JSON object");
    let expected = [
        "id",
        "unit",
        "environment",
        "ts",
        "actor",
        "kind",
        "epistemic",
        "body",
        "refs",
        "raw_ref",
    ];
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut wanted = expected;
    wanted.sort_unstable();
    assert_eq!(keys, wanted, "event fields drifted from docs/contracts.md");
    assert_eq!(object["actor"]["kind"], "agent");
    assert_eq!(object["kind"], "test_result");
    assert_eq!(object["epistemic"], "observed");
    assert_eq!(object["ts"], "2026-09-06T10:20:00Z");
}

#[test]
fn scalars_are_validated_on_the_way_in() {
    let mut value = serde_json::to_value(unit()).expect("unit serialises");
    value["branch"] = serde_json::Value::String(String::from("bad branch name"));
    let error =
        serde_json::from_value::<Unit>(value).expect_err("a branch with a space is not a branch");
    assert!(error.to_string().contains("branch name"), "{error}");

    let mut value = serde_json::to_value(unit()).expect("unit serialises");
    value["id"] = serde_json::Value::String(String::from("not-a-ulid"));
    assert!(serde_json::from_value::<Unit>(value).is_err(), "a non-ULID id must be rejected");
}

#[test]
fn optional_fields_are_absent_as_null_not_omitted() {
    let value = serde_json::to_value(session()).expect("session serialises");
    assert!(value.get("ended_at").is_some(), "an open session still carries the key");
    assert_eq!(value["ended_at"], serde_json::Value::Null);
}
