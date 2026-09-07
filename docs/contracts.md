# Contracts

Stable surfaces that other tools can rely on. Changing one requires a documented decision and a version bump.

Machine-readable form: the types behind these surfaces are published as JSON Schema in
`schemas/v1/`, generated from the model and diffed in CI (`schemas/README.md`).

## Directory contract
A unit's home contains `.nodal/id` (marker, verified against the registry), `.nodal/env` (dotenv),
`.nodal/manifest.toml`, `WORKUNIT.md` (facts about the unit and its siblings), `.envrc` (`dotenv .nodal/env`),
and a normal `.git` directory. Nothing else is required for a terminal, IDE or agent to integrate.

Nodal adds `.nodal/` and `.envrc` to `.git/info/exclude`, so `git status` in a unit stays clean.

`.nodal/manifest.toml` states the identity of the home, every environment name it carries, the
origin of each name, and every declared name that no source answered. It holds no value. Its shape
is published as `schemas/v1/manifest.json`.

## Environment variables
`NODAL_ID`, `NODAL_UNIT` (slug), `NODAL_PROJECT`, `NODAL_HOST`, `NODAL_ROOT`, plus recipe-declared
generated variables (`PORT`, `APP_URL`, service URLs). Every process started in an activated home
carries them, which is what makes attribution and session state readable.

A shell reads them by one of two routes. direnv reads `.envrc`, which reads `.nodal/env`. A shell
with no direnv evaluates `nodal env --export`, which is what the hook `nodal shell-init` installs
does. Nodal spawns no subshell for either route.

The rc hook is the default route, because it needs no second program and no per-directory approval.
The `.envrc` stays and is always written: an IDE with a direnv extension activates a terminal from
it with no shell integration at all. The two compose. The hook does nothing in a home direnv has
already activated, because `NODAL_ROOT` is then already the home.

`nodal env --export` ends with `NODAL_EXPORTED`, which names every variable those lines set. A
prompt hook unsets exactly those on the way out of a home, so it removes what Nodal added and never
what a person exported. `--shell fish` renders the same set as `set -gx` assignments.

`NODAL_CD_FILE` names a file a waiting shell reads a directory from. A command that names a
directory writes the path there and prints it on standard output. The shell function makes the file;
a shell without the function gets the path and nothing changes for it.

## Secrets
A recipe declares names. It never holds a value. Three tiers supply the values, and Nodal asks them
in this order:

1. the unit itself, for a value it minted for its own services;
2. the per-machine file `~/.nodal/secrets.env`;
3. nothing, in which case the name is a line of the report.

A unit-generated value wins over a machine-wide value of the same name, because only the unit-
generated value is bound to the unit's own resources. `NODAL_SECRETS_FILE` moves the per-machine
file, as `NODAL_STORE` moves the registry.

Nodal creates `~/.nodal/secrets.env` with mode `0600`. Nodal refuses to read the file when its mode
gives any access to group or other, and reports the mode alone.

A name that no tier answers is a line of the report. It never stops a unit from being created.

A secret value goes into `.nodal/env` and into the output of `nodal env --export`. It goes nowhere
else: not into a manifest, a bundle, a log line, an error message or `--json` output.

## Home path policy
`~/.nodal/<project>/e/<id>/`, equal length for every unit of a project. `<id>` is the last
eight characters of the environment's identifier: they are the random part rather than the
part derived from the clock, so two units created in the same moment do not collide.
`NODAL_HOME` moves the whole directory, as `NODAL_STORE` moves the registry inside it; the
registry's default path is `registry.db` in that directory. A project keeps its homes under `e/`,
its bases under `b/` and its reclaimed homes under `trash/`, so a walk of one cannot reach
another.

A home is refused where it would overlap a tree Nodal already knows: the project it is
cloned from, another project's root, or another unit's home. A home inside its own project
would make the next clone copy a copy.

## `nodal.toml`
`backend`, `package_manager`, `commands.{dev,build,test,migrate,seed}`, `toolchain`, `db.{kind,url_var}`,
`services.{shared,per_unit}`, `env.{required_local,generated,secrets}`, `base.{exclude,invalidate}`,
`hooks.{pre_new,post_new,pre_reclaim,post_reclaim}`, `sync.auto_irreversible`, `reclaim.trash_retention`
(in days). Alongside those, and additive to them: `package_manager_pin`, `monorepo`, `task_cache`,
`dockerfile`, `compose`, `commands.{lint,typecheck,reset}`, `db.{tool,migrations_dir,fixed_ports}`.

Every key is optional and unknown keys are rejected, so a typo is a message rather than a line silently
ignored. Most keys are inferred by `nodal init` from the project's own files; only the gaps need a human
line, and `init` writes each gap as a comment above the empty key it belongs to. Published as
`schemas/v1/recipe.json`.

## CLI
`init, new, cd, adopt, ls, show, explain, env, shell, shell-init, run, ps, start, note, ask, handoff,
sync, done, merge, prune, reclaim, gc, doctor, base, status`. Every read command accepts `--json`; `status --watch`
emits newline-delimited JSON. Global `--store` and `--no-hooks`.

`--json` and the default output are two renderings of one value, so a field a person sees is a field a
tool can read. A read type carries the instant it was taken as `now`, and every relative time it prints
is measured from that, so a rendering is a function of its inputs.

