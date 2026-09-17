# Contracts

Stable surfaces that other tools can rely on. Changing one requires a documented decision and a version bump.

Machine-readable form: the types behind these surfaces are published as JSON Schema in
`schemas/v1/`, generated from the model and diffed in CI (`schemas/README.md`).

## Directory contract
A unit's home contains `.nodal/id` (marker, verified against the registry), `.nodal/env` (dotenv, no secret),
`.nodal/manifest.toml`, `WORKUNIT.md` (facts about the unit and its siblings), `.envrc` (`dotenv .nodal/env`, then `nodal env --export`),
and a normal `.git` directory. Nothing else is required for a terminal, IDE or agent to integrate.

### What Nodal writes into a home, and the one rule for all of it

**Nodal writes one of its own files only where Git does not track it, hides it in the home's
`info/exclude` when it wrote it, and never touches it when the project tracks it.** A tracked file
arrived with the clone, so it is the project's. Writing in one would put the home in `git status`
from the moment it exists, and a home that is dirty at birth is one `nodal reclaim`, `nodal done` and
`nodal gc` refuse and one whose pull request carries Nodal's rewrite.

| file | what it is | hidden | what `nodal reclaim` takes back |
|---|---|---|---|
| `.nodal/env` | activation | always | the file |
| `.envrc` | activation | always | the file |
| `.nodal/manifest.toml` | activation | always | the file |
| `.nodal/id` | marker | always | the file, by its own step |
| `WORKUNIT.md` | memory | always | the file |
| `CLAUDE.md` | pointer | where Nodal created it | the one line, and the file when that was all of it |
| `AGENTS.md` | pointer | where Nodal created it | the one line, and the file when that was all of it |
| `.claude/settings.json` | Claude Code settings | where Nodal wrote it | Nodal's hooks, and the file when they were all of it |

A project that commits its own `.envrc` — direnv and Nix users do — keeps it: Nodal writes
`.nodal/env`, which the committed `.envrc` reads, and states in one line that it left the tracked file
alone. Such a home gets its identity and its generated values from direnv and its secrets from the
shell hook, because the committed file holds no line that resolves them. The same holds for every
other row.

Every worktree of a repository shares one exclude file — a linked worktree's own
`.git/worktrees/<name>/info/exclude` is not read — so a unit adopted in a nested worktree writes the
block into the repository the project shares. The names are Nodal's own.

`.nodal/manifest.toml` states the identity of the home, every environment name it carries, the
origin of each name, and every declared name that no source answered. It holds no value. Its shape
is published as `schemas/v1/manifest.json`.

## Stand-in values
A name under `env.generated` that no adapter answers gets a stand-in at create. Nodal derives the
value from the unit's handle and a port of the project's block. The value has the shape the name
asks for. A generate step that reads the name and parses the value runs. No service answers on the
value.

The manifest marks such a name with the origin `stand_in`. `nodal env` marks it in the origin column
and in one line under the table. `nodal explain` states where the value came from. The unit's memory
states the same. The create names every stand-in it made.

A name whose shape asks for a bare port takes no stand-in. A port a process binds is granted, never
derived. Such a name stays on the missing list.

`env.stand_in` pins the template for one name. Nodal fills `{slug}` with the unit's handle. Nodal
fills `{port}` with the derived port. An adapter that produces the name replaces the stand-in at the
next activation.

## The unit's memory
`WORKUNIT.md` in a unit's home states what the unit is and what changed around it. The next agent,
terminal or person reads it to continue the work. Every nodal command that touches a unit writes the
file again. `nodal show` writes it on demand.

Nodal compiles the file from the registry and from Git. The last copy of the file is never an input to
the next one. A session that ends with no handoff therefore loses no fact, because no fact in the file
came from that session.

The file has three sections, in this order.

**Facts** states what is true now:

- the objective, the state, the branch and the home;
- the base revision, and the commit where the branch left it;
- how far the base moved under the branch, and what a merge would do;
- what the working tree holds, and which files the branch changed against its base commit;
- the branch's commits;
- the last commands, from the event log;
- the last test result. A result states counts when an event carries them. If no event carries them,
  the line states what the recipe's test command last exited with.

**Stated** holds the notes and handoffs a person or an agent wrote down (`Epistemic::Stated`). Each
line carries its time and its actor. Nodal never mixes them with the facts. Nodal writes every line
that comes from an event body on one line, so text from outside Nodal cannot change the shape of the
file.

**Project ledger** states, for every other open unit: its branch, its commits, and the files those
commits changed against its own base. It also states what the branch this unit merges into gained
since this unit's base commit. Each sibling takes 40 lines at most. The cap states how many commits
and files it dropped.

The write is atomic. Nodal writes the text to a file beside the memory, then renames it onto the
memory. A reader therefore gets one whole answer, never half of two.

`CLAUDE.md` and `AGENTS.md` in the home each carry one line that names `WORKUNIT.md`. The line is
idempotent, and it keeps its place in a file a person wrote. Nodal leaves a file the project tracks
exactly as it is, and states that it did not write there. Nodal never changes a file under version
control.

## Environment variables
`NODAL_ID`, `NODAL_UNIT` (slug), `NODAL_PROJECT`, `NODAL_HOST`, `NODAL_ROOT`, plus recipe-declared
generated variables (`PORT`, `APP_URL`, service URLs). Every process started in an activated home
carries them, which is what makes attribution and session state readable.

A shell reads them by one of two routes. direnv reads `.envrc`. `.envrc` holds two lines: it reads
`.nodal/env`, then it evaluates `nodal env --export`. A shell with no direnv evaluates the same
command, which is what the hook `nodal shell-init` installs does. That one evaluation is the only one in the hook, and what it evaluates is Nodal's own output,
quoted so that no character of a value is interpreted. Nodal spawns no subshell for either route.

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
2. the person's own file `~/.config/nodal/secrets.env`;
3. nothing, in which case the name is a line of the report.

A unit-generated value wins over a value of the same name in a person's file, because only the
unit-generated value is bound to the unit's own resources. `NODAL_SECRETS_FILE` moves the file, as
`NODAL_STORE` moves the registry. A machine that still holds `<state root>/secrets.env` and has no
file under `~/.config` reads the old path, so nothing breaks on an upgrade.

The file is the person's own and not the machine's. `NODAL_HOME` may name a directory a whole group
owns (see Shared hosts), and one secrets file for such a host would hand every account the same
credentials.

Nodal creates `~/.config/nodal/secrets.env` with mode `0600`, in a directory with mode `0700`.
Nodal refuses to read the file when its mode gives any access to group or other, and reports the
mode alone.

A name that no tier answers is a line of the report. It never stops a unit from being created.

A secret value goes into the output of `nodal env --export` and nowhere else: not into `.nodal/env`,
not into a manifest, a bundle, a log line, an error message or `--json` output. `.nodal/env` holds
the unit's identity and the values its own services generated, so two accounts entering one home
read the same file and resolve their own credentials.

## Shared hosts
Two people with accounts on one box see one list. The state root's own mode says so: a directory
with the setgid bit set is a shared root, and Nodal reads that rather than a setting of its own.

In a shared root, Nodal writes the registry and its `-wal` and `-shm` files with mode `0660`, runs
under umask `002`, and writes each home's `.nodal/env` group-readable. A root without the setgid bit
is one person's own and nothing above applies to it.

`nodal init --shared <group>` makes a root like that: it creates the directory, gives it to the
group, sets mode `2775`, and prints each thing it did. Run it once per host. Every command after it
reads the mode.

A project is keyed by its `origin` remote, normalised, and by its checkout path where it has no
remote. Two clones of one repository are therefore one project with one base and one block of
ports. Each command acts on the checkout the person is standing in, never on another person's.

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
`services.{shared,per_unit}`, `env.{required_local,generated,secrets,stand_in}`, `base.{exclude,invalidate}`,
`hooks.{pre_new,post_new,pre_reclaim,post_reclaim}`, `sync.auto_irreversible`, `reclaim.trash_retention`
(in days), `lock.idle_hours`. Alongside those, and additive to them: `package_manager_pin`, `monorepo`, `task_cache`,
`dockerfile`, `compose`, `commands.{lint,typecheck,reset}`, `db.{tool,migrations_dir,fixed_ports}`.

No copy drops a path the project tracks. A copy that is missing a tracked path is dirty the moment it is
made: `git status` in it reports one deletion for every file under that path. Inference reads `git ls-tree`
before it proposes a row, and a copy reads it again before it starts. What the copy then does depends on
who wrote the row. A row the project wrote in `base.exclude` is refused, and the message names each tracked
path it found. A row of Nodal's own default table yields instead, because no recipe key can take such a row
off the list: the copy keeps the directory and the report carries one note naming the row and why it was
kept.

Every key is optional and unknown keys are rejected, so a typo is a message rather than a line silently
ignored. Most keys are inferred by `nodal init` from the project's own files; only the gaps need a human
line, and `init` writes each gap as a comment above the empty key it belongs to. Published as
`schemas/v1/recipe.json`.

`package_manager` takes `pnpm`, `yarn`, `npm`, `bun`, `cargo`, `uv`, `poetry` and `pip`. One per
ecosystem, chosen from the committed file each manager installs from; `pip` is read from a
`requirements.txt`. A repository that carries an ecosystem's manifest and names no manager for that
ecosystem is reported not ready, with the manifest and the ecosystem named: nothing would install
that half, so no base is warm for it.

**A base build never installs into the host.** `uv` and Poetry make their own environment; `pip` does
not, so a base build makes one for it. The build runs `python3 -m venv .venv` as its own step and then
runs `.venv/bin/pip install -r requirements.txt`, and a host with neither `python3` nor `python` is
refused before anything is cloned, in the words the refusal for a package-manager pin uses. Because
the environment is in the tree, a copy with no `.venv` is reported cold rather than unanswerable.

