# What Nodal reads and cannot read on macOS

This file records each sentence where Nodal says that it cannot read something on a Mac.
The work that adds a process scan on macOS compares its result against this table. When a
row in the table changes, that work must say why. A row that is struck through is a
sentence Nodal no longer says, and the cell that follows says what it says instead.

## The host

| Item | Value |
| --- | --- |
| Commit | `bb0d4ae` |
| macOS | 26.6.1 (25G76) |
| Chip | Apple M1 Pro, arm64 |
| rustc | 1.98.1 (48a229cea 2026-09-01) |
| Registry | migration 14 |
| Docker | installed, daemon not running |
| Date | 2026-09-16 |

## The workspace tests

The command was `nodal run -- cargo test --workspace --locked --no-fail-fast`, with an
empty `NODAL_HOME`. Without `--no-fail-fast`, cargo stopped after `nodal-core` (lib), and
the binaries after it did not run.

| Crate | Test binaries | Passed | Failed | Ignored |
| --- | --- | --- | --- | --- |
| `nodal-cli` | 24 | 242 | 0 | 0 |
| `nodal-core` | 27 | 936 | 4 | 2 |
| `nodal-fixture` | 2 | 7 | 0 | 0 |
| `nodal-safety` | 34 | 189 | 0 | 0 |
| `nodal-startup-bench` | 1 | 12 | 0 | 0 |
| Doc-tests | 3 | 0 | 0 | 0 |
| **Total** | **91** | **1386** | **4** | **2** |

Two binaries failed: `nodal-core` (lib), with 662 passed and 2 failed, and
`nodal-core --test materialize`, with 10 passed and 2 failed. The two ignored tests are in
`nodal-core --test lifecycle` and `nodal-core --test substrate`.

The four failures have one cause:

- `workspace::xattr::tests::an_attribute_survives_a_copy` (lib) expects 1 attribute and
  gets 2.
- `workspace::xattr::tests::a_file_without_attributes_copies_none` (lib) expects 0 and
  gets 1.
- `an_extended_attribute_survives_the_clone` (`materialize`) expects 1 attribute and
  gets 2.
- `a_read_only_file_that_carries_an_attribute_is_cloned` (`materialize`) expects 1
  attribute and gets 2.

On macOS 26, a file gets the `com.apple.provenance` attribute when a process writes it.
`xattr -l` on a new file in a temporary directory shows this attribute. Each count is one
more than the test expects, and that one attribute is the one macOS added. The tests count
every attribute on the file and do not know about this one.

## How the readings were made

- The sample is a clone of a real project on this Mac, made with
  `git clone --no-hardlinks`. It is a Node monorepo with apps and packages.
- `NODAL_HOME` was a new temporary directory. No reading used the registry in `~/.nodal`.
- The readings ran in this order: `nodal init`, `nodal new "baseline reading" --name
  probe`, `nodal ls`, `nodal show probe`, `nodal ps`, `nodal reclaim probe --check`,
  `nodal doctor --machine`.
