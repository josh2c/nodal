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
│   └── startup/               spawns a binary N times, reports the median, gates on it
├── schemas/                   generated JSON schemas (committed, diffed in CI)
├── shims/                     tiny shell scripts: PATH shims, per-unit git hooks, rc hook, .envrc template
├── tests/                     workspace-level integration and safety suites
│   ├── fixture/               generator for the small pnpm+migrations project
│   ├── safety/                the test kit every suite runs on, and one test per interference row
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
  port.rs              PortBlock, PortAllocation
  event.rs             Event, EventKind, Epistemic
  recipe.rs            Recipe (nodal.toml) types
  fingerprint.rs       WorkspaceFp, SchemaFp, SubFp (types only)
  manifest.rs          Manifest (.nodal/manifest.toml)
  trash.rs             Trashed (one reclaimed home) and its retention arithmetic
  bundle.rs            Bundle envelope

store/                 SQLite (WAL); repository functions; no business rules
  mod.rs               Store::open, connection pragmas
  migrations.rs        the numbered migration table and the runner
  migrations/          0001_init.sql, 0002_… 0005_tether.sql (plain SQL files, include_str!)
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
  trash.rs             the homes reclaim moved aside and gc will remove
  port_blocks.rs       the block of ports a project hands out from
  port_allocations.rs  one port an environment holds, keyed by the port itself

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
  branches.rs          local branches, the merged set, and the unpushed count of one ref
  push.rs              the one call that leaves this machine: refspecs to a remote
  host.rs              the web host a remote names, and its compare page (a table)
  status.rs            porcelain parsing -> StatusSummary
  history.rs           log and name-status parsing: the commits and files of a range
  merge.rs             commit, squash, rebase, fast-forward; the fast-forward rule
  scrub.rs             post-clone scrub (remove worktrees dir, set HEAD, gc.auto, hooks)
  snapshot.rs          the work-in-progress commit, built in a temporary index
  preflight.rs         refuse in-progress state

substrate/
  mod.rs               Layout (bases and homes are separate roots) + the project row
  bases.rs             ensure(fp) -> Base; neighbour selection; pins; evict and gc
  build.rs             build a base as a Plan: clone, checkout, install, warm
  lru.rs               eviction policy (pure)
  progress.rs          Reporter: where a build says what it is doing
  templates.rs         ensure(schema_fp) -> Template; incremental from parent

workspace/
  mod.rs               Materializer trait + select_backend()
  exclude.rs           default list + recipe excludes (data)
  apfs.rs              clonefile FFI, filtered walk
  reflink.rs           Linux FICLONE walk
  btrfs.rs             subvolume snapshot
  copy.rs              fallback
  sharing.rs           whether nodal shares blocks at the state root, as init and doctor report it
  relocate.rs          CacheRelocator trait + InvalidateCache (rewrite reserved, not built)
  home.rs              where Nodal keeps its state, and where a home goes inside it (pure paths)

services/
  mod.rs               ServiceAdapter trait; registry of adapters by recipe kind
  ports.rs             allocator + fixed-port check
  listeners.rs         which granted ports are really bound (/proc/net/tcp*, Linux)
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

doctor/                what this machine has left behind; reads only, never removes (DL-015)
  mod.rs               survey(): the sections, and which one a path belongs to
  size.rs              logical bytes and newest modification of a directory (no writes)
  attribution.rs       whose a Docker resource is when its name is the only evidence
  branches.rs          local branches with no worktree, in three buckets by where their commits are
  worktrees.rs         checkouts from `git worktree list`; locked and prunable are read no further
  caches.rs            build caches nothing has written to for a fortnight (table of names)
  containers.rs        exited containers and unreferenced volumes, attributed like ps does
  databases.rs         directories named as a Nodal database with no registry row
  units.rs             a project over the open-unit threshold, and the disk its homes hold
  intent.rs            the first prompt of the Claude Code session that made a worktree

env/
  mod.rs               activate(env) -> Vec<EnvVar> (pure assembly)
  files.rs             write .nodal/env, .envrc, manifest; hide via info/exclude
  secrets.rs           SecretSource trait + FileSource; resolution order; missing report
  vars.rs              NODAL_* constants (one place)

runtime/
  mod.rs
  shells.rs            the shells Nodal speaks: names, rc files, assignment dialects (data)
  init.rs              shell-init: render one of shims/nodal.{bash,zsh,fish}
  entry.rs             which home a target names; the NODAL_CD_FILE channel
  shell.rs             become the user's shell with the home's env (exec, no child)
  show.rs              one unit in full: the list's row for it, with its log
  explain.rs           why a home is as it is: its base, what it did not receive, what was removed
  run.rs               run a command, record observed event, redact values; --tether's process group
  actor.rs             who is running this: NODAL_ACTOR, then a table of agent signals
  processes.rs         Processes trait; /proc scan for the variables a process carries
  sessions.rs          derive sessions from processes; reconcile registry rows
  attribute/           Attributor trait; one file per signal
    process_env.rs · cwd.rs · docker.rs · listeners.rs
  ps.rs                merge signals into Attributed rows with confidence
  stop.rs              Signals trait; a process or a whole group; SIGINT, SIGTERM, then SIGKILL