`nodal init --force` rewrites a recipe that is already there. It keeps every key the file sets and
renders the file from the merged recipe, so it writes the template's comments over the ones a person
wrote. It names every line it takes out and every line it puts in, with the line number of the file
each belongs to. Those lines go to standard error, and they go before the file is written, while it
still holds them; standard output carries the one document about the file that now exists, and
`--json` carries the same list as `changes`.

## CLI
`init, approve, new, new --carry, cd, adopt, ls, show, explain, env, shell, shell-init, claude-code, mcp, run, ps, start, note, ask,
handoff, sync, done, merge, prune, reclaim, reclaim --check, gc, doctor, base, status, uninstall, upgrade`. Every read command accepts
`--json`; `status --watch` emits newline-delimited JSON. Global `--store` and `--no-hooks`.

`nodal done --wip` says on standard error what the flag sends — every uncommitted and untracked file of
the home — before the push, not after it.

`nodal handoff [--unit <unit>] "<text>"` records one stated handoff on a unit and prints it. It takes no
lock, enters no home and writes no file in one: the unit's memory is compiled from the registry by the
commands that read a unit. A handoff whose text is empty or blank is refused. The actor is read the way
every event reads it, so a handoff an agent states is the agent's.

## The tool surface
`nodal mcp` answers an agent's tool calls as a model context protocol server. One JSON-RPC 2.0 message per
line on standard input, one answer per line on standard output, synchronous: a request is read, answered and
written before the next line is read. There is no runtime and no daemon, and stdio is the only transport.
Standard output carries answers and nothing else; everything a command would say to a person goes to standard
error.

Methods: `initialize`, `tools/list`, `tools/call`, `ping`. A message with no `id` is a notification and is
never answered.

Tools: `ls`, `show`, `check`, `new`, `handoff`, `done`. **A tool result is what the matching
`nodal <verb> --json` writes.** The content is one text block holding that document. There is no second
representation, and `ci/acceptance-mcp.sh` compares the two documents.

The `done` tool sends the unit's branch and nothing else. It takes no `wip`: that flag sends the
work-in-progress snapshot, which carries every uncommitted and untracked file of the home, and it stays
on the command line where the person who types it is the person whose work it is.

`reclaim`, `merge`, `gc`, `uninstall` and `base` are not tools. They are absent from `tools/list`, and
`tools/call` on one of them answers with a result marked `isError: true`, naming the verb, the reason it
is not offered, and the command a person runs instead — the same shape a refusal of the work takes, so
the agent that asked for it reads the answer.

**A refusal and a bad message are different answers.** The work saying no — no such unit, a held unit, a
handoff with nothing in it, a verb that is not offered — comes back as the tool's own result with
`isError: true`, carrying the sentence the command line prints, because a model has to read it to act on
it and several clients never show a protocol error to a model. A message that is wrong — an argument the tool does not take, one of
the wrong type, a required one missing, an unknown tool — is `-32602`. A line that is not a request, a
batch, a missing `jsonrpc` or `method`, or an id that is not a string, a number or null, is `-32600`,
answered under the id the line carried. Arguments are checked against the tool's own published schema,
so what `tools/list` states and what the server enforces are one thing.

Each call opens the registry the way the matching command does, resolves what an interrupted operation left,
and closes it again. A tool runs in the directory the server was started in; no tool takes a path to work in.

`nodal init --claude-hooks` declares the server in the project's own `.mcp.json`, under
`mcpServers.nodal`, as `"command": "nodal"` with the argument `mcp`. It names the program and never the
path of the binary that wrote it: the file is committed in most projects, and an absolute path would put
one person's home directory in the repository and hand every teammate a server that is not there. It is
one marked region, written the way the hooks are, so `nodal uninstall` takes it out and leaves the file
byte for byte the file it was, with every other server somebody declared still in it; a file somebody has
since reformatted is read and written again instead, which changes its formatting and no other member of
it. `nodal init` reads both files before it writes either, so a malformed `.mcp.json` refuses the command
rather than leaving the hooks installed and the declaration missing. It says on standard error that the
declaration runs `nodal mcp`, so whoever opens the project needs `nodal` on their own PATH. `nodal mcp --tools` prints the listing; the committed copy is
`schemas/mcp/tools.json` and `ci/schema-diff.sh` fails on a change that is not committed with it.

Nodal asks whether it shares file blocks under the state root **once**, when the state root is made. The
command that makes that directory is the one that asks: the first registry open creates it, and `nodal init`
creates it too. Nodal asks the way the materializer asks. It writes one small file in the nearest directory
that exists, puts that file on the disk, tries one clone with the backend's own call, and removes both files.
It does not read the name of the filesystem to decide.

The answer is written beside the registry as `sharing.json`. The record holds the state root, the filesystem's
name or `null`, the answer, when it was taken, and the device the state root was on. Every command that needs
the answer reads that record and asks no filesystem anything: the backend a home is made with, `nodal init`,
`nodal doctor`, and the `materialize` example. Nodal asks again only when there is no record, when the state
root's device is not the recorded one, or when `nodal init --reprobe` asks.

The answer has three values: `yes`, `no`, and `could-not-ask`. A state root the probe could not write in — a
full disk, a read-only mount, an exhausted quota, a directory owned by somebody else — is `could-not-ask`.
Nodal prints it as "could not ask" with the errno, and never as a filesystem that cannot share blocks.

Where Nodal does not share blocks, every unit home is a full copy, and `init` prints one line for that case on
standard error: the state root, the filesystem where Nodal can name it, that each home is a full copy, and the
fix. The fix names the filesystems this build shares blocks on. On Linux those are btrfs, XFS formatted with
reflink (`mkfs.xfs -m reflink=1`), and bcachefs. On macOS it is a volume in an APFS container. Where the
filesystem is named `9p` or `drvfs`, the line says instead that this is a Windows disk mounted into Linux and
that the state root belongs on a Linux disk. The name says that, not the machine. A state root Nodal could not
ask about reads the fix `nodal init --reprobe`. A state root where Nodal shares blocks prints nothing.

`nodal shell-init <shell> --install` writes the script into `<state directory>/shims/` and adds one marked block
to that shell's start-up file. The block sources the file. It evaluates nothing and starts no process, because it
runs in every shell a person opens. `nodal shell-init <shell>` on its own still prints the same script, and
`eval "$(nodal shell-init bash)"` in a start-up file still works.

`nodal uninstall` removes the block, the scripts, and with `--state` the state directory. It prints one item per
thing before it removes any of them, and it asks once; a terminal nothing is watching is refused rather than
waited on. A start-up file is byte-identical to the file it was before the install.

The default removes what Nodal installed on the machine and **no unit home**. The state directory stays and the
homes are inside it, so what a person is left with is what the promise has to be about: **each home is a
standalone Git repository**. Its history is readable, its tree is clean, its object store is whole, it borrows
no objects from the base or from anywhere else under the state directory, and nothing in its Git configuration
names a path only Nodal puts on a machine. `gc.auto = 0` stays, because it names nothing and any Git can use a
repository that has it; `remote.origin.url` names where the project came from, which is the person's own.
`tests/safety/tests/uninstall_repositories.rs` asserts each of those from inside a surviving home, with `nodal`
off the search path and every variable Nodal reads out of the environment.

`--state` is the other promise and it is destructive by request. It removes the state directory and the unit
homes in it. It runs `lifecycle::uniqueness` over every unit home first and refuses while one holds work that
exists nowhere else; `--force` accepts that and says what it accepted.

`nodal upgrade`, and `nodal update`, report how this copy was installed — a cargo bin directory, a Homebrew
cellar, a system package path, or a binary placed by hand — and print the one command that upgrades it there.
Nodal has no self-updater and **makes no network call of its own**: no update check, no telemetry, no version
comparison. `tests/safety/tests/no_network.rs` asserts that no code path in either crate could make one.

`done <unit>` sends a unit's work for review. It pushes **one** ref, the unit's branch, with one
`git push`. Nothing a person did not commit goes. A work-in-progress snapshot of everything the
home holds that no commit does is still taken, and it stays on this machine at
`refs/nodal/<unit>/wip`. `--wip` adds that ref to the push and is documented as sending
uncommitted files; it is the only way one leaves this machine. The push is the user's own `git`, so
the credentials and the hooks are theirs, and the report says so in the line it prints. The branch
goes as it is; a push that would not fast-forward is refused by the remote and reported. Only
Nodal's own `refs/nodal/` ref is ever replaced, because each snapshot is built from the working tree
rather than on the last one.

`done` pushes the branch. A reclaim deletes Nodal's own refs on the remote. Nothing else Nodal does
reaches the network.

It then prints the page a person opens the change on, for the host the remote names, and **opens no
pull request**. There is no host API in Nodal and no client of one; a remote whose host Nodal has no
compare page for is told so rather than guessed at. The unit moves to `review`.

### `new --carry`

`nodal new --carry` starts the unit at the checkout's `HEAD` and copies into it what no
commit holds. The carried set is what Git calls work: **staged changes, unstaged changes to
tracked files, and untracked files no ignore rule covers.** The distinction between staged
and unstaged is preserved wherever Git can express it — a path that was staged *and* edited
again on top arrives in both states, as it was.

It is a **copy, never a move**. The source checkout is read with three `git` processes and
the bytes of its untracked files, and nothing else: nothing is staged, stashed, committed,
checked out or cleaned in it, and its index is never opened for writing — every invocation
Nodal makes sets `GIT_OPTIONAL_LOCKS=0`, so not even the stat cache a `git diff` refreshes
is written back. After a successful carry the checkout's working tree and its `.git/index`
are byte for byte what they were.

The unit **starts dirty**. Nodal manufactures no commit for the carried work, writes no
ref, and makes no network call. The unit's branch stands at the same commit the checkout's
`HEAD` stands at, and the work sits on top of it uncommitted, where the person commits it
as they meant to.

