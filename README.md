# Nodal

**One list for every coding agent on your project, whichever tool started it. Cheap units instead of fresh checkouts. Nothing left behind.**

Nodal gives every piece of engineering work its own isolated, durable home on top of shared project
state, so many agents on one repository cost a fraction of many checkouts, installs, build caches and
service stacks, and so the work survives whichever agent, engineer, terminal or machine is driving it.

## The problem

Run several coding agents on one project and each creates a worktree, installs dependencies, builds,
starts a dev server, and either shares your one local database with everything else or spins up its own
stack. Worktrees pile up. Nobody knows which are done. An agent in one worktree answers from code that
main replaced weeks ago.

## What Nodal is

A single CLI, `nodal`, and a local store. No daemon, no cloud, no new version control. Git and GitHub
stay exactly as they are. The unit of work is a **WorkUnit**: *a branch with a home directory and a memory.*

- **Branch**: identity every tool already understands. One WorkUnit is one PR-sized change.
- **Home directory**: a normal folder, its own repository, that any terminal, IDE or agent can open.
  Entering it activates the right ports and environment with no plugin.
- **Memory**: a `WORKUNIT.md` rebuilt from reality on every command, stating the unit's state and what
  sibling units have changed, so the next agent or engineer continues without a transcript.

## How it works

`nodal new "fix worker import"` clones a fresh, warm **golden base** copy-on-write (about two seconds
and a few megabytes on APFS or btrfs) as an independent repository on a new branch, allocates a port
and writes the environment into the folder. `nodal` lists every unit with how far behind main it is,
what it touched, what is running, whether it is done (squash merges included), and what it was doing.
`nodal merge` commits, squashes, rebases, fast-forwards and cleans up in one command. `nodal doctor`
reports what agents left behind, and never deletes it. `nodal reclaim` removes a unit into a trash that
`nodal gc` empties later, and refuses if unique work would be lost.

With Claude Code, Nodal installs as hooks: `claude --worktree` and desktop sessions create Nodal units
instead of bare worktrees, context is injected at session start, and cleanup is deterministic.

## Status

Pre-alpha; design complete, implementation starting. See `docs/contracts.md`, `docs/code-structure.md`
and `docs/scenarios.md`.

## License

MIT.