context/               WORKUNIT.md: recomputed from the project, never from the last copy
  mod.rs               refresh(): one survey, one file per unit, and the notes
  survey.rs            one pass over the registry and the homes: the facts of every unit
  ledger.rs            what every other open unit did, and what the base gained
  render.rs            the Markdown, the caps, and the flattening of an event body
  pointer.rs           the one line in CLAUDE.md and AGENTS.md
  atomic.rs            beside-and-rename
  capture.rs           Capture writer (store + jsonl), used by shims/hooks via CLI
  rules.rs             default agent rules text

setup/                 what nodal puts on a machine outside its state, and how it takes it back
  mod.rs
  rc.rs                the marked block in a start-up file; add and remove are byte inverses
  shims.rs             the shell script on disk: <state>/shims/nodal.<shell>
  plan.rs              install; survey what an uninstall removes, then remove exactly that
  channel.rs           which channel installed this binary, and the command that upgrades it

adapters/               what nodal writes into somebody's repository for another tool
  mod.rs               the two rules every adapter keeps: clobber nothing, and be reversible
  settings.rs          .claude/settings.json: splice one region in, take exactly it out
  claude_code.rs       the four hooks, the provider contract, and the payload
  codex.rs             AGENTS.md pointer (not yet written)
  generic.rs           (not yet written)

lifecycle/             the only module that composes others; each op = plan() pure + apply() IO
  mod.rs               run(plan) and resolve(): the runner, and what the next command does
  step.rs              Step trait {key, apply -> Output, undo}; Plan = steps + the final registry write
  journal.rs           operation and operation_step rows: op id, step key, state, output
  owner.rs             whose run an operation is, and whether that process is still there
  guard.rs             the placement rule: a home never overlaps a source, a project or another home
  marker.rs            .nodal/id: write, read, verify against the registry
  ops/
    new.rs · adopt.rs · sync.rs · merge.rs · reclaim.rs · gc.rs · done.rs · transfer.rs · doctor.rs
  uniqueness.rs        the single uniqueness_check
  hooks.rs             recipe hooks: the six phases, the context, and approval by digest
  template.rs          the five values a hook command may name, and the substitution
  states.rs            transition tables as data (unit, environment, session)
  idle.rs              idle detection (pure over timestamps + process list)

output/
  mod.rs               Render trait: human + json; every read type implements it
  human.rs             Doc/Block/Table layout: the one place that decides alignment
  json.rs              pretty for --json, compact for one line of a stream
  watch.rs             Source trait + polling loop; writes only changed answers
  view/                the read types themselves, one file per command family
    unit.rs · status.rs · event.rs · base.rs · init.rs · env.rs · created.rs
    reclaim.rs · doctor.rs · merge.rs · done.rs · explain.rs · setup.rs
```

## crates/nodal-cli/src

```
main.rs                clap parse → dispatch; nothing else
cli.rs                 the clap derive tree (one enum)
commands/              one file per command, each ≤ 40 lines: parse args → call core → render
  init.rs · new.rs · cd.rs · adopt.rs · ls.rs · show.rs · explain.rs · shell.rs · shell_init.rs
  claude_code.rs       one subcommand per Claude Code hook: payload in, the contract out
  run.rs · ps.rs · start.rs
  note.rs · ask.rs · handoff.rs · sync.rs · done.rs · merge.rs · reclaim.rs · gc.rs · doctor.rs
  base.rs · db.rs · status.rs · push.rs · pull.rs · open.rs · uninstall.rs · upgrade.rs
```

## Rules the linter enforces (workspace lints, CI fails on any)

- `clippy::cognitive_complexity` threshold 12; `clippy::too_many_lines` threshold 80; `too_many_arguments` 5.
- `clippy::pedantic` on, with a short, documented allow-list; `clippy::unwrap_used` and `expect_used` denied
  outside tests; `missing_docs` on public items of `nodal-core`.
- No `match` nesting deeper than two: extract a function or use a table.
- Every apply step is idempotent and has an undo; the runner journals steps and finalizes the registry in one transaction.
  The work an operation does inside a home is therefore steps of that operation, not one helper that does all of it:
  a helper would hide the seam the journal needs, which is one key and one undo per thing that changed.
- No plan holds a resolved secret value. A plan is rebuilt from the journal, which is a table in the registry, so a
  step that needs a value asks its source when it applies rather than carrying one.
- A step that learns something the registry write needs returns it. The runner writes that value into the step's
  own journal row in the statement that records the step as applied, and hands the whole set to the commit
  (`Commit = Fn(&Transaction, &Outputs) -> Result<Output>`). This is what makes a run finished by a second process
  the same run: that process never ran the steps, and reads what they produced out of the journal.
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
- One test kit, in `tests/safety/src`: the runner for the binary (`runner`), the runner for `git`
  (`git`), the two streams of a finished command (`text`), the project to make units in (`project`),
  the registry rows a test writes by hand (`rows`), and what a state directory answers (`state`).
  A suite that needs one of those asks the kit; it never writes a second copy. The two crates that
  depend on the kit are the two it depends on, so the edge back is a dev-dependency.