**Ignored state is not carried.** A path an ignore rule covers is dependency or build state,
and the base already owns it; the home is a clone of the base and has it already. Carrying
a second copy would be paying twice for what copy-on-write gave for nothing.

`--carry` refuses rather than guess, and each refusal names which of these it was:

| refused | why |
|---|---|
| an index with unresolved merge stages | Git can express neither stage in a patch, and a unit given one side of it would look resolved |
| a `HEAD` that is detached or has no commit | the unit has nowhere to start and the work nothing to be a difference from |
| a Git operation in progress — merge, rebase, cherry-pick, revert, bisect, `am` | the same rule every create is already held to |
| `--from` | `--carry` pins the start to the checkout's `HEAD`, so a second starting point is a contradiction |
| a **submodule holding work of its own** — modified tracked files, or untracked ones | that work is in a repository the superproject does not hold, so neither patch can reach it |
| a carried path that is **not a regular file or a symbolic link** — a directory, a named pipe, a socket, a device | a copy reproduces those two kinds and nothing else |
| more than 5,000 paths or 64 MiB | a set that size is state a build left behind that no ignore rule covers, not an edit in progress |
| a path the home already holds with other content | overwriting would lose one of the two silently |

**Submodules.** A submodule's recorded commit is a gitlink in the superproject's own
index, and it travels in the patch like any other change. What does not travel is
anything inside the submodule's own working tree, and the diffs pass
`--ignore-submodules=dirty` so that a patch never carries the unusable `-dirty` rendering
of it. `--carry` therefore refuses while a submodule holds work, rather than making a unit
that silently lacks part of what the person had. The reading that decides this passes
`--ignore-submodules=none`, which overrides `submodule.<name>.ignore` and
`diff.ignoreSubmodules`: hiding the work from `git status` does not make it something a
carry could reproduce. Carrying a submodule's work recursively is not something `--carry`
does.

**Path kinds.** Every path in the carried set is classified by one `lstat` before any of
them is opened, and before the first `git diff` runs. A named pipe has no end of file, so
a reader of one waits for a writer that may never come; answering from the kind rather
than the content is what makes the refusal reachable at all. A directory reaches this list
through the case that actually occurs: Git does not descend into a repository it does not
own, so a clone made inside the project arrives as one untracked path, and copying it
would put a second copy of somebody's repository in the unit. A broken symbolic link is
carried like any other link — what travels is the target's name, not the target.

Every refusal leaves the checkout unchanged and no unit behind, and every one that can be
made before a base is built is made there, so a `--carry` that cannot work does not first
cost minutes. A failure *after* the home exists rolls the create back to nothing, exactly as
any other failed create does, and still touches nothing in the checkout.

The carry is the last step of the create, so what it lands on is a home a clean `nodal new`
would have made. It is journalled like every other step, and the unit's log records how many
paths of each kind it brought across — counts and bytes only, never content.

`--carry` is a thing a person types. The Claude Code provider hook never passes it: a
worktree an agent asked for is a place to start work, not somewhere to move a person's
half-finished edit to.

`adopt <branch-or-path>` makes a unit of work that is already here, in one of two forms. `adopt --all
--in-place` does the same for every worktree of the project except the main checkout.

`--in-place` makes a checkout or a linked worktree a unit **where it stands**. The only writes are
`.nodal/` and `.envrc`, and both are excluded from Git before either is written, so `git status` in
that directory is byte for byte what it was. Nothing is cloned, no branch is created, and no file of
the person's is touched. The environment row carries `managed = false`, which makes the directory a
root: a reclaim unregisters it and never moves it. A directory can be adopted no other way, so
`--in-place` is stated rather than inferred.

`--all` adopts every worktree `git worktree list` names for the project, one at a time. It skips the
main checkout and any worktree that is already a unit, and it says so. It needs `--in-place`. Each
row is one line. A summary counts what it did.

Without `--in-place` the target is a branch nothing has checked out, and it gets a home of its own
from a base, made exactly as `nodal new` makes one except that the branch already exists and is
fetched from the project's own checkout rather than created. A branch a worktree *does* hold is
refused and the message names that worktree: a second home for it would leave whatever is uncommitted
there behind.

Adoption refuses the project's own checkout, a directory that already carries a unit's marker, a
directory inside another unit's home, and a checkout whose HEAD is a commit rather than a branch.

The two forms run different hooks, because they did different things. Adoption in place runs **no
recipe hook at all**: nothing was created, and running a project's commands inside a person's live
checkout on the strength of registering it is not something registering it asked for. The
materialised form runs `post_new`, exactly as `nodal new` does: it made a home, and the recipe's
contract is that a home Nodal made has had that hook run in it. Neither form runs `pre_new`, which is
about the moment before a home is made from a base nothing has decided yet.

The handle comes from `--name`, then from the last segment of the branch, because the branch is the
name every other tool already shows for that work.

Where nothing states what the unit is for, adoption recovers it from the records of the session that
ran in that checkout — the same reading `nodal doctor` prints beside a nested worktree — and records
it as **recovered, not stated** (`unit.objective_epistemic`). Every rendering says which of the two it
is. A stated objective is never replaced by a recovered one.

A recovered objective is marked recovered wherever an objective prints: `nodal ls`, `nodal show`, the
adoption's own report, and `WORKUNIT.md`.

**The prompt is read, not copied.** A session that a dispatch opened starts with a preamble, and the
task comes after it. Recovery drops the lines that are preamble — a heading in capitals, a note the
agent tool wrote about itself, a rule with no sentence in it — and takes the first sentence after
them that opens with an instruction verb. A prompt with no such sentence gives its first line, which
is what recovery gave before this rule. Every answer is text out of the record. Nodal writes no
objective of its own and marks each recovered one as recovered.

**An adoption closes with a sentence.** The report ends with what happened to the directory and how
many declared environment names have no value on this machine (`adopted payroll-export in place; 2
declared env names missing locally`). The names follow that line, under it. `--json` carries the same
facts, and gained one field for them: `arrival`, which says `created`, `adopted_in_place` or
`adopted`.

`explain <unit>` answers with why the home is as it is, read back out of what was recorded at the
time: which base it was cloned from and why that one, what the clone left out and who decided each
row, what was removed from the copy after it was made, and which block the ports came from. For a
checkout adopted in place the first three say that nothing was copied, rather than being blank.

`merge <unit>` takes one unit from a dirty home to a merged target in one command. It runs five
stages, and every stage has a flag that drops it: `commit` (`--no-commit`), `squash` (`--no-squash`),
`rebase` (`--no-rebase`), the fast-forward of the target, and `remove` (`--no-remove`). The commit
message comes from `-m`, then from the unit's objective, then from the unit's handle. Nodal opens no
editor and asks no language model for it.

The command prints the plan on standard error and asks once whether to run it. `--yes` answers that
question, which is what a script uses. A terminal that is not watched refuses rather than waits.

The target is the unit's parent branch, then the branch `refs/remotes/origin/HEAD` names, then `main`
and `master`. It is always a local branch of the project's own checkout.

The fast-forward moves that branch only when the commit it points at is an ancestor of the commit it
moves to. A target that has moved is refused. Nodal never force-pushes, never rewrites the target's
history, and pushes nothing: sending the branch to a remote stays a person's own command.

A rebase that stops for a conflict leaves the unit in the middle of the rebase. The report names the
paths and both ways out. A second `nodal merge <unit>` continues the rebase and finishes the
remaining stages. `nodal merge <unit> --abort` stops it and puts the branch back at the commit the
merge recorded.

Before the squash rewrites the branch, Nodal writes the branch tip to `refs/nodal/<id>/premerge`.
Every commit the squash folds stays reachable from that ref. The ref travels to the trash with the
home, and `nodal gc` removing that home is what lets go of it.

The `remove` stage is the ordinary reclaim, so the uniqueness check applies to everything the merge
did not integrate. A reclaim that refuses leaves the unit where it is. The merge itself is already
done, and the report says so.

`doctor` reads and never writes. It reports worktrees, stale build caches, exited containers,
unreferenced volumes, orphan databases, a project over the open-unit threshold, and the unit homes that
hold work no other copy has, each with a size, in two
sections: this project, and a separate section for another project's leftovers that carries names and sizes
only.

A unit home is read the way a reclaim reads it (`reclaim --check`, one evaluator), and the row says how
many of its commits are only here and how many nothing has checked. The row carries the unit's
objective as its intent. A home that could not be read is a row too. So the closing sentence of the
first section — nothing of this project is left behind — is printed only where every home of the
project read clean, and it is never the answer for a machine holding the only copy of a morning. A worktree another tool holds a lock on is reported as locked and read no further. Removal of
unmanaged state is a later command.

The report opens with one line about the state root, in every case. Doctor reads the record and writes
nothing: it never takes one, because taking one writes a file into the directory the report is about. Where
there is a record, the line says whether Nodal shares blocks between files there, does not share them, or
could not be asked, and it names the filesystem where it can. A filesystem this build cannot name is reported
as unnamed, and `--json` carries the name as `null`. Where there is no record, the line says that nothing has
recorded an answer and names `nodal init --reprobe`; doctor does not go and find out. `--json` carries the
whole record as the `sharing` field, and `null` where there is none. Doctor states the fact and prints no
fix; `nodal init` prints the fix at the moment a person chooses the state root.

The worktrees are every worktree the repository names, read from `git worktree list`. Where the directory
sits is not part of the question: a worktree under the checkout, beside it, or anywhere else on the machine
is the same row with the same facts — branch, pushed or unpushed, dirty, behind, locked, size and recovered
intent — and it is in this project's section because this project's repository named it. A row is named
relative to the checkout when it is inside one and by its whole path when it is not.