- For the live shell, a second pseudo-terminal ran `nodal shell "$(nodal cd probe)"`. The
  shell was `/bin/zsh`, with its working directory in the unit home. With that shell
  open, `nodal ps`, `nodal show probe`, `nodal reclaim probe --check` and `nodal ls` ran
  again from the first terminal. Then the shell stopped, and `nodal reclaim probe` ran.
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
| `nodal ls` | no session, live shell | `who: a process scan reads /proc, which macos does not have` | No `/proc` on this host | `nodal-core/src/runtime/ls.rs` (`who: {error}`), text from `nodal-core/src/error.rs` (`ProcessScanUnsupported`), raised in `nodal-core/src/runtime/processes.rs` |
| `nodal ls` | no session, live shell | `<agent> holds 7 h` (WHO column) | None. The column shows the hold and not whether its process is there. | `nodal-core/src/output/view/unit.rs` (`holds_cell`) |
| `nodal show probe` | no session, live shell | `hold       liveness not read: this host has no process table to read` | No process table on this host | `nodal-core/src/output/view/unit.rs` (`Unknowable::NoProcessTable`), chosen in `nodal-core/src/runtime/lock.rs` (`liveness`) |
| `nodal show probe` | no session, live shell | `running    —` | None. The line shows no process, and it does not say that it could not look. | `nodal-core/src/output/view/unit.rs` |
| `nodal show probe` | no session, live shell | `disk       875 MB apparent · a home shares blocks with the base it was copied from, and no portable call says how many of these bytes are its own` | No portable call gives the bytes that belong only to the home | `nodal-core/src/doctor/size.rs` (`SHARED`) |
| `nodal new` | no session | ``not ready  build: a `npm` build names no output directory this can check`` | The build names no output directory | `nodal-core/src/substrate/warmth.rs` |
| `nodal ps` | no session, live shell | `<host>: nothing attributed to a unit` | None on this line. The next lines give it. | `nodal-core/src/output/view/ps.rs` |
| `nodal ps` | no session, live shell | `env, cwd: a process scan reads /proc, which macos does not have` | No `/proc` on this host | `nodal-core/src/output/view/ps.rs` (notes joined by `nodal-core/src/output/notice.rs`), text from `nodal-core/src/error.rs` |
| `nodal ps` | no session, live shell | `docker: Cannot connect to the Docker daemon at unix://~/.docker/run/docker.sock. Is the docker daemon running?` | The Docker daemon does not answer | Docker's own message, passed on by `nodal-core/src/services/docker.rs` |
| `nodal ps` | no session, live shell | ~~`listener: a listener scan reads /proc/net/tcp, which macos does not have`~~ Gone. `ps` now names the bound port under its unit, as it does on Linux: `macos-listeners  listener  app  —  20101  probable  listener` | The scan reads `net.inet.tcp.pcblist_n`, which macOS publishes host-wide to an ordinary account | `nodal-core/src/services/listeners.rs` |
| `nodal reclaim probe --check` | no session, live shell | `verdict  safe — a reclaim would go ahead` | None | `nodal-core/src/output/view/check.rs` (`verdict_cell`), from `nodal-core/src/lifecycle/assess.rs` (`safe_to_reclaim`) |
| `nodal reclaim probe --check` | no session, live shell | `because  nothing that is only here, and nothing standing in the home` | None | `nodal-core/src/output/view/check.rs` (`because_cell`) |
| `nodal reclaim probe --check` | no session, live shell | `runtime  would stop: 0 recorded groups · env could not be read · docker could not be read` | The env signal and the Docker signal did not answer | `nodal-core/src/output/view/check.rs` (`counted`) |
| `nodal reclaim probe --check` | no session, live shell | `nothing was found standing in the home, and the process table could not be read` | The process table could not be read | `nodal-core/src/output/view/check.rs` (`runtime_cell`) |
| `nodal reclaim probe --check` | no session, live shell | `env: a process scan reads /proc, which macos does not have` | No `/proc` on this host | `nodal-core/src/output/view/check.rs`, text from `nodal-core/src/error.rs` |
| `nodal reclaim probe --check` | no session, live shell | `docker: Cannot connect to the Docker daemon at unix://~/.docker/run/docker.sock. Is the docker daemon running?` | The Docker daemon does not answer | `nodal-core/src/output/view/check.rs`, message from Docker |
| `nodal reclaim probe` | after the shell stopped | `verify  nothing found by id; a signal could not be read` | A signal could not be read | `nodal-core/src/output/view/reclaim.rs` (`verify_cell`) |
| `nodal reclaim probe` | after the shell stopped | `stop    0 tethers · 0 processes · 0 containers removed · 3 ports` | None. The count of 0 processes does not say that the scan could not run. | `nodal-core/src/output/view/reclaim.rs` |
| `nodal reclaim probe` | after the shell stopped | `env: a process scan reads /proc, which macos does not have` | No `/proc` on this host | `nodal-core/src/output/view/reclaim.rs`, text from `nodal-core/src/error.rs` |
| `nodal doctor --machine` | no session | `<clone>  no clone of this remote here heard from it more recently, so this clone's own remote-tracking refs could not be checked` | No other clone of the remote has newer refs | `nodal-core/src/doctor/unique.rs` (`UNWITNESSED`) |
| `nodal doctor --machine` | no session | `41 clones not checked; a clone this run could not read is not known to be safe` | This run could not read those clones | `nodal-core/src/output/view/machine.rs` |
| `nodal doctor --machine` | no session | `25 clones had no fresher clone of their remote here; another copy in this checkout or the clones beside it holds their commits, and whether a remote does was not checked` | No clone has newer refs for their remote | `nodal-core/src/output/view/machine.rs` |
| `nodal doctor --machine` | no session | `~/Library/Containers/com.apple.Siri/Data/Library: could not be read` | The walk could not open the directory | `nodal-core/src/doctor/scan.rs` |
| `nodal doctor --machine` | no session | `~/.vscode/extensions/<extension>: ~/.vscode/extensions/<extension> is not a Git repository` | Git does not accept the directory as a repository | `nodal-core/src/doctor/machine.rs`, text from `nodal-core/src/error.rs` |

