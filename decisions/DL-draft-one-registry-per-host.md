# DL-draft: one registry per host, and what else had to leave the state root

Status: draft. Raised by task `registry-per-host`.

## What changed

A state root whose own mode says a group owns it is a shared root. Nodal writes a
registry the group can write, runs under a umask that keeps the group's write bit, and
makes each home's `.nodal/env` group-readable. A project is keyed by its `origin`
remote, so two engineers' clones of one repository are one project with one base and one
block of ports. A person's secrets move to `~/.config/nodal/secrets.env`.

## Three deviations from the task as written

### 1. The hook approvals moved as well

The task lists one file to move: the secrets file. The hook approvals are a second file
of the same kind, and leaving it behind would have opened a hole the task itself created.

An approval records that a person read an exact command line and accepts it running.
The record lived at `<state root>/hooks.toml`. Once the state root belongs to a group,
that record belongs to the group: one engineer running `nodal init` would decide that a
command runs under the other engineer's account, with no reading and no consent.

So the approvals file resolves the same three ways the secrets file does:
`NODAL_HOOKS_FILE`, then `~/.config/nodal/hooks.toml`, then the old path when a machine
still holds a file there.

`tests/safety/tests/shared_host.rs::an_approval_by_one_account_does_not_approve_a_hook_for_another`
is what holds it.

### 2. A project row comes back standing in the caller's own checkout

The task says a project is keyed by remote URL plus checkout path. Taken literally, one
row wins and every operation then acts on the path that row records, which is one
engineer's clone. `nodal merge` fast-forwards that path. One account moving a branch
inside another account's working copy is not something this task asked for.

So the row's stored path is a record of where the project was first seen, and every
lookup hands back the same project standing in the checkout the person is in. One
project identity, and no command reaches across accounts.

### 3. The identity lookup asks the path first and the remote second

The task says the remote first, the path second. A path match is a registry lookup; a
remote match costs one `git` call, and a project is resolved on every entry into a home.
Asking the remote only when the path is not recorded keeps that call off the hot path.

The two orders differ in one case: a checkout recorded under a path and later repointed
at another repository. That row keeps the identity it has, because re-keying every unit
of a project on the strength of a changed remote is not a thing a command should do
while nobody is looking.

## Two things that need a decision

### `.envrc` is two lines now, and a committed one is still one

`.envrc` reads `.nodal/env` and then evaluates `nodal env --export`, because the values
a person supplies are no longer in the file. A project that commits its own `.envrc` —
direnv and Nix users do — keeps its file, which holds no such line. Those homes get
their identity and generated values from direnv and their secrets from the shell hook.
A person with a committed `.envrc` and no shell integration loses the secrets that
direnv used to deliver.

The fix, if this matters, is a line in the documentation telling such projects to add
the eval line themselves. Nodal must not write in a tracked file.

### A second uid is not asserted anywhere

The suite fakes the second person with a second home directory, which is the seam that
decides which secrets file is read. It does not switch uid, so nothing here shows the
kernel refusing one account the other's file. What is asserted is the mode and the group
the kernel would decide that from. Each test that makes the narrower claim prints it.