A worktree Git itself calls prunable is reported as `prunable`, in that word. Git decides this from its own
record and doctor states the verdict with the reason Git gave. The verdict is not a question about the
directory. A reaper of temporary directories removes the files of a worktree and leaves the directories
behind. The path then exists while the worktree is prunable. Doctor asks Git, never the filesystem.

A Docker container or volume is attributed by its label first, then by the host paths it mounts, and last by
its name. A stopped container mounts nothing, and a volume nothing refers to has no mount and no label. Both
still carry a project's name in their own name, which is what a compose file writes. Doctor knows two sets
of project names: the registry's projects, and the directory names of the checkouts it surveys. A name that
holds the whole name of another such project puts the row in that project's section. A name that matches
nothing stays in this project's section. "I cannot say whose this is" is not the same claim as "this is
another project's".

A third section reports the local branches of the checkout that no worktree has checked out. Every other
source is anchored to a directory and a branch is not. A report of directories can therefore be all-clear
over work that exists on no remote. Each branch is in one of three buckets: merged into the default branch;
unmerged, with every commit on a remote; or unpushed, meaning commits that exist on no remote-tracking ref
(the `remote_containment` predicate). The unpushed bucket prints one row per branch. A row carries the branch, the count of
commits no remote has, the age of the last commit, and whether its upstream is gone. The other two print one
line each with a count. `--all` opens them. `--json` carries every row either way: the flag decides how much
is shown, never what was found. The audit is two `for-each-ref` calls and one `rev-list` per ref, and it
reads only. What to do about a branch is a person's decision.

Every size doctor prints is logical bytes: the sum of the sizes of the files and links under a directory, as
the source counts them. It is not the space a removal would give back. A filesystem that shares blocks
between files holds less than the figure; one that pads every file to a block holds more, so on a filesystem
of that kind a reclaim gives back more than doctor reported. Doctor prints the figure it read and does not
model a filesystem to guess the other one.

A registry a later Nodal wrote stops every other command (`StoreTooNew`). It does not stop `doctor`. Doctor
is what a person runs when something is wrong, so it reports what needs no registry — the worktrees and the
caches of the checkout — and states the mismatch as a note: both schema versions, the fact that nothing in
the registry was read, and the one command that upgrades this copy of Nodal. Nothing is fetched to say it.

`--machine [ROOT ...]` walks for `.git` under each root. With no path, it walks the home directory.
Add roots with `--machine <path>`. A root is the directory the walk searches. A root that is not itself
a clone is neither a skip nor an error. The walk reads a repository out of a `.git`: a directory holding
`HEAD`, or a file naming a `gitdir`. A `.git` of any other kind leaves the directory an ordinary one, and
the walk goes on under it. The default depth is 6. `--depth` changes it. The walk skips Nodal's
state directory, any registered unit home, and a mount that is not a local filesystem. It reports each
skip and the reason. It groups clones by the URL of `origin`. SSH and HTTPS forms of one host path are
one group. A clone with no remote is its own group, named by its path. Each group states the clone count,
commits on no remote, dirty clones, logical size, the largest ignored directories, and last commit age.

The survey proves the uniqueness of each clone on each run. It proves it from two things, and it reaches
no network. A commit another clone on this machine holds survives the deletion of this one. A commit a
remote-tracking ref holds reached the remote, but only if the freshest clone of that remote on this
machine vouches for the ref. The freshest clone is the one that heard from the remote most recently and
fetches every branch.

Only refs under `refs/remotes/origin/` are read as evidence about the remote. A group is the clones that
share the URL of `origin`, so `origin` is the remote in question. A `backup` or an `upstream` remote says
nothing about it and may not answer for it. Refs of any name still count as a second copy, because a ref
of any name keeps the object alive in the store it sits in.

The freshest clone's reading of a branch replaces this clone's. A branch it does not have is gone, and
its old ref proves nothing. A branch it has is read at the freshest tip, so a rewritten branch does not
vouch for the commits it dropped. This clone's own tip for a branch counts as well, but only where the
freshest clone confirms it: a branch that moved forward keeps its old tip in its history and a branch
that was rewritten does not. A tip the freshest clone never fetched is one it cannot vouch for.

With no clone fresher than this one, nothing here can check its refs and none of them is believed. Its
uniqueness is then settled only by a second copy on this machine. Where another clone holds every commit
it holds, it is safe to delete and the report says so; its `unpushed` is `null`, because whether a remote
has the work is not something this machine knows. Where no other clone holds them, the clone is reported
as "not checked". A lone clone of a remote is therefore never called clean on its own bookkeeping.

"nothing unique" means the survey examined every clone of the group and found no commit that only one
clone holds. A clone the survey could not read is reported as "not checked", never as clean, and the
report counts how many were not checked. A second closing line counts the clones whose refs nothing here
could check. The report names each clone that holds the only copy of a commit, with the count, and gives
the command that sends the work to a remote. It names each clone whose commits are on no remote but
survive in another clone here. `--json` carries every clone, and each clone names the clones whose
reading of the remote checked its refs, in `witnesses`.

A group's size counts a file once. Cargo and `git clone --local` hardlink one file into many directories,
and a sum of the clones would count it once per link. The figure is apparent bytes, held to within 1% of
`du -c --apparent-size` over the same paths. It is not the blocks the filesystem allocated.

The walk writes nothing.

`--json` and the default output are two renderings of one value, so a field a person sees is a field a
tool can read. A read type carries the instant it was taken as `now`, and every relative time it prints
is measured from that, so a rendering is a function of its inputs.

`status --watch` polls; there is no daemon. One line is one whole `status` document, identical in shape
to `status --json`, and a line is written only when the answer has changed — the instant moving on its
own is not a change. A consumer therefore holds the last line as current state, and silence means
unchanged rather than gone.

## The list
`nodal ls`, and `nodal` with no subcommand, answer with one row per unit of the project the
working directory is in.

A directory is in one of four states. Each state gets a different answer.

A project the registry holds units of gets the table. A project that holds a `nodal.toml` and no
units gets the empty list. A note says which command makes the first unit. `nodal init` writes the
recipe and opens no registry. This is the state of every project between `init` and the first `new`.

A checkout with neither a recipe nor a registry row gets the verdict on its worktrees (see The
verdict). A directory that is not a checkout either is in no project, and `nodal ls` refuses. A bare
`nodal` prints the help for that fourth state only.

**What a home holds is a walk of it, and a list does not take one.** The `disk` of a unit is either
what a walk found or the reason nothing walked it, and it is never an empty column. `nodal ls` and a
bare `nodal` say `not measured`; `nodal show` walks the home and states apparent bytes. The figure is
the one `nodal reclaim --check` prints for the same kind of claim: apparent bytes, whether the walk
read everything, and the sentence that a home shares blocks with the base it was copied from, so this
is not what a removal gives back. The two commands do not print one number: `nodal show` measures the
whole home, and the preflight measures the paths a reclaim has an opinion about.

## The verdict
`nodal`, in a checkout Nodal holds no row for, prints one row for each other worktree of the
repository. It writes nothing to do so. It does not create the state directory. It does not create
the registry. This is the first command most people run, so it asks for nothing first.

The row names the worktree, what it is for, whether the work is done, what it holds that exists
nowhere else, how far it is behind, what it occupies, and how old it is. Read the columns as
`WORKTREE`, `FOR`, `DONE`, `ONLY HERE`, `BEHIND`, `SIZE`, `AGE`.

`DONE` is the same merge-tree verdict the list computes. Nodal measures it against the checkout's
default branch. Nodal resolves that branch from `origin/HEAD` first, then `main`, then `master`.

`BEHIND` is measured against that same branch. The row names the branch it used. A checkout with no
default branch says `unknown`. It never says `0`.

`BEHIND` is as new as the checkout's last fetch. Nodal never fetches to make it newer. Nodal reads
how old the reading is instead. It reads the log Git keeps for the branch. It reads the branch file
itself when the repository keeps no logs. A branch that neither dates is not called fresh or stale.

The last line names that age when the branch has not moved for more than one day. It gives a length
up to one week. It gives the date after one week. A checkout fetched within the day gets no such
words. The age is in the last line and not in a column. Every row is measured against one branch,
and the staleness belongs to the checkout.

`FOR` is quoted text. It is the first prompt of the session that made the worktree. Nodal cuts it to
the width of the column. Nodal changes nothing else about it.

`ONLY HERE` counts commits that exist on no remote and paths that no commit holds. This is the
column that governs the order. A worktree that holds either is printed first. The finished ones are
printed last.

The last line counts the worktrees that are done and hold nothing unique, gives their total size,
says how old the `BEHIND` reading is when it is old, and says that Nodal removed nothing.

A registered project prints the same worktrees in its own table. A leading column says whether each
row is a `unit` or a `worktree`. A home Nodal made is a unit. A folder another tool made is a
worktree. Nodal never prints one under the word for the other.

The list's reading is pure. After reading, the command layer records at most two things it learned
or derived: a unit's flip to merged, and each touched unit's recomputed `WORKUNIT.md`. It records no
event, reconciles no session, and never contacts the network.

**A note is printed once for each cause.** A list reads every home of the project, and most of what
it cannot do it cannot do for all of them. A cause is therefore stated once, with the units it is
about: two units or fewer are named, and more than two are counted (`3 units: the project tracks
CLAUDE.md, so nodal did not write in it`). The number of note lines is a property of the causes and
not of the number of units. The same rule holds for `nodal ps`, where one process table that cannot
be read stops two signals and prints one line that names both.

Each row carries what Git says about the unit's branch at the moment it was asked: how many
paths are changed, staged and untracked; whether HEAD names a commit rather than a branch; how
far the branch has moved from the branch it merges into; what the upstream on the remote has
and what it does not; and one integration verdict.