## What the live shell changed

Nothing. The four commands printed the same sentences with and without the shell. Only
the `age` and `last` fields changed. The temporary registry held no session row, no lease
and no event for the shell. It held one unit lock for this host.

This is the case that the macOS process scan must turn into a reading. A live `/bin/zsh`
stood in the home. `nodal reclaim probe --check` said `safe — a reclaim would go ahead`
and `nothing standing in the home`.

## How the same commands differ on Linux, from the code

The code in `bb0d4ae` gives these differences. They were not run on a Linux host for this
file.

- **Process scan.** On Linux, `runtime/processes.rs` reads `/proc/<pid>` for the
  `NODAL_*` variables, the working directory and a short command. On any other host it
  returns `ProcessScanUnsupported`. The env and cwd signals in `ps`, `ls`, `show` and
  `reclaim` all use this scan.
- **Hold liveness.** On Linux, `runtime/lock.rs` reads the process of the hold. `show`
  then prints a live or gone hold, and `ls` prints `gone,` and the time left for a gone hold. On
  macOS the hold is `Unknown { NoProcessTable }`, and `ls` always prints `holds`.
- **A shell in the home.** On Linux, the scan finds the shell by its working directory.
  `ps` attributes it to the unit. `reclaim --check` lists it under `standing in the home,
  never signalled` and says `a reclaim would refuse to move the home`. On macOS the
  verdict is `safe`.
- **Listeners.** On Linux, `services/listeners.rs` reads `/proc/net/tcp` to find which
  allocated port is bound. On macOS it reads `net.inet.tcp.pcblist_n`, which gives the
  same answer for IPv4 and IPv6 together. Both readings are host-wide: a socket held by
  another account is in them. This row changed after this baseline was taken.
- **Reclaim verify.** On Linux, when every signal answers, `reclaim` prints `nothing left
  by id`. On macOS it prints `nothing found by id; a signal could not be read`, even with
  Docker running.
- **Block sharing.** `workspace/apfs.rs` uses `clonefile` on APFS, and
  `workspace/reflink.rs` uses reflink on Linux. The `disk` sentence is the same on both
  hosts.
- **Extended attributes.** `workspace/xattr.rs` has a macOS branch for `listxattr`,
  `getxattr` and `setxattr`. The four test failures above occur only on macOS, because
  macOS adds `com.apple.provenance`.
- **The same on both hosts.** The Docker message, the `doctor --machine` sentences, the
  build readiness sentence and the `disk` sentence.
