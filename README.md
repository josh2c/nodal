# Nodal

> **Warning: pre-alpha.** The foundation layer is built and tested. The commands below are under construction. Interfaces and on-disk formats may change without notice.

One list for every coding agent on your project. Cheap work units instead of full checkouts. Nothing left behind.

- work units are copy-on-write clones: seconds to create, a few megabytes each
- one list across every tool: how far behind main, what changed, what runs, what is done
- each unit knows what its sibling units changed, so agents do not answer from stale code
- deterministic cleanup: trash, gc, and a doctor that reports and never deletes
- no daemon, no cloud, no new version control; Git and GitHub do not change

## The problem

Run several coding agents on one project. Each agent creates a worktree. Each worktree installs dependencies, builds, and starts a dev server. Disk fills. Nobody knows which worktrees are done. An agent in an old worktree answers from code that main replaced weeks ago.

## The unit of work

A **WorkUnit** is a branch with a home directory and a memory.

- **Branch**: an identity every tool understands. One WorkUnit is one PR-sized change.
- **Home directory**: a normal folder with its own repository. Any terminal, IDE, or agent can open it. The folder supplies the correct ports and environment. No plugin is necessary.
- **Memory**: a `WORKUNIT.md` file, compiled and never edited. `nodal show` writes it again every time it is asked. It states what the unit is for and how that is known, the unit's condition, what sibling units changed, and the unit's own log. The next agent or engineer continues without a transcript.

## Platforms

| Platform | Backend |
| --- | --- |
| macOS (APFS) | `clonefile` per entry, filtered walk for excludes |
| Linux (btrfs, XFS, bcachefs) | `FICLONE` per file, into a directory of its own |
| Any other filesystem | a copy of the bytes, with a warning |
| Windows | via WSL2, as Linux |

## Commands

- `nodal new "fix worker import"` clones a warm **golden base** as an independent repository on a new branch, with its own port and environment. The clone takes approximately two seconds.
- `nodal` lists every unit: how far behind main, what it touched, what runs, whether it is done. Done detection includes squash merges.
- `nodal merge` commits, squashes, rebases, fast-forwards, and removes the unit in one command. It shows the plan first.
- `nodal ps` reports what runs on this machine and which unit owns it: processes, containers and bound ports. Every row states one of two confidences. `certain` means the thing names its unit. `probable` means Nodal inferred the unit from where the thing is.
- `nodal adopt` brings an existing worktree or checkout under management without moving it.
- `nodal doctor` reports what tools left behind: stale worktrees, dead containers, orphan caches. It deletes nothing. A registry written by a later Nodal stops every other command; doctor reports what it can still read and tells you which command upgrades this copy.
- `nodal run --tether <command>` gives the command a process group the unit owns. `nodal reclaim` stops the whole group, and a group that outlived the `nodal run` that started it is still stopped.
- `nodal reclaim` moves a unit to trash. It refuses if the unit holds work that exists nowhere else. `nodal gc` empties the trash after a retention period.
- `nodal uninstall` takes back what Nodal put on the machine. It names every item first, then removes the lines in your shell start-up file and the scripts they load. `--state` also removes the state directory, and it refuses while a unit home holds work that exists nowhere else.
- `nodal upgrade` reports how this copy was installed and prints the one command that upgrades it. `nodal update` is the same command. Nodal makes no network calls of its own: no update check, no telemetry. The only network activity Nodal causes is the `git` you configured talking to the remotes you configured.

## Agents

Nodal is agent-neutral. A unit is a folder; the environment comes from the folder. Any tool that can open a directory can use a unit, with no integration at all.

Optional per-tool integrations go further:

| Tool | Integration |
| --- | --- |
| Claude Code | hooks: worktree requests become units, context loads at session start, cleanup is deterministic |
| Codex | an `AGENTS.md` pointer line; unit context in `WORKUNIT.md` |
| Cursor, VS Code, any IDE | open the unit folder; the integrated terminal is already activated |
| Plain terminal | `nodal shell-init` for your shell, or `cd` and `.envrc` |

Adoption works in the other direction too: worktrees that other tools already created can be adopted with their original intent recovered where the tool recorded it.

## Status

The foundation layer is complete and tested: domain model, published JSON schemas (`schemas/v1/`), SQLite registry, git operations, environment fingerprints, project recipes with inference, an output layer, and an operation journal with crash recovery. Environment activation writes a unit's `.nodal/env`, `.envrc` and `.nodal/manifest.toml`, and `nodal env` reports what a home carries. `nodal new` makes a unit from a golden base. It gets the base for the workspace first. If there is no base yet, it builds one and reports each step on standard error. It then clones that base into a home of its own, scrubs the Git state the clone inherited, takes the unit's branch, grants ports, and writes one registry row set at the end. A unit therefore holds nothing the checkout left uncommitted. The next unit of the same workspace clones the same base and builds nothing. A run that a kill stops part-way through leaves nothing once the next command resolves it. `nodal shell-init` prints a shell function for bash, zsh or fish, so that a terminal entering a unit carries its environment and `nodal cd` moves the shell you are in; `nodal run` records what it ran, and `nodal run --tether` gives a command a process group the unit owns and a reclaim stops. `nodal ps` names the unit a running process, container or bound port belongs to, and states a confidence on every row. A signal this machine does not have becomes a note under the table. Warm bases are built. The first base of a project is a fresh clone of its remote. A project with no remote gets a fresh clone of its own checkout. Each later base is a copy of the nearest base already on the machine. `nodal base` lists, builds and collects them. `nodal ls`, and `nodal` on its own, list every unit of a project: what the working tree holds, how far the branch has moved from the branch it merges into, what the remote has, who is attached, and one integration verdict per unit. The verdict is read with `git merge-tree`, so a squash merge counts as done although no commit of the unit is on the base. The units the base has moved furthest under are printed first and the finished ones last. `nodal merge` takes one unit from a dirty home to a merged branch in one command: it commits what the home holds, folds the branch into one commit, rebases it onto the branch it merges into, fast-forwards that branch in your own checkout, and reclaims the unit. It shows the plan first and asks once. Each stage has a flag that drops it. It moves the target branch only when that is a fast-forward, it pushes nothing, and it keeps every commit it folded on a ref inside the home. A rebase that stops for a conflict is reported with both ways out: run the command again to carry on, or `--abort` to put the branch back. `nodal adopt` makes a unit of work that is already on the machine. A checkout or a linked worktree becomes a unit where it stands: the only writes are `.nodal/` and `.envrc`, both excluded from Git before either is written, so `git status` in that directory is byte for byte what it was, and the directory is a root a reclaim unregisters rather than moves. A branch nothing has checked out gets a home of its own from a base instead, carrying commits the base has never had. Where nothing says what a worktree was for, adoption recovers it from the records of the session that made it and records it as recovered rather than stated, which every rendering says. `nodal show` prints one unit in full and rewrites its `WORKUNIT.md`, and `nodal explain` says why a home is as it is: which base and why, what the clone left out and who decided each row, what was removed from the copy afterwards, and which block the ports came from. The rest of the command surface is under construction.

See `docs/contracts.md`, `docs/code-structure.md`, and `docs/scenarios.md`. Contributions: `CONTRIBUTING.md`.

## License

MIT.
