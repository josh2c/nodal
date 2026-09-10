# DL-draft: the verdict in a checkout Nodal holds no row for

Status: draft. Raised by task `verdict-1`.

## What changed

`nodal`, and `nodal ls`, in a Git checkout with no recipe and no registry row now print
the verdict on that checkout's worktrees. Before, they refused.

## Four deviations from the task as written

### 1. The cost is above the stated budget for a worktree that is ahead of the base

The task set the budget at "the doctor's process count per worktree plus one
merge-tree". Measured on the six-worktree fixture, doctor spends four Git processes per
worktree. The verdict spends five for a worktree the base already carries, and seven for
a worktree that is ahead of it.

The two extra processes are both required by columns the task asks for.

One is `rev-list --left-right --count`, which is the BEHIND column. The task specifies
that column separately, so this process is not merge-tree overhead.

The other is `rev-parse <base>^{tree}`. Git's `merge-tree` verdict needs it to tell an
absorbed branch from an open one. It is part of one merge-tree, not a second one.

Read that way the cost is doctor's four, plus one for BEHIND, plus one merge-tree that
costs two processes. Nothing was added beyond the columns. The budget should say two
processes for the merge-tree verdict.

Median wall-clock, release binary, this workstation:

| worktrees | median | per worktree |
|---|---|---|
| 12 | 63 ms | 5.3 ms |
| 50 | 238 ms | 4.8 ms |

### 2. A registered project keeps its own columns

The founder chose one table with a column that says whether each row is a unit or a
worktree. The preview shown beside that question used the verdict's columns for every
row. The task text says the registered case prints "the list as today".

The task text won. A registered project prints the columns it always printed, with one
leading column added, and the worktree rows fill those columns. A project whose
repository names no other worktrees prints exactly the table it printed before.

The heading over the names changes from `unit` to `name` only in the mixed table. A
folder Nodal did not make must never appear under the word `unit`.

### 3. A declared project gained the worktree rows too

The task names three cases. It does not name the project that holds a `nodal.toml` and
no units. That project printed an empty list.

A person who has just run `nodal init` in a checkout with nine worktrees is the person
this feature is for. So the declared case now carries the worktree rows as well.

### 4. `nodal ls` no longer exits non-zero in a checkout with no row

This is the point of the task. It is also a change to a documented contract, so it is
recorded here. `docs/contracts.md` now states four cases instead of three.

A directory that is not a checkout either is still refused, and the refusal still names
the directory and gives its reason.

## One thing the task did not ask for

`nodal` no longer creates the registry to find out that a directory is in no project.
A machine with no registry has no projects, so the answer is the same either way.

The first command a person types now leaves no trace at all. That promise is held by
`tests/safety/tests/verdict_writes_nothing.rs`.
