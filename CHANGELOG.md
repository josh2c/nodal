# Changelog

This file records what each release of Nodal lets a person do, and what it refuses.
One line per behaviour. Versions follow [semantic versioning](https://semver.org).

## Unreleased

### Read

- `nodal doctor` reads the remote itself where `origin` is a directory on this machine, and
  the branch and worktree rows then say "only here" and "on the remote". Where `origin` names
  a server nothing here can read it, and the rows keep the weaker words, "unpushed" and "seen
  on a remote". The line a table with nothing to report prints says which reading it made. It
  called a branch seen on a remote after a push, a merge and a remote branch deletion without
  a prune.
- `nodal ls` prints one note for a unit whose home is not on this disk. It runs no `git` in
  the missing directory, and no longer prints what each `git` said about it.
- `nodal ls` prints `unknown` in `NEEDS` for every home when the process table could not be
  read, with the reason under the table. It no longer reads an unread table as nothing
  standing in the home.
- `nodal doctor --machine` names a directory the walk took for a clone and Git could not
  open once, with what the walk saw: "taken for a clone because its `.git` is a file naming
  a Git directory, and Git finds no repository there".
- `nodal reclaim --check` puts the witness clause of a `commits` row on a line under the
  row. The row ends at "a reclaim keeps this home"; the clause and its instruction follow
  whole.
- The `nodal` shell function finds the binary each time it runs: the path it was printed
  with, else the one on the `PATH`. When neither holds one, it prints "nodal is not on the
  path" and exits 127, where it ran an empty command and printed `permission denied`.

### Locks

- A unit's write lock records the process that took it as a pinned identity: its
  identifier and the instant it started. A hold is read as `gone` only where a reading of
  this host's whole process table does not hold that identity. A process the reading
  cannot date on both sides, an unreadable table, a row that names no process and a hold
  taken on another machine are all `unknown`, and none of them frees a hold.
- `nodal show` and `nodal ls` no longer say "pid 4120 is not on this host any more" about
  a hold whose own process is running. The reading compared the recorded process against
  the instant the hold began, which a refresh keeps while it writes the refreshing
  process's identifier, so every refreshed hold read as a recycled number. The refusal
  that quoted that reading told the next actor a held home was free
  (`crates/nodal-core/src/runtime/lock.rs`, `tests/safety/tests/lock_liveness.rs`).
- A hold moves on three grounds and each writes its own `handoff` line on the unit's log:
  a reading proved the holder gone, the lease expired on the clock, or a person asked with
  `--take`. A lapsed hold used to move with nothing written down. A hold that came back to
  the holder it already had writes no line: the same actor re-entering its own home after
  the idle window is not a hand-off, and the log no longer says "taken from ada by ada".
- A holder's process is read as the one the row pinned when the two instants are within a
  second. The instant is derived from the boot instant, which some kernels recompute, so
  exact equality reported a running holder gone on a second of arithmetic.
- This build reads registry schema 17. `lock.pid_started_at` holds the instant, null for every
  row written before it.

### Reclaim

- `nodal reclaim` reads every ref the home holds, and not the branch it is on alone. A commit
  on a side branch, a stash or a tag is work the home holds, and the verdict said nothing
  about it. Nodal's own `refs/nodal/*` are left out: two are copies of the person's checkout,
  and the rest are records of runs whose trees the working tree holds.
- A repository supplies a second copy only where it holds the work behind the commits. A
  partial clone, a clone that borrows its objects, a shallow clone and a worktree of the home
  hold every commit and cannot produce the content; each is read before it may vouch, and a
  store that passes is walked for the objects those commits add. A store that fails leaves the
  commits `not_checked` and the row names the directory and the property that failed.
- A reading of a remote is dated per branch, from `FETCH_HEAD`, which Git rewrites on every
  fetch with one line per ref the fetch saw. A branch the last fetch did not see is a branch
  the remote had not got, whatever the tracking ref still names, and a reading older than the
  home's own record of a branch proves nothing about it. `packed-refs` no longer dates a
  reading: rewriting it is not hearing from anything.
- A commit a store holds only under its own `refs/remotes/*` is no longer a second copy. One
  `git fetch --prune` there deletes such a ref, exactly as `git gc` deletes an object under no
  ref.
- Every `git` Nodal runs is held to the disk it reads. A partial clone fetches a missing object
  the moment anything asks for one, and a reading of somebody else's repository was the last
  place the no-network rule could leak.
- `nodal reclaim` prints a `record` line naming the ref the home was committed to before
  the first step, `refs/nodal/<unit>/pre/<operation>`, for every reclaim that took one.
- `nodal reclaim` names on its `check` line where the commits it did not refuse over
  also live: the repository that holds them and the refs in it. The trash row records the
  same, so a later sweep can name the copy the verdict rested on.
- `nodal gc` reads every expired home again before it removes it, with the reading a
  reclaim makes, over the refs that home holds. A commit no ref outside the directory
  reaches keeps the home and its row, and one line names the commit and the copy that is
  gone. Nothing is removed on a reading that could not be made. A home a reclaim forced
  past a finding is removed on its retention as before.
- `nodal reclaim` writes into the home of the unit it names and into the registry, and
  into nothing else. It no longer rewrites `WORKUNIT.md`, fetches refs or writes a tree
  object into every other open home. The report says so, and the next command that reads
  those units writes their memory again.
- A reclaim of a managed home refuses over a process holding a path inside it, not only
  over one standing in it: a descriptor opened for writing, a file mapped writably and
  shared, or a root inside the home. The refusal names the process, its command and the
  path. A descriptor opened for reading refuses nothing. Linux publishes the open flags
  that separate the two; macOS does not, and occupancy there stays the working directory.
- A process this account may not read is kept in the reading with what was refused said
  out loud, where a Linux scan used to leave it out without a word. It refuses a managed
  move when its parent, group or session reaches something already found in the home.
- Every verdict carries an `evidence` record: the stores asked and what each said, the
  refs walked and the refs not walked, how much of the process table was read and how many
  entries were refused, what was not checked and why, and the instant of the reading.
  `nodal reclaim --check` prints it and still writes nothing; an executed reclaim writes it
  into the unit's log as a `verdict` event. Nothing in the record changes a verdict.

### Make

- `nodal new`, `nodal adopt` and `nodal show` read a Node build at the directory the
  project says it writes: an output flag or path in the build command or its
  `package.json` script, a tool's fixed output such as `.next`, or the task cache's
  `build` outputs. The line reads `not checked` only when the build names no directory.
- A readiness part nothing here could read is labelled `not checked`, apart from `not
  ready`, which is a part whose file is not there.

## 0.1.0-rc.3 — 2026-09-18

### Read

- `nodal new`, `nodal adopt` and `nodal show` print one line for each tool the project
  pins. The line names the tool, the pin, and the version this host answered. A tool this
  host has no program for reads `not checked`, with the reason.
- `nodal reclaim --check` names the repository that holds the second copy of a commit.
  The path is what the claim rests on, so the report states it.
- `nodal reclaim --check` prints one `content` row for a refused commit whose tree a
  remote tip already holds. The row names the ref, its tip and the tree. It moves no
  verdict, because the ref rebuilds the content and not the commit.

### Make

- `nodal new` installs the dependencies in the home when `base.exclude` keeps the install
  output out of the base. The base runs no install for that manager, and its readiness
  line names the directory and the manager. Cargo never moves, because its download cache
  is outside the tree.
- A base build and a home install run the form of each package manager that installs
  from the lockfile and refuses to change it: `npm ci`, `pnpm install --frozen-lockfile`,
  `yarn install --immutable` (`--frozen-lockfile` when the project pins Yarn 1),
  `bun install --frozen-lockfile`, `uv sync --frozen`, `cargo fetch --locked`. A project
  with no lockfile keeps the plain install, and the progress line says so.
- `nodal init` prints one line when `package-lock.json` records a name or version that
  `package.json` no longer states. The line names both.

### Reclaim

- `nodal reclaim --check` takes more than one unit. Each unit keeps the verdict it would
  get alone. A second copy that lives only in another home on the same command line
  becomes one more reason on that unit. The report closes with the joint verdict, and the
  exit code carries it.
- `nodal doctor` asks the same question of a project's open homes as a set. A home of one
  of them is not a second object store, because a person clearing a machine removes them
  together.

### Read

- `nodal reclaim --check` on a checkout adopted in place names the build output and the
  installed dependencies it holds, and says that `--prune` is what removes them. It said
  before that a trash would keep the local state of such a home. No trash holds it,
  because no reclaim moves the home.
- `nodal doctor --machine` names `nodal adopt <path> --in-place` for the clones it found,
  and `nodal reclaim <unit> --check` after it. It runs neither.

### What it does

- `nodal reclaim <unit> --prune` removes the build output and the installed dependencies
  from a checkout adopted in place. Every other reclaim of such a checkout leaves the
  directory exactly as it is, and its report names what `--prune` would remove.

### What refuses

- `nodal reclaim --prune` removes a path only when an ignore rule covers it and the
  exclusion table calls it regenerable. It never removes a tracked file, and it never
  removes the directory. A reclaim that refuses over work prunes nothing.

- `nodal new` and `nodal adopt` ask for hook approval before the operation writes
  anything. A create or an adoption refused for a hook nobody approved leaves no unit, no
  branch, no port lease and no home.
- An install that changes a file the project tracks is refused. The change is put back
  in the base or the home it ran in, never in the checkout, and the refusal names the
  file and the tool. This holds for every package manager.
- A frozen install that fails because the lockfile disagrees with its manifest is refused
  with the tool's own sentence and one line of Nodal's: which file disagrees with which,
  and that the fix goes in the project's checkout. The create leaves no unit.

## 0.1.0-rc.2 — 2026-09-17

### Read

- macOS reads the process table, so `nodal ls`, `nodal ps` and `nodal reclaim --check`
  name the unit a process belongs to. Where macOS refuses the variables or the directory
  of a process, the reading gives a note with the reason.

### What refuses

- A second copy of a commit counts only where a ref of that repository reaches it. An
  object under no ref is one `git gc` removes, so `nodal reclaim` refuses over it.

## 0.1.0-rc.1 — 2026-09-16

The first candidate release. It reads registry schema 14 and publishes JSON schema
catalogue `v1`.

### Read

- `nodal` lists every worktree and unit of the project, with its work, its integration
  state and its age.
- `nodal doctor` reports what development tools left on this machine. It removes nothing.
- `nodal doctor --machine` reads every clone under your home directory, grouped by remote.
- `nodal show` reports one unit in full and writes the unit's memory again.
- `nodal explain` reports why a home is as it is: its base, what it did not receive, what
  was removed from it, and where its ports came from.
- `nodal ps` names the unit that a running process or a bound port belongs to.
- `nodal env` reports the variables a home carries.
- `nodal base` lists the warm bases that unit homes are cloned from.
- On a host with no process table, `nodal ls`, `nodal ps` and `nodal reclaim --check`
  name the signal they could not read. They do not print a zero.

### Make

- `nodal init` writes `nodal.toml` with one line for each gap the inference cannot fill.
- `nodal new` makes a unit: a branch, a home cloned from a warm base, ports of its own,
  and an environment.
- `nodal new --carry` does the same and copies your uncommitted work into the new home.
- `nodal adopt` registers a worktree or a checkout where it stands. It does not move it.
- `nodal base build` builds a warm base and installs the project's dependencies in it.

### Work

- `nodal cd` moves the shell you are in to a unit's home.
- `nodal shell` starts a shell that carries a unit's environment.
- `nodal run` runs a command in a unit's environment and records the command in the
  unit's log.
- `nodal handoff` leaves a note on a unit for the person or agent who continues it.
- `nodal approve` accepts, on this machine, the hook commands that the project declares.

### Integrate

- `nodal done` pushes a unit's work and prints where to open the change. It does not
  open a pull request, and it makes no call to a host API.
- `nodal merge` commits, squashes, rebases, fast-forwards the target, and removes the
  unit.
- A rebase that stops for a conflict leaves the home in the middle of the rebase. A
  second `nodal merge` continues it. `nodal merge --abort` puts the branch back where
  the merge found it.

### Reclaim

- `nodal reclaim` ends a unit: it stops what the unit runs, gives back the unit's ports,
  and moves the home to trash.
- `nodal reclaim --check` reports the decision and removes nothing.
- `nodal gc` reclaims merged homes after their retention, and removes trashed homes after
  theirs.
- `nodal uninstall` takes back the shell integration and the provider hooks, file by file.
  Each unit home stays a standalone Git repository.

### Integrate with agents

- `nodal mcp` answers an agent's tool calls on standard input, as a model context protocol
  server.
- `nodal claude-code` answers Claude Code's hooks.
- `nodal shell-init` prints the shell integration for bash, zsh and fish.

### What refuses

- `nodal reclaim` refuses a home that holds work which exists nowhere else, and names
  what it found: uncommitted changes, untracked files, or commits no remote has.
- `nodal reclaim` refuses to move a home while something that carries no unit identifier
  stands in it. The teardown stops what carries the identifier and leaves the rest.
- `nodal reclaim` refuses to move a home while the process table could not be read.
  Nothing found is not nothing there. On a host with no process table, `--force` moves
  the home after a snapshot of the work it holds.
- `nodal uninstall --state` refuses while a home holds work that exists nowhere else.
- `nodal merge` refuses to fast-forward a target branch that moved under the rebase. No
  flag overrides this. The person runs the merge again.
- `nodal new` refuses a branch that an open unit holds, and names the holder.
- `nodal base gc <base>` refuses to remove a base that units still hold, and says how
  many hold it. A sweep with no base named steps over it instead.
- A hook runs only where a person approved its exact command line. Nodal refuses any
  other command line and names the project to run `nodal approve` in.
- Nodal refuses a registry that a later version wrote. The message names the schema
  version in the file and the schema version this build understands.

### Records

- A unit home carries `.nodal/manifest.toml`, which records the version of the binary
  that made the home.
- Nodal writes its own files only where Git does not track them, and hides each one in
  the home's `info/exclude`.
- Nodal makes no network call of its own. It runs no update check and sends no telemetry.
- Two units of one project cannot read or write each other's home.
