# Nodal

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org)
[![CI](https://github.com/josh2c/nodal/actions/workflows/ci.yml/badge.svg)](https://github.com/josh2c/nodal/actions/workflows/ci.yml)

**[Install](#install)** · **[Try it](#try-it-on-a-repository-you-already-have)** · **[Commands](#commands)** · **[Docs](docs/contracts.md)** · **[Contributing](CONTRIBUTING.md)**

**Git made branches cheap. Worktrees made branches parallel. Nodal makes their
environments cheap, durable, and manageable.**

See every worktree you already have, manage them safely in place, or create Nodal units
that arrive ready to run.

> **Pre-alpha.** The foundation is built and tested. Parts of the command surface are
> not. Interfaces and on-disk formats may change without notice.

## What it looks like

An example of the answer, in a repository Nodal has never been told about. It reads the
worktrees, answers, and writes nothing. The names are made up; the shape and the wording
are what the command prints.

```console
$ nodal

  ~/projects/acme  (no project of nodal's; nothing was written)

  WORKTREE          FOR  DONE             ONLY HERE  BEHIND             SIZE    AGE
  ../acme-t8        —    conflict         ^1         -49 (origin/main)  141 kB  21 d
  ../acme-t14       —    conflict         ^1         -43 (origin/main)  254 kB  21 d
  ../acme-t16       —    conflict         ^1         -41 (origin/main)  263 kB  21 d
  ../acme-t21       —    done (ancestor)  —          -37 (origin/main)  276 kB  21 d
  ../acme-t22       —    done (ancestor)  —          -37 (origin/main)  282 kB  21 d
  /tmp/scratch/h1   —    prunable (gitdir file points to non-existent location)
  /tmp/scratch/h2   —    prunable (gitdir file points to non-existent location)

  2 worktrees are done and hold nothing unique: 558 kB. behind is measured against
  origin/main, which last moved on 2026-08-23. nodal removed nothing.
```

**`BEHIND` is only as new as your last fetch.** Nodal never fetches to make it newer.
It reads how old the number is and says so when the number is old. A checkout you fetched
today gets no such line.

**`ONLY HERE` is the column that matters.** It answers the only question that stops
people deleting anything: *will this destroy work that exists nowhere else?* `^1` means
one commit is only in that folder. A dash means nothing is.

`DONE` is a separate question, read with `git merge-tree`, so a squash merge still counts
as done. `FOR` shows the purpose you gave a unit, or a purpose recovered from a supported
agent's session record. Today that means Claude Code; a worktree any other tool made
shows a dash until you name it.

## Try it on a repository you already have

```sh
cd ~/projects/anything
nodal
```

That is the whole first minute. It needs no `init`, no config file, and no permission. It
reads Git and the filesystem, prints the table above, and tells you it removed nothing.
`nodal doctor --machine` does the same across every clone under your home directory,
grouped by remote.

Cleanup is a trust problem before it is a disk problem, so nothing is ever removed for
you. Nodal says what exists, why it believes a thing is disposable, and what it touched.
Reclaiming is a separate command you choose, and it refuses outright while a folder holds
work that exists nowhere else.

## Install

Rust 1.88 or later.

```sh
cargo install --git https://github.com/josh2c/nodal nodal-cli
```

Then, for `nodal cd` to move the shell you are in:

```sh
echo 'eval "$(nodal shell-init bash)"' >> ~/.bashrc   # or zsh, or fish
```

Nodal makes no network calls of its own: no update check, no telemetry. The only network
traffic it causes is the `git` you configured talking to the remotes you configured.

## Two kinds of folder

Nodal reads the worktrees you already have, and it makes something different from them.
The first is how you arrive. The second is the product.

| | Where it lives | What it is |
| --- | --- | --- |
| **A worktree you already have** | where you made it | Git's own worktree, sharing Git's object database, exactly as it was. Nodal reads it, and `nodal adopt` registers it in place. The only files written are `.nodal/` and `.envrc`, both excluded from Git first, so `git status` there stays byte for byte what it was. |
| **A unit Nodal makes** | its own home | An independent, ready-to-run development environment: its own repository, its own ports, its own runtime state, and a memory. Not a worktree. |

## What `nodal new` makes

Git gave every task a branch. Worktrees gave every branch a folder. Nodal gives every
task a complete environment without giving every task a complete copy of the project.

```sh
nodal init          # write nodal.toml, one line per gap
nodal new "fix the worker import"
```

A unit is an independent, ready-to-run development environment with its own repository,
runtime state, and memory.

- **Dependencies, not a build.** A base installs the project's dependencies, so a unit
  starts with them in place. A base runs the project's build command only when `nodal base
  build --warm` made it. A unit of a compiled project builds before it runs. Ports and
  environment come from the folder, so any terminal, IDE or agent can open it with no
  plugin.
- **An environment, not a second install.** A unit's home is a copy-on-write clone of a
  warm base, so it costs the blocks that differ rather than another full environment.
  Measured on this repository, on btrfs, on 28 cores: `nodal new` returns a home in 0.29 s
  from a base of sources, and in 0.49 s from a 6.8 GB base that carries a build. The second
  home costs 48 KiB of its own at creation. `docs/benchmark.md` states the method, and says
  what a whole task costs, of which a create is a small part.
- **Its own repository, on purpose.** Nodal does not create worktrees. A unit's home is a
  full clone including `.git`, independent of every other unit, while the shared history
  costs nothing to duplicate. Worktrees existed to avoid expensive checkouts;
  copy-on-write makes checkouts cheap, so the trade is no longer worth making. You get
  logical independence and physical deduplication at the same time, and none of the
  shared-repository edge cases around `gc`, stash, refs and branch checkout.
- **A memory.** `WORKUNIT.md` is compiled, never edited, and rewritten on every `nodal
  show`. It says what the unit is for, what its siblings changed, and what ran. The next
  agent or engineer continues without a transcript.
- **Cleanup that sorts by kind.** Unique work is kept, reconstructable state is
  reclaimed, runtime is stopped, shared state is left alone.

### Starting a unit on work you have already begun

```sh
nodal new "fix the worker import" --carry
```

`--carry` starts the unit at your checkout's `HEAD` and copies what you had not committed
into it: what was staged is staged, what was unstaged is unstaged, what was untracked is
untracked. The unit starts dirty, in the shape you were already working in, and you commit
it there as you meant to.

It **copies**. Your checkout is read and left exactly as it was, index included, and
nothing is stashed, staged, committed or cleaned in it. Nodal writes no commit, no ref and
no network call to do it, so the unit's branch stands at the same commit yours does.

What an ignore rule covers stays where it is — that state belongs to the base, which
already has it. `--carry` refuses rather than guess: an index with unresolved merge stages,
a `HEAD` that is not a branch with a commit, `--from` naming a second starting point, an
uncommitted set over the ceiling, or a file it would have to overwrite in the new home.
Every refusal says which of those it was, and leaves both your checkout and the unit that
would have been made untouched.

A base is rebuilt only when the inputs that define an environment move: lockfiles,
toolchain, migrations, service definitions. An ordinary source commit moves nothing.

## Commands

Everyday:

| Command | What it does |
| --- | --- |
| `nodal` | the table above: every worktree and unit, what it is for, whether it is done |
| `nodal new` | a unit from a warm base: branch, home, ports, environment |
| `nodal new --carry` | the same, starting at your `HEAD` with your uncommitted work copied in |
| `nodal adopt` | register a worktree or checkout where it stands, without moving it |
| `nodal show` | one unit in full, and its memory rewritten |
| `nodal shell`, `cd`, `run` | work inside a unit, and record what ran |
| `nodal done`, `merge` | push and print the compare link, or squash, rebase and fast-forward |
| `nodal reclaim` | end a unit; it refuses while the home holds work that is only there |

When you need it: `nodal doctor` reports what every tool left behind on this machine and
removes nothing; `nodal explain` says why a home is as it is; `nodal ps` names the unit a
running process or bound port belongs to; `nodal base` lists and builds the warm bases;
`nodal env` reports what a home carries; `nodal gc` empties the trash after its retention;
`nodal uninstall` takes back the shell integration and the provider hooks, file by
file, and leaves every unit home a standalone Git repository. `--state` is what removes
the state directory and the unit homes in it, and it refuses while one holds work that
exists nowhere else.

Full surface and guarantees: [`docs/contracts.md`](docs/contracts.md).

## Agents

Nodal is agent-neutral. A unit is a folder and the environment comes from the folder, so
any tool that can open a directory can use one with no integration at all.

| Tool | Integration |
| --- | --- |
| Claude Code | hooks can answer a worktree request with a Nodal unit instead of a bare worktree; context loads at session start, and the unit outlives the session |
| Codex | an `AGENTS.md` pointer line; unit context in `WORKUNIT.md` |
| Cursor, VS Code, any IDE | open the unit folder; the integrated terminal is already activated |
| Plain terminal | `nodal shell-init`, or `cd` and `.envrc` |

## Platforms

| Platform | Backend |
| --- | --- |
| macOS (APFS) | `clonefile` per entry, filtered walk for excludes |
| Linux (btrfs, XFS, bcachefs) | `FICLONE` per file, into a directory of its own |
| Any other filesystem | a copy of the bytes, with a warning |
| Windows | via WSL2, as Linux |

## Status

Built, tested, and used daily on this project to build itself: the substrate, the list,
adoption, and the cleanup. Under construction: locks across users on one machine, a
database per unit, and `sync`, which reconciles a unit's environment after its
dependencies or migrations move. Safety properties are asserted as invariant tests in
[`tests/safety`](tests/safety) and run on every pull request.

## License

MIT. See [LICENSE](LICENSE).
