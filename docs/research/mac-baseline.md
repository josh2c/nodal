# What Nodal reads and cannot read on macOS

This file records each sentence where Nodal says that it cannot read something on a Mac.
A row is a sentence that prints today. When a sentence stops printing, its row goes, and
the section "What changed since the first reading" says why.

## The host

| Item | Value |
| --- | --- |
| Commit | `823797f` |
| macOS | 26.6.1 (25G76) |
| Chip | Apple M1 Pro, arm64 |
| rustc | 1.98.1 (48a229cea 2026-09-01) |
| Registry | migration 14 |
| Docker | installed, daemon not running |
| Date | 2026-09-21 |

## The workspace tests

The command was `nodal run -- cargo test --workspace --locked --no-fail-fast`, with an
empty `NODAL_HOME`. The flag is kept so that one failure does not hide the binaries
after it.

| Crate | Test binaries | Passed | Failed | Ignored |
| --- | --- | --- | --- | --- |
| `nodal-cli` | 24 | 251 | 0 | 0 |
| `nodal-core` | 27 | 985 | 0 | 2 |
| `nodal-fixture` | 2 | 7 | 0 | 0 |
| `nodal-safety` | 35 | 198 | 0 | 0 |
| `nodal-startup-bench` | 1 | 12 | 0 | 0 |
| Doc-tests | 3 | 0 | 0 | 0 |
| **Total** | **92** | **1453** | **0** | **2** |

Every binary passed. The two ignored tests are in `nodal-core --test lifecycle` and
`nodal-core --test substrate`.

The four extended-attribute failures this file recorded at `bb0d4ae` are gone. Each
counted every attribute on a file, and macOS 26 adds `com.apple.provenance` when a
process writes one. The tests now count against the source file instead of a number.

## How the readings were made

- The sample is a clone of a real project on this Mac, made with
  `git clone --no-hardlinks`. It is a Node monorepo with apps and packages.
- `NODAL_HOME` was a new temporary directory. No reading used the registry in `~/.nodal`.
- The readings ran in this order: `nodal init`, `nodal new "baseline reading" --name
  probe`, `nodal ls`, `nodal show probe`, `nodal ps`, `nodal reclaim probe --check`.
- For the live shell, a second pseudo-terminal ran `/bin/zsh -i`, with its working
  directory in the unit home. The pid confirmed the shell. With that shell open, `nodal
  ls`, `nodal show probe`, `nodal ps` and `nodal reclaim probe --check` ran again from the
  first terminal. Then the shell stopped.
- For the listener, `python3 -m http.server` bound the unit's granted `app` port from
  inside the home. `nodal ps` and `nodal reclaim probe --check` ran again. Then the
  listener stopped.
- Last, `nodal reclaim probe` ran, and then `nodal doctor --machine`.
- The shell and the readings ran from a coding agent. For this reason the unit hold names
  the actor `<agent>`. A person at a terminal gets a different actor name. The
  readings do not change.
- No reading changed a tracked file in the sample.

In the pasted output, a home path is `~`, a temporary directory is `$NODAL_HOME` or
`<sample>`, a clone or remote name is `<clone>`, the agent name is `<agent>`, and the host
name is `<host>`.

## The readings

"No session" is the state before the second terminal opened. "Live shell" is the state
while `/bin/zsh` stood in the unit home. A row with both states printed the same sentence
in each state.