`status --watch` polls; there is no daemon. One line is one whole `status` document, identical in shape
to `status --json`, and a line is written only when the answer has changed — the instant moving on its
own is not a change. A consumer therefore holds the last line as current state, and silence means
unchanged rather than gone.

## The list
`nodal ls`, and `nodal` with no subcommand, answer with one row per unit of the project the
working directory is in. The command reads. It opens no transaction, records no event and
reconciles no session row.

Each row carries what Git says about the unit's branch at the moment it was asked: how many
paths are changed, staged and untracked; whether HEAD names a commit rather than a branch; how
far the branch has moved from the branch it merges into; what the upstream on the remote has
and what it does not; and one integration verdict.

The verdict has four values. `integrated` means the base carries every change of the branch,
and it names one of two reasons. `ancestor` means the branch tip is in the base's history.
`absorbed` means the base carries the changes without the commits, which is what a squash
merge and a rebase leave. `conflict` means merging the branch into the base would leave
conflicts. `open` means the branch carries changes the base does not, and the merge is clean.
`unknown` means Git could not answer, and there is then a note under the table.

The verdict is read from trees, with `git merge-tree`, and never from the commit history. A
unit whose work is on the base is finished however it got there, so a squash merge counts as
done although no commit of the unit is on the base.

The order is the answer to "which unit needs a person next". Units the base carries already
are last. The rest come first, the one the base has moved furthest under at the top. Units
that tie are ordered by slug, so one list of one registry is always the same list.

Who is attached to each unit comes from the process table, by the same signals `nodal ps`
reads. A host whose process table Nodal cannot read still lists every unit and says under the
table that it could not see.

## Entry
`nodal new` and `nodal cd` print a home path. `nodal shell-init <bash|zsh|fish>` prints a shell
function and a prompt hook; the function turns those two printed paths into a directory change in
the shell a person is already in. `nodal shell` replaces its own process with the shell, for a
script, a remote host or a terminal with no integration. Nodal starts no shell under another one and
asks nothing when a shell ends.

Sessions are derived, not declared: a process carrying `NODAL_ID` is attached to that unit, and a
session ends when the process is gone. Nothing has to be run on entry or on exit.

## Attribution
`nodal ps` answers what is running on this host and which unit each thing belongs to. Every row
carries a confidence, and there are two levels. `certain` means the thing named its unit: a process
carrying `NODAL_ID`, a container carrying the `nodal.unit` label. `probable` means Nodal inferred
the unit. Three readings are probable: a process whose working directory is inside a home, a
container that mounts a home, and a granted port that has a listener. A row carries no other level.

Nodal labels every container it starts with `nodal.unit` (the unit identifier) and
`nodal.environment` (the materialisation). These are the container half of the environment-variable
contract above.

A signal that cannot run gives a note under the table. It is never a failure. A host with no Docker
daemon still answers, and so does a host whose process table Nodal cannot read. An empty answer means
nothing runs. A note means Nodal could not read that signal.

## Hooks
Recipe hooks receive `NODAL_SOURCE` (the project checkout), `NODAL_ROOT` (the unit's home),
`NODAL_ID`, `NODAL_UNIT`, `NODAL_ENV`, and `NODAL_PARENT_ID`. `NODAL_PARENT_ID` names the base the
home was cloned from. It is empty for a checkout adopted in place.

Hooks do not receive `NODAL_HOME`. That variable moves Nodal's whole state directory. A hook that
set it and then ran `nodal` would write into the unit's home. `NODAL_ROOT` names the home, as it
does in an activated shell.

Each hook runs in a directory that exists. `pre_new` runs in the project root, before the home is
made. `post_new` runs in the home. `pre_reclaim` runs in the home. `post_reclaim` runs in the
project root, and `NODAL_ROOT` then names the home in the trash.

Hook commands require approval. `nodal init` approves the set the project declares. It pins each
command by the digest of its exact text. The record is per machine, in `<state>/hooks.toml`;
`NODAL_HOOKS_FILE` moves that file. A command that has changed refuses to run, and the message
shows the command. A command nobody approved refuses in the same way. `--no-hooks` runs no hook
and needs no approval.

## Reclaim, trash and gc
Every destructive path calls one uniqueness check. It reports three things: uncommitted changes,
untracked files that no ignore rule covers, and commits that no remote and no other tree on this
machine has. A hit refuses the operation and names the paths. `--force` does not skip the check.
It first commits the whole home to `refs/nodal/<unit>/wip`, then goes on.

A reclaim stops what the unit runs. It gives back the unit's ports and leases in the transaction
that records the reclaim. It moves the home to `<state>/<project>/trash/<id>`, under the name the
home had. It then reads back everything the unit had, by identifier, and reports what is still
there. It does not claim that the machine is clean.

Nodal never trashes two things. A checkout adopted in place is unregistered, and the directory
stays where it is. A base is not a home; `nodal base gc` collects it.

A reclaim reads the two signals `nodal ps` reads: the process table and the container daemon. A
signal Nodal cannot read becomes a note, never silence. A host with no readable process table
still reclaims the home and still gives back the ports. The verification there says that it found
nothing, not that nothing is left, and the note says which signal went unread.

`nodal gc` removes a trashed home when `reclaim.trash_retention` days have passed. Nodal stamps
that window on the row when it moves the home. A recipe edited later cannot shorten a retention
that somebody relies on. `gc` also gives back lapsed leases. It stops runtime that belongs to a
unit whose materialisations have all been reclaimed. It never stops the runtime of a live unit.

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