**The flip to merged** is a state no command could have written. A unit that is **ahead of the base**,
whose verdict is `integrated`, **and** whose branch is contained in a remote has been merged somewhere
else — by a reviewer, on a website — and the list is where Nodal first sees it. That unit moves to
`merged`, and the move is recorded rather than rendered: the retention `nodal gc` measures runs from it.

All three signals are required. Integration alone is a base somebody rebased under an unpushed branch;
containment alone is the state before review, not after it. And a branch ahead of nothing has
contributed nothing that was not already the base's: a unit `nodal new` has just made is `integrated
(ancestor)` because its tip is in the base's history, and is contained by every remote because the
commits it is made of are the project's. Both readings are true and neither is about that unit, so
without the first signal every unit is merged the moment it is created and its home is reclaimed a
retention later on a state that was never true.

`ahead > 0` keeps the case that must be kept. A squash merge and a rebase leave the unit's changes on
the base and its commits only on its own branch, so it is ahead by construction, and `absorbed` is only
ever reached for a branch that is ahead. What it drops is `ancestor`: a unit merged with an ordinary
merge commit is not flipped, and stays in `review` for a person to reclaim. That is deliberate. Git
alone cannot tell that unit from one that never committed — the branch tip is in the base's history
either way — and Nodal records no fork point that survives a rebase. This flip starts a clock that ends
in a home being taken away, so where the reading is ambiguous it does nothing.

The containment signal costs one `git rev-list` and is asked for only of a unit that is ahead and reads
as integrated. The list still fetches nothing, so a home hears about a merge when the person's own
`git` next does.

**The memory** is derived, not learned: it is this reading, written where the next agent reads it
(see The unit's memory). The command writes one `WORKUNIT.md` per unit of the project, not only for
the unit a person named, because a ledger is a statement about the others. It writes no registry row
to do so, and it rewrites a file only when the compiled bytes differ from the bytes that file already
holds. The list is the command a person types most, so it is the command that keeps the memory of a
unit nobody touched today current.

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

**`NEEDS` says why**, in the words `nodal reclaim --check` uses, so one word means one thing in
both places. It is ranked, and the first that applies is the one printed:

| rank | value | what it is |
|---|---|---|
| 1 | `unique loss` | the working tree holds changed, staged or untracked paths |
| 2 | `blocked` | something Nodal did not start is standing in the home |
| 3 | `unknown` | the unit is ahead of the base, the project has a remote, and nothing here has read that remote since the home last wrote its own record of it |

| 4 | `diverged` | merging would conflict, or the base has moved under the branch |
| 5 | `review` | the work is on the base, or the branch is ahead and clean |
| 6 | `nothing` | none of the above |

Row 3 asks whether the **project** has a remote, not whether the unit has an upstream, and the
difference is a whole class of unit. A branch nobody has pushed has no upstream at all; reading
that as "no remote question" would put it under `review` while `nodal reclaim` refuses it. A
unit with no upstream has the most to lose, not the least.

A row shows `—` where nothing computed it: a unit with no home, and a home Git could not answer
for. `—` is not `nothing`, and no value ever prints as the other: one says the question was not
put, the other says it was answered. A reading that could not see the working tree cannot say
the top of the ranking is empty, so it ranks nothing at all and the note under the table says
why.

The column costs the list no extra `git`. The counts, the verdict and the divergence come from
the survey the list already takes; the bystander comes from the one process-table read that WHO
already needs; and the staleness of the remote evidence is read with `stat` — the checkout's own
newest reading of the remote once for the whole list, compared against each home. That is the
cheap necessary half of the witness rule and not the rule: `unknown` marks a row whose remote
evidence **cannot** be current, and whether a current reading actually reaches the commits is
what `nodal reclaim --check` costs a few processes to answer.

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

Each of those five verbs takes the unit's write lock before it does its own work. See Locks.

## Locks
One actor writes a unit's home at a time. The registry holds one lock row per unit. The row names the
host, the actor, the process that took the hold, when it began and when an entry last touched it.

`nodal cd`, `shell`, `run`, `new` and `adopt` take the hold or refresh it. A second actor running one
of those in a held home is refused. The message names the holder, says how long they have held it, and
says `--take`. Every one of those verbs accepts `--take`.

The lock is advisory. It refuses Nodal's own write verbs and stops nothing else. An editor opens in a
held home. `git` runs in it. A process starts in it. A second actor is told, not blocked.

The read verbs never refuse. `ls`, `show`, `ps`, `explain` and `env --export` answer in a held home.
`env --export` is what the prompt hook runs on entry, so it refreshes the holder's window and takes a
free lock. It refuses nobody, and a second actor's shell carries the unit's variables and ports.

A hold lapses two ways. The absolute expiry passes, which is the clock a transfer bundle carries from
another host. Or nobody enters the home for the idle window, which is `lock.idle_hours` in
`nodal.toml` and eight hours when the recipe does not say. The idle window runs from the last entry.
A lapsed hold is taken by the next actor without `--take` and without a hand-off, because nothing was
taken from anybody.

`--take` moves a hold that has not lapsed. It writes a `handoff` event on the unit naming who it came
from and who it went to. Nothing else moves a live hold.

A reclaim releases the unit's hold. This host releases only its own; a hold another machine took is
that machine's to release.

A hold is an actor and a lineage. An actor name alone is not a writer: every agent of a fleet reports
as `claude-code`, so the name matched itself and a second agent entered a home the first one held,
silently. A second process of one actor is a second holder and is refused the way another actor is,
with `--take` as the way through.

The lineage is the POSIX session the hold was taken from, recorded in `lock.session`. It is neither
the recorded process nor the process group: every write verb runs in a new process, and a shell with
job control puts every foreground command in a new group, so both change between one command and the
next while the session does not. So a second `nodal run`, `nodal shell` or `nodal cd` from the same
shell is the same holder and passes, and `nodal run --tether` keeps working: the tether stays live and
the shell that started it holds.

Where the recorded session still holds a process, a second lineage of the same actor is refused, and
the refusal says which of the two the hold belongs to. Where it holds none, the hold has lapsed for
re-entry: the next actor takes it, the take is written on the unit's log as a hand-off, and the log
says the previous holder was gone.

A reading that could not be taken refuses nobody, and never lets a hold go. "I cannot see" is not "it
is gone", and three things say it: a host that publishes no process table, a table this host could not
list, and a table holding a record this account may not read. The last is the shared host the lock
exists for — `hidepid`, or another account's process — where reading a live session as gone would hand
away a hold nobody let go of. In every one of them a same-actor re-entry falls back to the rule that
came before, the name alone, and passes.
A row that records no session, written before locks carried a lineage, is read the same way, and the
next entry rewrites it.

Only a table that was read all the way through says a session has gone. A process that ended between
the listing and the read is gone and says nothing about any other session; counting it as unreadable
would make "I cannot see" the answer on any busy machine and leave every lapsed hold standing for the
whole idle window.

The process that took a hold is recorded and reported. Nothing signals it, and nothing here signals a
session either: both numbers are read from the process table and written down. No hold is released
because the recorded process is gone.

A report says whether that actor is still there. The holder carries a `state`: `live`, `gone`, or
`unknown` with the reason it could not be read — the hold is on another machine, the row records no
process, or this host has no process table. The reading is the process table and never a signal. It is
one question: whether the process the row records is still on this host.

Nothing else raises a hold to `live`. An actor name is not a process, and neither is the unit's own
identifier: an agent that is killed leaves a child standing in the home, and that child carries both.
Reading either as evidence about the hold reported a killed holder as `live` with hours left to run,
which is the case a person most needs the reading to be right about. A process of the unit's own says
the unit is being worked in; it never says who holds the write.

What is still in the home is reported beside the state rather than folded into it. A holder that is
gone whose actor still has a process in the home carries `orphan`, and the report says the two as two
facts: the hold is nobody's to refresh, and something of that actor is still writing in the home.

`gone` is printed where a reading contradicts the row, and nowhere else. A hold this host could not
read a process for is printed the way the row states it, because a reading nobody could take is not
evidence against the row: on a host with no readable process table every hold reads `holds`, as it
always did, and `--json` carries `unknown` with the reason. `nodal show` states the process and the reason under the WHO line.

The state changes no refusal. This is the reading of the recorded **process**, and it stays a word in
the report: a process identifier is reused, and a hold that let go on a reading of one would be a hold
that let go of the wrong home. A held unit refuses the write verbs until the hold lapses or `--take`
moves it, whatever became of that process. The refusal says what this host read — "pid 4120 that took
it is gone from this host" — because a person refused over a session that ended can take the unit at
once, and one refused over a session at work waits.

The reading of the recorded **session** is the other one, and it does change the refusal. It is a
different question with a different failure: a session identifier that came round again names a
session, and the worst it does is keep a lapsed hold held, which is the conservative direction.

A lock row written before locks carried an actor names a host and holds nobody. It refuses no one, and
the next entry into that home rewrites it. The same rule holds for a row that records no session: a
record that states nothing refuses nobody.

WHO is two readings, in this order: the lock rows, then the process table. The order is the point. A
process scan reads `/proc`, which does not cross Linux accounts, so on a host two people share it
cannot see the other person. The lock row is written down and can. `nodal ls --json` and
`nodal show --json` carry both: `holder` with its expiry, and `sessions`.

## Attribution
`nodal ps` answers what is running on this host and which unit each thing belongs to. Every row
carries a confidence, and there are two levels. `certain` means the thing named its unit: a process
carrying `NODAL_ID`, a container carrying the `nodal.unit` label. `probable` means Nodal inferred
the unit. Three readings are probable: a process whose working directory is inside a home, a
container that mounts a home, and a granted port that has a listener. A row carries no other level.

The level decides what a teardown does. `nodal reclaim` and `nodal gc` signal the certain level.
They report the probable one and signal none of it.

Nodal labels every container it starts with `nodal.unit` (the unit identifier) and
`nodal.environment` (the materialisation). These are the container half of the environment-variable
contract above.

A signal that cannot run gives a note under the table. It is never a failure. A host with no Docker
daemon still answers, and so does a host whose process table Nodal cannot read. An empty answer means
nothing runs. A note means Nodal could not read that signal. Its `reach` is `unread` when the signal
did not run, and `part` when the signal ran and the host refused part of what it reads.

macOS reads the process table and refuses two things. It does not show the variables or the
directory of a process of another account. It does not show the variables of a process that runs a
restricted binary, which most programs under `/bin` and `/usr/bin` are. Each refusal is a `part` note
with that reason. A restricted binary that stands in a home is found by its directory, so it is
`probable` and never signalled, even when it carries the unit's `NODAL_ID`. A Linux scan leaves out a
process this account cannot read, and gives no note.

## Tether
`nodal run --tether <command>` starts the command in a process group of its own and records that
group in the registry, as a session row carrying a `pgid`. The group then belongs to the unit. A
session is the row for it because a session is already one attachment to one materialisation, opens
when the attachment starts, closes when it ends, and is already what a reclaim gives up. A lease is
not: a lease expires, and a tether that lapsed would be a running group with no record.

The row is the record of the group, not the `nodal run` that wrote it. A tether whose parent was
killed is still found and still stopped. A tether whose leader has been replaced by a process it
started is still stopped, because the signal goes to the group.

Nodal refuses `--tether` in a home the registry does not know. Every other run carries on there and
loses only its event. A tethered group that nothing recorded could never be stopped.

A tethered command reads no terminal input. A process in a group of its own is not the terminal's
foreground group, so a read from the terminal would stop the command instead of answering it.

`nodal reclaim` stops the unit's tethers first, before anything a scan attributed. A group is a
record; a scan by `NODAL_ID` is a record too. A scan by working directory is an inference, and
neither command signals one. `nodal gc` stops a tether whose materialisation has been reclaimed,
and never one of a unit that is live. Both close the row once the group is empty.

A group is addressed with `kill`, which every host answers. A tether is therefore stopped, and its
survival reported, on a host whose process table Nodal cannot read.

## Hooks
Recipe hooks receive `NODAL_SOURCE` (the project checkout), `NODAL_ROOT` (the unit's home),
`NODAL_ID`, `NODAL_UNIT`, `NODAL_ENV`, and `NODAL_PARENT_ID`. `NODAL_PARENT_ID` names the base the
home was cloned from. It is empty for a checkout adopted in place.

Hooks do not receive `NODAL_HOME`. That variable moves Nodal's whole state directory. A hook that
set it and then ran `nodal` would write into the unit's home. `NODAL_ROOT` names the home, as it
does in an activated shell.

There are six hooks. Each runs in a directory that exists.

| hook | when | directory |
|---|---|---|
| `pre_new` | before the home is made | the project root |
| `post_new` | after the unit's rows are committed, by `new` and by an adoption that made a home | the home |
| `pre_merge` | before a merge commits anything | the home |
| `post_merge` | after the target branch is fast-forwarded | the project root |
| `pre_reclaim` | before anything is torn down | the home |
| `post_reclaim` | after the home is in the trash | the project root |

`post_merge` runs before the merge removes the unit, so the home it names is still there. A merge
that stops for a conflict runs `pre_merge` and no other hook.

`nodal adopt --in-place` runs none of the six: it created nothing. `nodal adopt` without it made a
home, and runs `post_new` there and nothing else.

### What a hook leaves running
A hook's shell runs in a process group of its own, and is given no terminal input. A process in a
group of its own is not the terminal's foreground group, so a read from the terminal would stop it
rather than answer it; end of file is the answer, as it is for a tethered command. A hook that finishes with nothing still running
leaves no record, which is every ordinary hook.

A hook that backgrounds work leaves a group that is still running when its shell exits. That group
is written into the registry as a session of the unit's materialisation, with the group in `pgid`
and an actor named `hook:<phase>` — the same row `nodal run --tether` writes. `nodal reclaim` stops
it with the rest of the unit's recorded groups, and `nodal gc` stops one that outlived a unit
already reclaimed. Both work on a host with no readable process table, because a group is addressed
with `kill`.

A group that cannot be recorded is stopped before the command returns, and the reason is the error.
Three cases reach that: `pre_new`, which runs before the unit has any rows for a group to belong to;
a hook that exited non-zero; and a registry write that failed after the shell had started. Nodal
does not report a clean operation around a process nothing on the machine can name.

A recorded group that ends on its own gives its row up. `nodal run` and `nodal gc` ask, with signal
zero, whether each open `pgid` row still holds a process, and close the row of one that does not.
Nothing is signalled to find out, no process table is read, and a row whose group is still there is
left exactly as it is. This applies to a tether as much as to a hook.

`pre_reclaim` is read after it runs, so a group it leaves is stopped by the same reclaim.
`post_reclaim` runs after the teardown, so a group it leaves is recorded against the reclaimed
materialisation, reported as a session the reclaim left open, and stopped by the next `nodal gc`.

Every path a hook is given is resolved: `NODAL_SOURCE`, `NODAL_ROOT`, `{repo_root}`, `{unit_path}`,
and the directory the hook is started in. A hook can therefore compare one of them with its own
`$PWD`, which the shell takes from `getcwd`. On a host where a temporary directory is reached
through a link, the two would otherwise be different text for one directory.

### Template variables
Every hook command may name these five values. Nodal replaces each name with its value before it
gives the command to the shell.

| name | value |
|---|---|
| `{branch}` | the branch the unit owns |
| `{repo_root}` | the project's own checkout, which is also `NODAL_SOURCE` |
| `{unit_path}` | the unit's home, which is also `NODAL_ROOT` |
| `{hash_port}` | a port derived from the branch by a digest, the same port every time |
| `{sanitize}` | the branch reduced to lowercase letters, digits and single underscores |

This is plain substitution. There are no conditionals, no loops and no filters. A name this table
does not hold stays in the text as the project wrote it.

`{hash_port}` is between 30000 and 32767. That range is outside the block the port allocator hands
out, so a hashed port never collides with a granted one. Nothing records a hashed port and two
branches may hash to the same one.

A value that holds a character the shell reads as syntax is refused, and the hook does not run. The
message names the variable and the character. Quote the variable in the command to pass a value that
holds a space.

Hook commands require approval. `nodal approve` accepts the set the project declares, and writes
the approval record and nothing else: it never writes `nodal.toml`. It prints every command on
standard error first and then asks, so no command is accepted before a person has read it; `--yes`
answers in advance, and a run with no terminal is refused and told to pass it. `nodal approve
--print` shows the commands and records none. `nodal init` approves the same set when it writes a
recipe, because a person who runs it has just read the file they are writing. Approval pins each
command by the digest of its exact text. The record is per person, in
`~/.config/nodal/hooks.toml`; `NODAL_HOOKS_FILE` moves that file, and a machine that still holds
`<state>/hooks.toml` and has no file under `~/.config` reads the old path. An approval says that
this person accepts the command running on their account, so it is never shared through a state
root a group owns. A command that has changed refuses to run, and the message
shows the command, and names `nodal approve`. A command nobody approved refuses in the same way.
`--no-hooks` runs no hook and needs no approval.

## Claude Code
Claude Code fires named events at commands declared in a `.claude/settings.json`. There are two such
files. The person's own, `~/.claude/settings.json`, applies in every project on the machine;
`CLAUDE_CONFIG_DIR` moves it. The project's applies in that project and may be committed.

**`nodal init` asks nothing and installs nothing.** `--claude-hooks` writes the four hooks into the
person's own file. `--claude-hooks=project` writes the project's file instead, and prints what that
costs first: the provider hook on a machine with no `nodal` answers `WorktreeCreate` with a refusal, and
Claude Code ends the session over it. `--no-claude-hooks` is accepted and does nothing, because installing
nothing is now the default. `nodal uninstall` removes the hooks from either file.

Each command is the word `nodal` and a subcommand, and names no path of one machine, so the file is the
same on every machine that has Nodal on its `PATH`.

| event | kind | what Nodal does |
|---|---|---|
| `WorktreeCreate` | provider | `nodal claude-code worktree-create` makes the unit, carries the project's settings into its home, records the attachment, and prints the home |
| `SessionStart` | observer | prints the unit's memory, which Claude injects as context |
| `Stop` | observer | records the session's last message as a stated handoff |
| `WorktreeRemove` | observer | records a detach if it ever fires, and removes nothing |

`WorktreeCreate` is a **provider**, not an observer: Claude reads one absolute path from its standard
output and uses that directory, and empty or invalid output ends the session. Nodal's answer therefore
overrides Claude's own worktree creation, and Claude makes no `.claude/worktrees/` entry of its own. The
unit's objective is the slug Claude derived from the opening prompt, recorded as `observed` rather than
`stated`: it is a reading of somebody's intent, not a statement of one.

**A project with no `nodal.toml` is not a refusal.** The hooks are in the person's own settings by
default, so the provider fires in every project on the machine and most of them are not Nodal projects.
Nodal makes the worktree Claude Code would have made for itself, at `<project>/.claude/worktrees/<name>`,
on a new branch of the same name, and answers with it. It registers nothing: no unit, no environment, no
event. It says on standard error which file is missing and that `nodal init` gets a unit instead. A name
that is taken gets a number after it, because a directory that is already there holds somebody's work and
a branch another worktree has checked out is one Git refuses. A directory that is in no Git repository at
all has no worktree to make, and the session is answered with the directory it is already in.

The hook that cannot answer at all prints `./nodal-worktree-create-refused` — a relative path with a dot
segment, which Claude rejects — and says why on standard error. That is deliberate: printing nothing ends
the session just as certainly and says nothing about why. The reasons are a machine with no `nodal`, which
the command text itself handles, and a directory that cannot be named in a form Claude Code accepts.

`SessionStart` fires more than once for one session, with a different session identifier each time, and the
create payload carries a third. **Nothing correlates by session identifier.** The `cwd` a payload carries
is what names the unit, and a unit home says whose it is in `.nodal/id`.

**Every home carries a settings file.** Claude Code reads `.claude/settings.json` from the directory a
session works in. `WorktreeCreate` moves the session out of the project and into a unit home, so the file
that declared the hooks is no longer in scope. Without a file there the three observers never fire in a
session started with `--worktree`: no memory is injected and no handoff is recorded. This was measured on
2026-09-08, headless and interactive, and it was the same in both.

The file is written where the memory and the vendor pointers are written, so a unit made by `nodal new`,
one adopted in place and one the provider hook made all get it, and all three follow the one rule in
**Directory contract**.

**What goes there is the project's own file, copied.** Not a regenerated set of four hooks: that file
would be the only settings in scope for the rest of the session, so the project's permissions, its deny
rules and every hook somebody else installed would stop applying the moment the session moved. A project
with no settings file of its own, or one holding only whitespace, gets the four hooks.

**Nodal never assumes a project ignores `.claude/`.** Three cases:

- **The project tracks the file.** It arrived with the clone and it is the project's. Nodal does not
  touch it, for the reason it leaves a tracked `CLAUDE.md` alone.
- **Git does not track it and the home has none.** Nodal writes one. `/.claude/settings.json` goes in the
  home's `.git/info/exclude`, the way `WORKUNIT.md` does, and the uniqueness check names it. So
  `git status` in a new home is empty, `nodal reclaim`, `nodal done` and `nodal gc` do not call the home
  dirty, and `nodal merge` commits nothing of Nodal's onto the unit branch. The write is atomic, as every
  file Nodal writes into a home is.
- **Git does not track it and the home has one anyway** — a base build wrote it, or a `post_new` hook
  did. Its bytes are somebody else's and stay as they are; it is hidden all the same, because an untracked
  file in a home is a home the uniqueness check calls dirty.

Wherever the file that ends up in the home declares none of Nodal's hooks, Nodal says so once on standard
error and once as a `note` event: nothing observes the session, and the two things that would work are
committing the hooks or installing them in the person's own settings.

**A `WorktreeCreate` fired from inside a unit home answers that home and creates nothing.** A home now
carries the provider hook and also carries the project's recipe, so making a unit of it would register the
home as a project of its own and clone a unit of a unit. A directory is a home when it carries `.nodal/id`
**and** the registry holds that unit with an environment at that directory; a marker no row matches, or one
that cannot be read, is not a home. A payload with no `cwd` means the directory the hook is running in, and
that directory is resolved before the question is asked. `nodal new` refuses in the same case rather than
registering the home as a project.

`WorktreeCreate` also records one `attached` event, `observed`, with `claude-code` as the actor. It is the
one hook that is certain to have run, so the record of a session taking a home does not depend on an
observer firing. **The home is checked against the provider contract before anything durable is written
about it**, so a home Claude would not accept leaves neither a furnished directory nor an event saying a
session took it. **Neither that record nor the settings file may fail the create.** A store or filesystem
error is one line on standard error, and a note event where the store allows one; the home is still
answered with. Ending the session there would leave a fully built unit with nobody in it, which is the
failure this whole path exists to prevent.

`nodal uninstall` surveys the person's own settings file, then every project root the registry knows, then
the homes of registered units, and takes only Nodal's own region out of each settings file it finds. A home left carrying the provider hook after the binary has
gone would answer a later `claude --worktree` with `nodal: not on PATH` and end the session over a tool
the person removed. A home whose copy the project tracks is left alone: that file is the project's, and the
project's own copy is surveyed in its own right. One file the survey cannot read is one line saying so, and
the rest of the plan still stands.

`WorktreeRemove` fired in **none** of four measured session lifecycles, and nothing depends on it. Cleanup
is Nodal's own lifecycle: the unit persists when the session ends, `nodal ls` shows it, and `done`, `merge`,
`reclaim` and `gc` retire it. A unit outliving the session that made it is the product working, not a leak.

The settings file is edited, never rewritten. What Nodal adds is one contiguous region of text it can write
again, so removing it leaves the file byte for byte the file it was, with every other key and every hook
somebody else installed still in it. A file that was reformatted since the install loses the hooks by a
re-rendering of the document instead, which is the only path that is not byte-identical. A file holding
nothing but Nodal's hooks is removed, and `.claude/` goes with it when that empties the directory.

## Snapshots
Nodal records a unit's home before it changes it. The runner takes one commit before the first step of
any operation that changes a unit's tree or its refs: `merge`, an `adopt` of a checkout that is already
here, and `reclaim`. The commit goes on `refs/nodal/<unit>/pre/<operation>`, named by the run in the
journal, so a second run never writes over the record of the first. `nodal done` and
`nodal reclaim --force` write the work-in-progress ref `refs/nodal/<unit>/wip` as before.

The commit is built in an index file of its own, so the person's staged work is untouched and no
tracked file is written. A home with no commit yet has nothing to build on and is not recorded, which
is not a failure. A home that is not on the disk, and a directory Git cannot open, are not recorded
either. Any other failure stops the operation before its first step.

A snapshot is a ref in the home and it never leaves this machine. Nothing pushes one. `nodal done
--wip` sends the work-in-progress ref, by name, and sends nothing else of the namespace.

`nodal show --json` lists them as `snapshots`: the ref, the commit, when it was taken, and what took
it — the operation, with the kind the journal recorded, or the work-in-progress ref.

There is no restore verb. Reading one back is `git`, in the home or in the trashed copy of it:

```
git fetch <path-to-home> refs/nodal/<unit>/pre/<operation>
git checkout FETCH_HEAD            # look at it
git restore --source FETCH_HEAD -- <path>   # take one file back
```

A reclaim moves the home to the trash, and the refs go with it; `nodal gc` removing that home is what
finally lets go of them.

`nodal gc` also removes a record of a run that is over, in a home that is still here. A record is kept
for the `reclaim.trash_retention` window, measured from its commit, which is the instant it was taken.
Three records are never removed: one whose run is still open, one whose run failed, and one whose
journal row is gone. The work-in-progress ref, the branch before a squash and the copies a home took of
the checkout are not records of a run, and no retention applies to them. `nodal show` stops listing a
record the sweep removed.

## Reclaim, trash and gc
**A handle belongs to the unit that holds it.** A unit's handle is unique among the units of a
project that hold one. A reclaimed unit holds none, so the name is free for the next unit the moment
the reclaim archives it, and `nodal new --name <name>` takes it rather than `<name>-2`.

Nothing is renamed to free it. The archived row keeps the name a person typed, its identifier, its
branch, its objective and its place in the log. So a project can carry several archived units under
one name, and every command that takes a name reads them in one order: the unit that holds the handle
answers, and where nobody holds it, the unit that held it last. That is what makes `nodal reclaim
<name>` on a reclaimed unit say it was reclaimed already.

Every destructive path calls one uniqueness check. It reports three things: uncommitted changes,
untracked files that no ignore rule covers, and commits that no remote and no other tree on this
machine has. A hit refuses the operation and names the paths. `--force` does not skip the check.
It first commits the whole home to `refs/nodal/<unit>/wip`, then goes on.

### `reclaim --check`
`nodal reclaim UNIT --check` answers what a reclaim would do and does none of it. It runs no hook,
sends no signal, touches no container, gives back no port, takes no snapshot, moves nothing to the
trash, writes no registry row and reaches no remote. It cannot be given `--force` or `--yes`. The
exit code is the verdict: success where a reclaim would go ahead, failure where it would refuse.

The check and the reclaim read the machine with one evaluator, so a check that says safe over a home
the reclaim refuses is a bug in one place rather than a disagreement between two. The check asks for
more of the same reading: the operation asks only what its refusal rests on, and the check asks for
the ignored state and the runtime as well.

**A commit is in one of four dispositions**, drawn from the commits the home has that the project's
checkout does not reach from a branch, a tag or a stash of its own. Every commit of the project's
history is on the remote, and reporting all of it would bury the few that are not.

| disposition | what it means | does removing the home lose it |
|---|---|---|
| `remote_proved` | a witnessed reading of the remote reaches it | no, while the remote keeps the branch |
| `second_local_copy` | another object store in this checkout or the clones beside it holds it | no, and no server is involved |
| `not_checked` | nothing here read the remote, and nothing here holds it | unknown, so it is kept |
| `only_here` | the reading was taken and it is still nowhere else | yes |

**Which object stores "this checkout and the clones beside it" means.** Two: the project's checkout,
and the other repositories beside it. The second set is found by walking the checkout's parent
directory, two levels down, which reaches a clone put next to the checkout (`<parent>/mirror`) and one
put a directory below (`<parent>/siblings/mirror`). The walk does not enter the checkout itself or
Nodal's state directory, and it is the same bound for every project: this reading is taken before every
destructive step, so what it costs is paid on the safe path. It is not the walk `nodal doctor --machine`
makes.

This scope is not the host, and the two are said in two phrases. `nodal ps` reads **this host**: it
lists what is running anywhere on the machine, units of other projects included. `reclaim --check`
reads **this checkout and the clones beside it**: a clone of the same project somewhere else on the
same host is outside the walk and is not counted as a second copy. One phrase for one scope, because
the same words for both said that a commit held by a clone the walk never reached was held "on this
machine" and safe.

A project whose checkout sits directly in a home directory is the widest case this reaches, because the
parent is then the home directory itself. Two levels is what keeps that bounded. There is no way to turn
the walk off.

A store counts only where its own object store holds the commit, proved by `git rev-list` run in that
repository. A name never counts: a clone that was `reflog expire`d and garbage collected keeps refs over
objects it no longer has, and a reading that believed the name would call a home safe over the only copy
of its work. A store that cannot be opened or read proves nothing, which leaves the stricter answer
standing.

`not_checked` is not zero and it is not safe. A home's own `refs/remotes/origin/*` is the record of a
push it made, so the remote is proved only where a witness confirms it, and a ref name with no object
behind it proves nothing at all. `only_here` says which reading stands behind it: a settled one —
there is no remote, or the remote is on this disk and was read — or the newest reading of a clone,
which can only say what it last saw. A branch a reviewer squash-merged is `only_here`: the content is
on the base and the commit objects are in this home, and the objects are what a removal takes. The
loss is not priced in bytes, because no portable call says what a set of commit objects holds that
nothing else does.

**A path is in one of three dispositions**, over the same two gates the trash prune uses. `git
ls-files --others` supplies the candidates, so no path a commit holds is ever one, and the exclusion
table says which are regenerable.

| what it holds | disposition | what a reclaim does |
|---|---|---|
| uncommitted changes, untracked files | `must_survive` | refuses |
| ignored state no tool writes again | `must_survive` | trashes it; `nodal gc` is what takes it |
| build output and installed dependencies | `reconstructable` | drops it from the trashed copy |

Only the first refuses, and what makes that safe is the retention. A reclaim **moves** a home
to `<state>/<project>/trash/<id>` and `nodal gc` removes it once `reclaim.trash_retention` days
have passed — fourteen by default, stamped on the trash row at the moment of the move, so a
recipe edited later cannot shorten it. Ignored state no tool writes again is therefore still
readable for that window, at the path the report names. Refusing over it instead would refuse
every reclaim of every home that ever held an `.env.local`, and the person would use `--force`,
which is worse for them than the window is.

A directory the exclusion table calls regenerable answers for everything under it, and it goes
on answering when its own removal is refused: the trash keeps the whole of it and the sweep
removes nothing under it. Descending into a directory whose removal failed would leave a
half-pruned tree that no report describes.

Sizes are **apparent bytes** and the answer says so. A home shares blocks with the base it was copied
from, so what a removal gives back is not the sum of the file sizes, and no portable call says what it
is. Nodal prints the figure it has with the reason it is not the other one, and never relabels a
logical size as a physical one.

**The runtime is split the way the reclaim splits it.** The recorded process groups, the processes
carrying the unit's identifier and the labelled containers are what a reclaim would stop; a process
matched by working directory alone is named, never signalled, and is what would make the reclaim
refuse to move the home. Whether a recorded group is still running is not asked, because the portable
way to ask is to signal it. A signal that could not be read is a note and never a zero. A bystander
blocks only a home that would move: a checkout adopted in place is left where it is, so nothing is
moved out from under anybody.

A process table that could not be read is not safe, the same rule as `not_checked`: for a home that
would move, `--check` answers refuse with the reason `unknown`, and a reclaim refuses before it runs a
hook or stops anything. `--force` moves the home to the trash after a snapshot of any work it holds.

A reclaim stops what the unit runs. It sends three signals in order, with a grace period between
each pair: `SIGINT`, then `SIGTERM`, then `SIGKILL`. A process that stops on one signal never gets
the next. It never signals its own process, the process that started it, or the process group
either of them is in.

Reclaim stops what carries the unit's id; it reports what only stands in the home. It signals two
kinds of target. The first is a process group the registry recorded for `nodal run --tether`. The
second is a process that carries the home's own `NODAL_ID`. Both are records that name the unit.

A reclaim never signals a process it matched by working directory alone. That match is also made by
a tmux pane, an editor server over SSH, and a teammate's shell. The verification lists each one by
command and process, with the sentence `standing in the home; not signalled`.

A reclaim refuses to move the home while such a process stands in it. The refusal names the command
and the process. `--force` moves the home anyway. The teardown has already run at that point, so a
refused reclaim leaves the unit live with its runtime stopped. A `part` note does not refuse the move:
the table was read, and a process this account cannot read is not reported on Linux either.

The move step reads the table again just before the move. Two processes do not refuse it there. A
process that the `nodal run` of a recorded tether started is the wrapper's own, such as the `git` that
records the run. A process that ended between the reading and the check is no longer in the home.

`nodal gc` makes the same split. It signals a recorded tether of a reclaimed materialisation, and a
process carrying the `NODAL_ID` of a unit whose materialisations have all been reclaimed. It reports
a process standing in a reclaimed home, under both the name the home had and the trash path it is
in now, and it signals none of them.

A reclaim of a unit a `done` pushed for deletes Nodal's own `refs/nodal/<unit>/*` on that remote,
and never the branch. It asks the remote which of those refs are there and deletes the ones it read.
A unit no `done` pushed for is decided in the registry, and no network is reached for it at all. The
remote is the one `done` uses: `origin`, then the only remote there is. A remote that cannot be
reached, cannot be decided, or refuses the deletion is a note in the report. It is never a refusal:
a reclaim ends a unit on this machine whatever a remote says.

A reclaim stops the unit's tethered process groups before it stops anything else. It gives back the
unit's ports and leases in the transaction that records the reclaim. It moves the home to `<state>/<project>/trash/<id>`, under the name the
home had.

**The trash holds the home without its build output and dependencies.** Once the home is in the
trash, reclaim removes the generated state from the copy. A directory goes only when both things
are true of it: an ignore rule covers it, and Nodal's exclusion table calls it regenerable. The
first is read from `git ls-files --others`, so a path any commit holds is never a candidate, and a
project that commits a `dist` directory keeps it. The second is the same table that decides what a
clone carries: `target`, `node_modules`, `.venv`, `.next`, `dist`, `build`, `.turbo` and the rest of
the rows a tool writes again.

Everything else stays. An ignore rule covers a `.env.local` and a local database file too, and no
tool writes those again, so the trash keeps them and the reclaim report names them under **local
state kept in the trash**. The report says how many bytes were dropped, and the trash row records
the same figure. A trashed home that is not a repository, a listing that could not be made and a
removal that was refused are each a note in the report. None of them fails the reclaim: the home has
already moved, and a build directory that would not go is not a reason to put a person's home back.

`nodal doctor` counts the trash in its header: how many reclaimed homes are waiting for `gc`, and
what they hold. It reads the directories and not the registry, so a machine whose registry this
Nodal will not open still gets the figure. It then reads back everything the unit had, by identifier, and reports what is still
there. It does not claim that the machine is clean.

Nodal never trashes two things. A checkout adopted in place is unregistered, and the directory
stays where it is: its rows are closed, its ports come back, and Nodal's own files — the marker, the
activation files, the memory and the lines in `info/exclude` — are taken back out, so the directory is
left as adoption found it. A base is not a home; `nodal base gc` collects it.

When that checkout is a linked worktree and the work is done and unique nowhere else, reclaim prints
`git worktree remove <path>`. It asks once, `[y/N]`, default no. `--yes` runs that command. When the
worktree holds anything unique, reclaim refuses as today and prints nothing runnable. The offer never
appears for a home Nodal made. Those go to trash as today. Nodal never removes a worktree it did not
make unless the person confirmed.

A reclaim reads the two signals `nodal ps` reads: the process table and the container daemon. A
signal Nodal cannot read becomes a note, never silence. A host with no readable process table refuses
the reclaim without `--force`, as the check does. A forced reclaim there says that it found nothing,
not that nothing is left, and the note says which signal went unread.

`nodal gc` removes a trashed home when `reclaim.trash_retention` days have passed. Nodal stamps
that window on the row when it moves the home. A recipe edited later cannot shorten a retention
that somebody relies on.

A snapshot record is kept differently, and the difference is worth stating. Nothing stamps a window
on a record: the sweep reads the recipe as it stands and measures that window from the record's own
commit. So a shortened `reclaim.trash_retention` shortens the window of every record already taken,
and a lengthened one lengthens it. Shorten the key only when you accept losing the records of runs
that are already older than the new window. `gc` also removes the pre-operation records of runs that are over, and gives
back lapsed leases. It stops runtime that belongs to a unit whose materialisations have all been
reclaimed, and the tethers of every materialisation that has been reclaimed. It never stops the
runtime of a live unit.

A **merged** unit keeps its home for the same `reclaim.trash_retention` window, measured from the
moment the merge was recorded, because the day after a merge is when somebody wants to look at what
they did. `gc` then reclaims it by the ordinary path, so the uniqueness check applies in full: a merged
unit somebody has since put new work in is refused and named in the report, not removed. Reclaiming is
not removing — the home goes to the trash with a retention of its own, and a later sweep takes it.

`nodal gc --idle [DAYS]` adds one section: the live units nothing has touched for that many days,
read from the session rows, defaulting to a week. It **reports** them and stops nothing of theirs. A
development server left running for a fortnight is somebody's work, and a command that ends one on a
timer without being asked is the hazard `doctor` was ruled out of for the same reason. A unit somebody
is still attached to is never reported, whatever the clock says.

## Registry schema and upgrades

The registry is one SQLite file, `registry.db`, in Nodal's state directory. SQLite's own
`PRAGMA user_version` records how far the file has come. **Version 0.1.0-rc.1 reads
registry schema 14.**

This number is not the version of the JSON schema catalogue in `schemas/`. The catalogue
is `v1` and is versioned by directory (`schemas/README.md`). The two numbers move
independently.

**An upgrade goes forward only.** A newer binary opens an older registry, applies each
numbered migration in order inside one transaction, and stamps the new version. The
person runs no upgrade command. A registry that is already current takes no write lock.

**An older binary refuses a newer registry.** It stops before it reads a row, and the
message names both numbers:

```
/home/you/.nodal/registry.db is at schema version 15, and this build understands 14
```

**There is no down-migration.** A registry that a newer binary migrated cannot be brought
back to an older schema. To go back to an older binary, keep a copy of `registry.db` from
before the upgrade, or reclaim the units and start a new state directory. Nodal does not
copy the file for you.

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