| Command | State | The sentence, verbatim | The reason the sentence gives | Module that prints it |
| --- | --- | --- | --- | --- |
| `nodal ls` | no session, live shell | `who: macos does not show the variables or the directory of a process of another account` | The kernel refuses both readings for a process of another account | `nodal-core/src/output/view/unit.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` (`ANOTHER_ACCOUNT`, given by `withheld`), raised in `nodal-core/src/runtime/processes.rs` (`macos`) |
| `nodal ls` | no session, live shell | `who: macos does not show the variables of a process that runs a restricted binary` | The kernel zeroes the variables of a restricted binary | `nodal-core/src/output/view/unit.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` (`RESTRICTED`, given by `withheld`) |
| `nodal show probe` | no session, live shell | `disk       875 MB apparent · a home shares blocks with the base it was copied from, and no portable call says how many of these bytes are its own` | No portable call gives the bytes that belong only to the home | `nodal-core/src/doctor/size.rs` (`SHARED`) |
| `nodal new` | no session | ``not ready  build: a `npm` build names no output directory this can check`` | The build names no output directory | `nodal-core/src/substrate/warmth.rs` |
| `nodal ps` | no session, live shell | `env, cwd: macos does not show the variables or the directory of a process of another account` | The kernel refuses both readings for a process of another account | `nodal-core/src/output/view/ps.rs` (`note_lines`, collapsed by `nodal-core/src/output/notice.rs`), text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal ps` | no session, live shell | `env: macos does not show the variables of a process that runs a restricted binary` | The kernel zeroes the variables of a restricted binary | `nodal-core/src/output/view/ps.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal ps` | no session, live shell | `docker: Cannot connect to the Docker daemon at unix://~/.docker/run/docker.sock. Is the docker daemon running?` | The Docker daemon does not answer | Docker's own message, passed on by `nodal-core/src/services/docker.rs` |
| `nodal reclaim probe --check` | no session, live shell | `runtime  would stop: 0 recorded groups · 0 processes by id · containers could not be read` | The container signal did not answer | `nodal-core/src/output/view/check.rs` (`counted`) |
| `nodal reclaim probe --check` | no session, live shell | `env, cwd: macos does not show the variables or the directory of a process of another account` | The kernel refuses both readings for a process of another account | `nodal-core/src/output/view/check.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal reclaim probe --check` | no session, live shell | `env: macos does not show the variables of a process that runs a restricted binary` | The kernel zeroes the variables of a restricted binary | `nodal-core/src/output/view/check.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal reclaim probe --check` | no session, live shell | `docker: Cannot connect to the Docker daemon at unix://~/.docker/run/docker.sock. Is the docker daemon running?` | The Docker daemon does not answer | `nodal-core/src/output/view/check.rs`, message from Docker |
| `nodal reclaim probe` | after the shell stopped | `verify  nothing found by id; a signal could not be read` | A signal could not be read. Here it is the container signal | `nodal-core/src/output/view/reclaim.rs` (`verify_cell`) |
| `nodal reclaim probe` | after the shell stopped | `env, cwd: macos does not show the variables or the directory of a process of another account` | The kernel refuses both readings for a process of another account | `nodal-core/src/output/view/reclaim.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal reclaim probe` | after the shell stopped | `env: macos does not show the variables of a process that runs a restricted binary` | The kernel zeroes the variables of a restricted binary | `nodal-core/src/output/view/reclaim.rs`, text from `nodal-core/src/runtime/attribute/mod.rs` |
| `nodal reclaim probe` | after the shell stopped | `docker: Cannot connect to the Docker daemon at unix://~/.docker/run/docker.sock. Is the docker daemon running?` | The Docker daemon does not answer | `nodal-core/src/output/view/reclaim.rs`, message from Docker |
| `nodal doctor --machine` | no session | `<clone>  no clone of this remote here heard from it more recently, so this clone's own remote-tracking refs could not be checked` | No other clone of the remote has newer refs | `nodal-core/src/doctor/unique.rs` (`UNWITNESSED`) |
| `nodal doctor --machine` | no session | `48 clones not checked; a clone this run could not read is not known to be safe` | This run could not read those clones | `nodal-core/src/output/view/machine.rs` |
| `nodal doctor --machine` | no session | `26 clones had no fresher clone of their remote here; another copy in this checkout or the clones beside it holds their commits, and whether a remote does was not checked` | No clone has newer refs for their remote | `nodal-core/src/output/view/machine.rs` |
| `nodal doctor --machine` | no session | `~/.vscode/extensions/<extension>: ~/.vscode/extensions/<extension> is not a Git repository` | Git does not accept the directory as a repository | `nodal-core/src/doctor/inspect.rs`, text from `nodal-core/src/error.rs` (`NotARepository`) |

## What the live shell changed

The shell is a reading. `/bin/zsh` stood in the unit home, and the four commands answered
differently from the way they answered at `bb0d4ae`.

