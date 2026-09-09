# Contracts

Stable surfaces that other tools can rely on. Changing one requires a documented decision and a version bump.

Machine-readable form: the types behind these surfaces are published as JSON Schema in
`schemas/v1/`, generated from the model and diffed in CI (`schemas/README.md`).

## Directory contract
A unit's home contains `.nodal/id` (marker, verified against the registry), `.nodal/env` (dotenv),
`.nodal/manifest.toml`, `WORKUNIT.md` (facts about the unit and its siblings), `.envrc` (`dotenv .nodal/env`),
and a normal `.git` directory. Nothing else is required for a terminal, IDE or agent to integrate.

Nodal adds `.nodal/`, `.envrc` and `WORKUNIT.md` to the repository's common `info/exclude`, so
`git status` in a unit stays clean. A vendor file Nodal itself created is added there too. Every
worktree of a repository shares that one exclude file — a linked worktree's own
`.git/worktrees/<name>/info/exclude` is not read — so a unit adopted in a nested worktree writes the
block into the repository the project shares. The names are Nodal's own, and `nodal reclaim` takes
both them and the files back out again.

`.nodal/manifest.toml` states the identity of the home, every environment name it carries, the
origin of each name, and every declared name that no source answered. It holds no value. Its shape
is published as `schemas/v1/manifest.json`.

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

A shell reads them by one of two routes. direnv reads `.envrc`, which reads `.nodal/env`. A shell
with no direnv evaluates `nodal env --export`, which is what the hook `nodal shell-init` installs
does. That one evaluation is the only one in the hook, and what it evaluates is Nodal's own output,
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

## CLI
`init, new, cd, adopt, ls, show, explain, env, shell, shell-init, claude-code, run, ps, start, note, ask,
handoff, sync, done, merge, prune, reclaim, gc, doctor, base, status, uninstall, upgrade`. Every read command accepts
`--json`; `status --watch` emits newline-delimited JSON. Global `--store` and `--no-hooks`.

`nodal shell-init <shell> --install` writes the script into `<state directory>/shims/` and adds one marked block
to that shell's start-up file. The block sources the file. It evaluates nothing and starts no process, because it
runs in every shell a person opens. `nodal shell-init <shell>` on its own still prints the same script, and
`eval "$(nodal shell-init bash)"` in a start-up file still works.

`nodal uninstall` removes the block, the scripts, and with `--state` the state directory. It prints one item per
thing before it removes any of them, and it asks once; a terminal nothing is watching is refused rather than
waited on. A start-up file is byte-identical to the file it was before the install. `--state` runs
`lifecycle::uniqueness` over every unit home first and refuses while one holds work that exists nowhere else;
`--force` accepts that and says what it accepted.

`nodal upgrade`, and `nodal update`, report how this copy was installed — a cargo bin directory, a Homebrew
cellar, a system package path, or a binary placed by hand — and print the one command that upgrades it there.
Nodal has no self-updater and **makes no network call of its own** (DL-034): no update check, no telemetry, no
version comparison. `tests/safety/tests/no_network.rs` asserts that no code path in either crate could make one.

`done <unit>` sends a unit's work for review. It pushes two refs — the unit's branch, and a
work-in-progress snapshot of everything the home holds that no commit does — with **one** `git push`,
which is the only thing Nodal does that reaches a network. The push is the user's own `git`, so the
credentials and the hooks are theirs, and the report says so in the line it prints. The branch goes as
it is; a push that would not fast-forward is refused by the remote and reported. Only Nodal's own
`refs/nodal/` ref is replaced, because each snapshot is built from the working tree rather than on the
last one.

It then prints the page a person opens the change on, for the host the remote names, and **opens no
pull request**. There is no host API in Nodal and no client of one; a remote whose host Nodal has no
compare page for is told so rather than guessed at. The unit moves to `review`.

`adopt <branch-or-path>` makes a unit of work that is already here, in one of two forms.

