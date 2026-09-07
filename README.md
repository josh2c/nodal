# Nodal

**One list for every coding agent on your project. Cheap work units instead of full checkouts. Nothing left behind.**

## The problem

Run several coding agents on one project. Each agent creates a worktree. Each worktree installs dependencies, builds, and starts a dev server. The worktrees share one local database, or each starts its own service stack. Disk fills. Nobody knows which worktrees are done. An agent in an old worktree answers from code that main replaced weeks ago.

## What Nodal is

Nodal is one CLI (`nodal`) and a local store. It has no daemon, no cloud, and no new version control. Git and GitHub do not change.

The unit of work is a **WorkUnit**: a branch with a home directory and a memory.

- **Branch**: an identity every tool understands. One WorkUnit is one PR-sized change.
- **Home directory**: a normal folder with its own repository. Any terminal, IDE, or agent can open it. The folder supplies the correct ports and environment. No plugin is necessary.
- **Memory**: a `WORKUNIT.md` file. Nodal writes it again on each command. It states the unit's condition and what sibling units changed. The next agent or engineer continues without a transcript.

## How it works

- `nodal new "fix worker import"` clones a warm **golden base** with a copy-on-write clone. The clone takes approximately two seconds and a few megabytes on APFS or btrfs. The unit is an independent repository on a new branch, with its own port and environment.
- `nodal` lists every unit: how far behind main, what it touched, what runs, whether it is done. Done detection includes squash merges.
- `nodal merge` commits, squashes, rebases, fast-forwards, and removes the unit in one command. It shows the plan first.
- `nodal doctor` reports what agents left behind. It deletes nothing.
- `nodal reclaim` moves a unit to trash. It refuses if the unit holds work that exists nowhere else. `nodal gc` empties the trash later.

With Claude Code, Nodal installs as hooks. `claude --worktree` and desktop sessions then create Nodal units instead of bare worktrees. The session receives the unit's context at start. Cleanup is deterministic.

## Status

Pre-alpha. The foundation layer is complete: domain model, published JSON schemas, registry, git operations, environment fingerprints, project recipes, and an operation journal with crash recovery. The commands above are under construction. See `docs/contracts.md`, `docs/code-structure.md`, and `docs/scenarios.md`.

## License

MIT.
