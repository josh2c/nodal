# Contracts

Stable surfaces that other tools can rely on. Changing one requires a documented decision and a version bump.

Machine-readable form: the types behind these surfaces are published as JSON Schema in
`schemas/v1/`, generated from the model and diffed in CI (`schemas/README.md`).

## Directory contract
A unit's home contains `.nodal/id` (marker, verified against the registry), `.nodal/env` (dotenv),
`.nodal/manifest.toml`, `WORKUNIT.md` (facts about the unit and its siblings), `.envrc` (`dotenv .nodal/env`),
and a normal `.git` directory. Nothing else is required for a terminal, IDE or agent to integrate.

## Environment variables
`NODAL_ID`, `NODAL_UNIT` (slug), `NODAL_PROJECT`, `NODAL_HOST`, `NODAL_ROOT`, plus recipe-declared
generated variables (`PORT`, `APP_URL`, service URLs).

## Home path policy
`~/.nodal/<project>/e/<id>/`, equal length for every unit of a project.

## `nodal.toml`
`backend`, `package_manager`, `commands.{dev,build,test,migrate,seed}`, `toolchain`, `db.{kind,url_var}`,
`services.{shared,per_unit}`, `env.{required_local,generated,secrets}`, `base.exclude`,
`hooks.{pre_new,post_new,pre_reclaim,post_reclaim}`, `sync.auto_irreversible`, `reclaim.trash_retention`.
Most keys are inferred by `nodal init`; only the gaps need a human line.

## CLI
`init, new, adopt, ls, show, explain, shell, shell-init, run, start, note, ask, handoff, sync, done,
merge, prune, reclaim, gc, doctor, base, status`. Every read command accepts `--json`; `status --watch`
emits newline-delimited JSON. Global `--store` and `--no-hooks`.

## Hooks
Recipe hooks receive `NODAL_SOURCE`, `NODAL_HOME`, `NODAL_ID`, `NODAL_PARENT_ID`, `NODAL_UNIT` and run
in the unit's home. Hook commands require approval on first run.

## Event schema
`id, unit, environment, ts, actor {kind, name}, kind, epistemic {observed, stated}, body, refs, raw_ref`.
Kinds: `attached, detached, command, commit, test_result, failure, file_touched, finding, decision,
question, handoff, sync, note`.

## Fingerprint inputs
Two keys, each composed of named parts, so a diff says which part moved. The authoritative list of
paths is the table in `nodal_core::fingerprint::inputs`; this is what it covers.

Workspace key — `toolchain`: version files (`.nvmrc`, `.node-version`, `.tool-versions`,
`mise.toml`, `.python-version`, `.ruby-version`, `.java-version`, `rust-toolchain.toml`).
`dependencies`: lockfiles and package manifests at any depth (`package.json`, `pnpm-lock.yaml`,
`package-lock.json`, `yarn.lock`, `bun.lock`, `Cargo.toml`/`Cargo.lock`, `go.mod`/`go.sum`,
`pyproject.toml`, `poetry.lock`, `uv.lock`, `requirements.txt`, `Gemfile.lock`, `composer.lock`)
and the package manager's configuration (`.npmrc`, `.yarnrc.yml`, `pnpm-workspace.yaml`).
`services`: `Dockerfile*`, `docker-compose*`, `compose.y[a]ml`, `.dockerignore`, the database
config. `recipe`: `nodal.toml` and the monorepo task-graph and task-cache files it is inferred
from (`turbo.json`, `nx.json`, `lerna.json`) plus the declared environment file (`.env.example`).
The platform triple is part of this key.

Schema key — `schema`: the migrations directory tree (`supabase/migrations`, `prisma/migrations`,
`db/migrations`, `db/migrate`, `migrations`), the database config, the schema definition and the
seed. The platform triple is deliberately not part of this key: a template is the same database on
any host. The database config feeds both keys, so either moving is correct.

Inputs are `(path, mode, object id)` triples, never file contents: Git has already hashed the
contents. Bases use tree object ids at a commit, read with one `git ls-tree`; staleness uses
working-tree file hashes, which must be Git blob object ids so that a clean checkout of a commit
and the commit itself fingerprint identically.