`--in-place` makes a checkout or a linked worktree a unit **where it stands**. The only writes are
`.nodal/` and `.envrc`, and both are excluded from Git before either is written, so `git status` in
that directory is byte for byte what it was. Nothing is cloned, no branch is created, and no file of
the person's is touched. The environment row carries `managed = false`, which makes the directory a
root: a reclaim unregisters it and never moves it. A directory can be adopted no other way, so
`--in-place` is stated rather than inferred.

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
unreferenced volumes, orphan databases and a project over the open-unit threshold, each with a size, in two
sections: this project, and a separate section for another project's leftovers that carries names and sizes
only. A worktree another tool holds a lock on is reported as locked and read no further. Removal of
unmanaged state is a later command (`decisions/DL-015`).

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
the registry was read, and the one command that upgrades this copy of Nodal. Nothing is fetched to say it
(DL-034).

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

A directory is in one of three states, and each gets a different answer. A project the registry
holds units of gets the table. A project that holds a `nodal.toml` and no units gets the empty list,
with a note that says which command makes the first unit; `nodal init` writes the recipe and opens
no registry, so this is the state of every project between `init` and the first `new`. A directory
that has neither a recipe nor a row is in no project, and `nodal ls` refuses. A bare `nodal` prints
the help for the third state only.

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
record; a scan is an inference. `nodal gc` stops a tether whose materialisation has been reclaimed,
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

Hook commands require approval. `nodal init` approves the set the project declares. It pins each
command by the digest of its exact text. The record is per machine, in `<state>/hooks.toml`;
`NODAL_HOOKS_FILE` moves that file. A command that has changed refuses to run, and the message
shows the command. A command nobody approved refuses in the same way. `--no-hooks` runs no hook
and needs no approval.

## Claude Code
Claude Code fires named events at commands a project declares in `.claude/settings.json`. `nodal init`
offers to write four of them and installs them when the answer is yes; `--claude-hooks` installs them
without asking and `--no-claude-hooks` does not ask. `nodal uninstall` removes them again. Each command is
the word `nodal` and a subcommand, and names no path of one machine, so the file is the same on every
machine that has Nodal on its `PATH`. **Commit that file or do not: it is the project's, and Nodal reads it
the same either way.** A clone of it on a machine with no Nodal does nothing.

| event | kind | what Nodal does |
|---|---|---|
| `WorktreeCreate` | provider | `nodal claude-code worktree-create` makes the unit, carries the project's settings into its home, records the attachment, and prints the home |
| `SessionStart` | observer | prints the unit's memory, which Claude injects as context |
| `Stop` | observer | records the session's last message as a stated handoff |
| `WorktreeRemove` | observer | records a detach if it ever fires, and removes nothing |

`WorktreeCreate` is a **provider**, not an observer: Claude reads one absolute path from its standard
output and uses that directory, and empty or invalid output ends the session. Nodal's answer therefore
overrides Claude's own worktree creation even in a Git repository, and Claude makes no `.claude/worktrees/`
entry. The unit's objective is the slug Claude derived from the opening prompt, recorded as `observed`
rather than `stated`: it is a reading of somebody's intent, not a statement of one.

The hook that cannot answer prints `./nodal-worktree-create-refused` — a relative path with a dot segment,
which Claude rejects — and says why on standard error. That is deliberate: printing nothing ends the
session just as certainly and says nothing about why. The two reasons are a project with no `nodal.toml`
and a machine with no `nodal`, and the second is handled by the command text itself.

`SessionStart` fires more than once for one session, with a different session identifier each time, and the
create payload carries a third. **Nothing correlates by session identifier.** The `cwd` a payload carries
is what names the unit, and a unit home says whose it is in `.nodal/id`.

**The provider carries the project's settings into the home it answers with.** Claude Code reads
`.claude/settings.json` from the directory a session works in. `WorktreeCreate` moves the session out of
the project and into a unit home, so the file that declared the hooks is no longer in scope. Without a file
there the three observers never fire in a session started with `--worktree`: no memory is injected and no
handoff is recorded. This was measured on 2026-09-08, headless and interactive, and it was the same in
both.

**What goes there is the project's own file, copied.** Not a regenerated set of four hooks: that file
would be the only settings in scope for the rest of the session, so the project's permissions, its deny
rules and every hook somebody else installed would stop applying the moment the session moved. A project
with no settings file of its own gets the four hooks.

**Nodal never assumes a project ignores `.claude/`.** What the clone carries decides which of two cases
this is, and the home is new, so a settings file already in it is one the project commits and Git tracks.