- `nodal ps` gave the row `probe  process  zsh  <pid>  —  probable  cwd`. The shell runs a
  restricted binary, so the scan found it by its working directory and read no variables.
- `nodal reclaim probe --check` said `refuse — a reclaim would stop and change nothing`,
  `because  blocked: zsh (pid <pid>)`, and `standing in the home, never signalled: zsh
  (pid <pid>); a reclaim would refuse to move the home`.
- `nodal ls` moved NEEDS from `review` to `blocked`.
- `nodal show probe` read the hold. A hold whose process has ended says `pid <pid> is not
  on this host any more; the hold stands until it lapses`. This line reports the hold, not
  the shell.

The listener is a reading too. With `python3 -m http.server` bound on the granted `app`
port from inside the home, `nodal ps` gave the row `probe  listener  app  —  20000
probable  listener`, and printed no listener note.

The temporary registry held no session row, no lease and no event for the shell. It held
one unit lock for this host.

## How the same commands differ on Linux, from the code

The code in `823797f` gives these differences. They were not run on a Linux host for this
file. What remains different is what the kernel refuses.

- **Process scan.** Both hosts read the table. On Linux, `runtime/processes.rs` reads
  `/proc/<pid>`. On macOS it reads `proc_listallpids`, `proc_pidinfo` and `sysctl` with
  `KERN_PROCARGS2`. The env and cwd signals in `ps`, `ls`, `show` and `reclaim` use this
  scan on both hosts.
- **What the kernel refuses.** macOS refuses the variables and the directory of a process
  of another account, and the variables of a process that runs a restricted binary. Each
  refusal is a `part` note with its reason. A Linux scan leaves out a process this account
  cannot read, and gives no note.
- **Hold liveness.** Both hosts read the process of the hold. `show` prints a live or a
  gone hold, and `ls` prints `gone,` and the time left for a gone hold.
- **A shell in the home.** Both hosts find the shell by its working directory. `ps`
  attributes it to the unit, `reclaim --check` lists it under `standing in the home, never
  signalled`, and the verdict is `refuse`.
- **Listeners.** On Linux, `services/listeners.rs` reads `/proc/net/tcp`. On macOS it
  reads `net.inet.tcp.pcblist_n`, which gives IPv4 and IPv6 together. Both readings are
  host-wide: a socket held by another account is in them. `ps` names the bound port under
  its unit on both hosts.
- **Reclaim verify.** With every signal answering, `reclaim` prints `nothing left by id`
  on both hosts. This Mac has no Docker daemon, so it printed `nothing found by id; a
  signal could not be read`.
- **Block sharing.** `workspace/apfs.rs` uses `clonefile` on APFS, and
  `workspace/reflink.rs` uses reflink on Linux. The `disk` sentence is the same on both
  hosts.
- **Extended attributes.** `workspace/xattr.rs` has a macOS branch for `listxattr`,
  `getxattr` and `setxattr`. macOS adds `com.apple.provenance` to a file a process writes.
- **The same on both hosts.** The Docker message, the `doctor --machine` sentences, the
  build readiness sentence and the `disk` sentence.

## What changed since the first reading

Three changes moved these rows between `bb0d4ae` and `823797f`.

- The change that refuses to move a home while the process table is unread. Before it, a
  reclaim moved a home when the scan failed, and `--check` called that home safe. It also
  gave the views the words "could not be read" in place of a count.
- The change that reads the process table on macOS. It removed the five sentences that
  said a process scan reads `/proc`. In their place it put two `part` notes, one for each
  refusal the kernel makes. `ps` now names a shell that stands in a home, `ls` says
  `blocked`, `show` reads the hold, and `reclaim --check` refuses.
- The change that reads which granted ports are bound on macOS. It removed the sentence
  that said a listener scan reads `/proc/net/tcp`. `ps` now names the bound port under its
  unit, as it does on Linux.

Other rows went for reasons of their own. The walk of
`~/Library/Containers/com.apple.Siri/Data/Library` printed no "could not be read" line in
this run; `doctor/scan.rs` still holds that sentence for a directory it cannot open. The
rows that reported a count of 0 processes, or a `safe` verdict over an unread table, print
an honest reading now and say nothing about what Nodal cannot see.
