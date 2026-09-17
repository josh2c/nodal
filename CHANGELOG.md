# Changelog

This file records what each release of Nodal lets a person do, and what it refuses.
One line per behaviour. Versions follow [semantic versioning](https://semver.org).

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
