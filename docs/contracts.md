# Contracts

Stable surfaces that other tools can rely on. Changing one requires a documented decision and a version bump.

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
Workspace: lockfiles, package manifests, `.npmrc`, toolchain pins, Dockerfile and Compose files,
`nodal.toml`, platform triple. Schema: migrations directory tree, database config, seed. Bases use tree
object ids at a commit; staleness uses working-tree file hashes.
