# Code structure and complexity rules

Goal: a tree a new engineer, human or AI, can navigate in ten minutes. Every file has one job.
Branching lives in small pure functions. The linter refuses complexity; a reviewer does not have to.

## Workspace

```
nodal/
├── Cargo.toml                 workspace: members, shared deps, [workspace.lints]
├── clippy.toml                cognitive-complexity-threshold = 12, too-many-lines-threshold = 80
├── rustfmt.toml
├── deny.toml                  licenses, advisories, duplicate deps
├── .github/workflows/ci.yml   fmt --check · clippy -D warnings · test · deny · schema diff
├── README.md
├── docs/                      contracts, code structure, scenarios
├── benches/                   startup and materialization benchmarks with recorded results
├── schemas/                   generated JSON schemas (committed, diffed in CI)
├── shims/                     tiny shell scripts: PATH shims, per-unit git hooks, rc hook, .envrc template
├── tests/                     workspace-level integration and safety suites
│   ├── fixture/               generator for the small pnpm+migrations project
│   ├── safety/                one test per interference row
│   └── e2e/                   new → shell → sync → reclaim on the fixture
└── crates/
    ├── nodal-core/
    └── nodal-cli/
```

## crates/nodal-core/src — one directory per module, one file per concern

```
lib.rs                 pub use of module facades only; no logic
error.rs               one `Error` enum (thiserror), one `Result<T>`; variants per module, no strings-as-errors

model/                 plain data, serde + schemars; zero IO
  mod.rs               re-exports
  ids.rs               UnitId (ulid), EnvId, BaseId, TemplateId newtypes
  unit.rs              Unit, UnitStatus
  environment.rs       Environment, EnvState, Ports
  event.rs             Event, EventKind, Epistemic
  recipe.rs            Recipe (nodal.toml) types
  fingerprint.rs       WorkspaceFp, SchemaFp, SubFp (types only)
  manifest.rs          Manifest (.nodal/manifest.toml)
  bundle.rs            Bundle envelope

store/                 SQLite (WAL); repository functions; no business rules
  mod.rs               Store::open, connection pragmas
  migrations.rs        the numbered migration table and the runner
  migrations/          0001_init.sql, 0002_… (plain SQL files, include_str!)
  row.rs               model values in and out of columns, one place
  projects.rs          insert/get/find/list/update (one fn each)
  units.rs             insert/get/list/update_status (one fn each)
  environments.rs
  events.rs
  bases.rs
  templates.rs
  sessions.rs
  leases.rs
  locks.rs

recipe/
  mod.rs               load(): parse nodal.toml, merge inferred, validate
  infer/               one file per inference source, each `fn infer(root) -> Partial`
    package_manager.rs · scripts.rs · toolchain.rs · migrations.rs · services.rs · env_names.rs · backend.rs
  merge.rs             precedence: explicit > inferred; produces Recipe + Gaps

fingerprint/
  mod.rs               compute_workspace(tree) / compute_schema(tree)
  inputs.rs            which paths feed which sub-fingerprint (data table, not code)
  tree.rs              TreeSource trait: GitTreeAtCommit, WorkingTree

git/
  mod.rs               Git facade struct wrapping `git` CLI invocations
  cmd.rs               run(args) -> Output, one place for process spawning
  refs.rs              read/write refs, WIP snapshot ref
  status.rs            porcelain parsing -> StatusSummary
  scrub.rs             post-clone scrub (remove worktrees dir, set HEAD, gc.auto, hooks)
  preflight.rs         refuse in-progress state

substrate/
  mod.rs               Substrate facade
  bases.rs             ensure(fp) -> Base; neighbour selection; pin/unpin
  build.rs             build a base: clone, install, warm (each step a fn)
  lru.rs               eviction policy (pure)
  templates.rs         ensure(schema_fp) -> Template; incremental from parent

workspace/
  mod.rs               Materializer trait + select_backend()
  exclude.rs           default list + recipe excludes (data)
  apfs.rs              clonefile FFI, filtered walk
  reflink.rs           Linux FICLONE walk
  btrfs.rs             subvolume snapshot
  copy.rs              fallback
  relocate.rs          CacheRelocator trait + InvalidateNextCache (+ optional rewrite)
  create.rs            create_home(base, home): clone → scrub → checkout (calls above, no branching on backend)

services/
  mod.rs               ServiceAdapter trait; registry of adapters by recipe kind
  ports.rs             allocator + fixed-port check
  postgres/
    mod.rs             adapter impl
    conn.rs            exec-in-container vs tcp (one enum, two impls)
    template.rs        build/freeze/create-from/drop
    tracking.rs        migration tool tracking rows (supabase, prisma) as data-driven inserts
  supabase/
    mod.rs             composes postgres + api layer
    stack.rs           detect local stack from config.toml / supabase status
    api_layer.rs       per-unit postgrest + gotrue + router containers
  docker.rs            thin wrapper: run/rm/inspect/label queries

env/
  mod.rs               activate(env) -> Vec<EnvVar> (pure assembly)
  files.rs             write .nodal/env, .envrc, manifest; hide via info/exclude
  secrets.rs           SecretSource trait + FileSource; resolution order; missing report
  vars.rs              NODAL_* constants (one place)

runtime/
  mod.rs
  shell.rs             spawn user shell with env + rc hook; session row
  run.rs               run a command, record observed event
  attribute/           Attributor trait; one file per signal
    process_env.rs · cwd.rs · docker.rs · listeners.rs
  ps.rs                merge signals into Attributed rows with confidence

context/
  mod.rs
  capture.rs           Capture writer (store + jsonl), used by shims/hooks via CLI
  compile/             pack compiler; each section its own pure fn over inputs
    mod.rs · state.rs · commands.rs · tests.rs · stated.rs · render.rs
  rules.rs             default agent rules text

adapters/
  mod.rs               AgentAdapter trait
  claude_code.rs       hooks json, session start/stop
  codex.rs             AGENTS.md pointer
  generic.rs

lifecycle/             the only module that composes others; each op = plan() pure + apply() IO
  mod.rs               run(plan) and resolve(): the runner, and what the next command does
  step.rs              Step trait {key, apply, undo}; Plan = steps + the final registry write
  journal.rs           operation and operation_step rows: op id, step key, state
  owner.rs             whose run an operation is, and whether that process is still there
  ops/
    new.rs · adopt.rs · sync.rs · reclaim.rs · gc.rs · done.rs · transfer.rs · doctor.rs
  uniqueness.rs        the single uniqueness_check
  states.rs            transition tables as data (unit, environment, session)
  idle.rs              idle detection (pure over timestamps + process list)

output/
  mod.rs               Render trait: human + json; every read type implements it
  human.rs             Doc/Block/Table layout: the one place that decides alignment
  json.rs              pretty for --json, compact for one line of a stream
  watch.rs             Source trait + polling loop; writes only changed answers
  view/                the read types themselves, one file per command family
    unit.rs · status.rs · event.rs · base.rs · init.rs
```