- **The clone carries none.** The file Nodal writes is Nodal's own. `/.claude/settings.json` goes in the
  home's `.git/info/exclude`, the way `WORKUNIT.md` does, and the uniqueness check names it. So
  `git status` in a new home is empty, `nodal reclaim`, `nodal done` and `nodal gc` do not call the home
  dirty, and `nodal merge` commits nothing of Nodal's onto the unit branch. The write is atomic, as every
  file Nodal writes into a home is.
- **The clone carries one.** It is left byte for byte as it arrived, for the reason a tracked `CLAUDE.md`
  is left alone: rewriting it would put the home permanently in `git status` and the rewrite in the diff of
  every pull request the unit opens. When that file declares none of Nodal's hooks, one `note` event says
  the observers will not fire and what would put them back.

**A `WorktreeCreate` fired from inside a unit home answers that home and creates nothing.** A home now
carries the provider hook and also carries the project's recipe, so making a unit of it would register the
home as a project of its own and clone a unit of a unit.

`WorktreeCreate` also records one `attached` event, `observed`, with `claude-code` as the actor. It is the
one hook that is certain to have run, so the record of a session taking a home does not depend on an
observer firing. **Neither that record nor the settings file may fail the create.** A store or filesystem
error is one line on standard error, and a note event where the store allows one; the home is still
answered with. Ending the session there would leave a fully built unit with nobody in it, which is the
failure this whole path exists to prevent.

`nodal uninstall` surveys the homes of registered units as well as project roots, and takes only Nodal's
own region out of each settings file it finds. A home left carrying the provider hook after the binary has
gone would answer a later `claude --worktree` with `nodal: not on PATH` and end the session over a tool
the person removed.

`WorktreeRemove` fired in **none** of four measured session lifecycles, and nothing depends on it. Cleanup
is Nodal's own lifecycle: the unit persists when the session ends, `nodal ls` shows it, and `done`, `merge`,
`reclaim` and `gc` retire it. A unit outliving the session that made it is the product working, not a leak.

The settings file is edited, never rewritten. What Nodal adds is one contiguous region of text it can write
again, so removing it leaves the file byte for byte the file it was, with every other key and every hook
somebody else installed still in it. A file that was reformatted since the install loses the hooks by a
re-rendering of the document instead, which is the only path that is not byte-identical. A file holding
nothing but Nodal's hooks is removed, and `.claude/` goes with it when that empties the directory.

## Reclaim, trash and gc
Every destructive path calls one uniqueness check. It reports three things: uncommitted changes,
untracked files that no ignore rule covers, and commits that no remote and no other tree on this
machine has. A hit refuses the operation and names the paths. `--force` does not skip the check.
It first commits the whole home to `refs/nodal/<unit>/wip`, then goes on.

A reclaim stops what the unit runs. It sends three signals in order, with a grace period between
each pair: `SIGINT`, then `SIGTERM`, then `SIGKILL`. A process that stops on one signal never gets
the next. It never signals its own process, the process that started it, or the process group
either of them is in.

A reclaim stops the unit's tethered process groups before it stops anything else. It gives back the
unit's ports and leases in the transaction that records the reclaim. It moves the home to `<state>/<project>/trash/<id>`, under the name the
home had. It then reads back everything the unit had, by identifier, and reports what is still
there. It does not claim that the machine is clean.

Nodal never trashes two things. A checkout adopted in place is unregistered, and the directory
stays where it is: its rows are closed, its ports come back, and Nodal's own files — the marker, the
activation files, the memory and the lines in `info/exclude` — are taken back out, so the directory is
left as adoption found it. A base is not a home; `nodal base gc` collects it.

A reclaim reads the two signals `nodal ps` reads: the process table and the container daemon. A
signal Nodal cannot read becomes a note, never silence. A host with no readable process table
still reclaims the home and still gives back the ports. The verification there says that it found
nothing, not that nothing is left, and the note says which signal went unread.

`nodal gc` removes a trashed home when `reclaim.trash_retention` days have passed. Nodal stamps
that window on the row when it moves the home. A recipe edited later cannot shorten a retention
that somebody relies on. `gc` also gives back lapsed leases. It stops runtime that belongs to a
unit whose materialisations have all been reclaimed, and the tethers of every materialisation that
has been reclaimed. It never stops the runtime of a live unit.

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