## crates/nodal-cli/src

```
main.rs                clap parse → dispatch; nothing else
cli.rs                 the clap derive tree (one enum)
commands/              one file per command, each ≤ 40 lines: parse args → call core → render
  init.rs · new.rs · adopt.rs · ls.rs · show.rs · explain.rs · shell.rs · run.rs · start.rs
  note.rs · ask.rs · handoff.rs · sync.rs · done.rs · reclaim.rs · gc.rs · doctor.rs
  base.rs · db.rs · status.rs · push.rs · pull.rs · open.rs · uninstall.rs
```

## Rules the linter enforces (workspace lints, CI fails on any)

- `clippy::cognitive_complexity` threshold 12; `clippy::too_many_lines` threshold 80; `too_many_arguments` 5.
- `clippy::pedantic` on, with a short, documented allow-list; `clippy::unwrap_used` and `expect_used` denied
  outside tests; `missing_docs` on public items of `nodal-core`.
- No `match` nesting deeper than two: extract a function or use a table.
- Every apply step is idempotent and has an undo; the runner journals steps and finalizes the registry in one transaction.
- An operation is journalled before and after every step, so a process killed between two steps leaves an
  accurate account. The registry write and the journal's move to `committed` share that one transaction, so
  an operation is either wholly done or wholly not. The next command calls `lifecycle::resolve`, which
  rebuilds an interrupted operation's plan from the journal (`lifecycle::Rebuild`, one per op kind, passed
  in as a table) and either resumes or rolls it back as the plan declared.
- Plan/apply split in every lifecycle op: `plan()` returns a `Plan` value (pure, unit-testable),
  `apply(plan)` performs IO step by step and records each step's outcome. Branching lives in `plan()`.
- Backend choice happens once (`select_backend`), never inside operations.
- Data over code: exclusion lists, fingerprint inputs, state transitions, migration tracking rows are tables.
- One process-spawning function per external tool (`git::cmd::run`, `services::docker`), so mocking is one seam.
- Tests: unit tests beside code for pure functions; integration tests in `tests/` against the fixture;
  safety suite is its own directory and is a required CI job.
