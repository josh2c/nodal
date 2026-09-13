# Nodal measured against the clones this project was built with

This machine holds both halves of an experiment nobody designed. `~/Projects` holds 34
clones of this repository, made by hand before and while Nodal was written. The registry
under `~/.nodal` holds every unit made since. This report reads both, with Nodal first and
`git` only where Nodal could not answer.

Every number here comes from the release binary built from `1b8b472`, on this machine, on
btrfs with `compress=zstd:3`, on 28 cores. Timed figures are the median of five runs unless
the table says otherwise. Cold and warm are stated separately. Where a measurement
disagrees with a document, the measurement wins and the document is named.

## Implementation table

| Part | Percent | LOE | Certainty |
| --- | --- | --- | --- |
| Where the 34 came from, and whether Nodal was there | 100% | M | 0.95 |
| What the old way cost, and what the new way costs | 100% | L | 0.95 |
| The defect that separates the two | diagnosed and repaired | M | 0.99 |
| What the registry says about a week of real use | 100% | M | 0.80 |
| Ready, or only installed | 100% | L | 1.00 |
| What should happen to the 34 | 100% | M | 0.98 |
| README corrected where measurement contradicts it | 100% | S | 1.00 |
| The project's own recipe filled in | 100% | S | 1.00 |

Certainty is lowest on the registry reading. The registry records unit lifecycle writes and
every command run through `nodal run`. It records nothing when a person types `nodal ls`,
`show`, `explain` or `doctor`. Adoption of the read commands is not measurable, and this
report does not guess at it.

## The four answers, before the evidence

**Was Nodal used?** Yes, from the first minute it could be. The last `git clone` and the
first `nodal new` are three minutes and sixteen seconds apart, and no clone was ever made
again.

**Should it have been used sooner?** Yes. `nodal new` worked on `main` eleven hours before
the first unit. Eighteen of the 34 clones, holding 53.39 GB of the 53.41 GB, were made in
that window. What was missing was not a feature. It was a `nodal.toml`, which this change
adds.

**Is it doing what we believe it does?** Half. It creates and reclaims exactly as claimed,
and it returned 218.67 GB in four days. It did not deliver a warm build, because a base
built with `--warm` was stale before it was copied. That single defect is the difference
between 45.9 s and 124.3 s to a green test run, and between 104 KiB and 6.33 GiB per unit.
Section 5 says what the defect was and what removed it.

**Ready, or only installed?** Only installed, today. The prior finding is confirmed and
sharpened. A unit arrives in 0.3 s to 0.9 s and cannot run one test until it builds. With
the defect in section 5 fixed, the same unit runs the whole suite in 45.9 s and compiles
nothing, which is the first measurement on this machine that makes the word "ready" true.

## Questions for the maintainers

1. **Is a base allowed to fail its own build?** Answered by measurement rather than by
   policy. The base was not failing its build; it was doing the build at one path and
   handing it over at another. Every step now runs at the delivered path, and the base
   compiles nothing when the same command is run again.
2. **Should `nodal new` warm the base it builds?** `WARM_BUILD` is hard-coded `false`. With
   question 1 answered, warming costs 119 s once and saves 74 s on every later unit. The cost
   lands on the first `nodal new` a person ever runs.
3. **Should `doctor` say how old its knowledge of the remote is?** It answered "nothing
   unique" for ten clones whose branches the remote no longer has. The answer was correct,
   and not for the reason `doctor` gave.
4. **Should a base clone the local checkout when one exists?** A cold base build needs the
   network today, on a machine that already holds a full checkout. That is deliberate,
   so this is a request to revisit, not a bug report.
5. **`docs/contracts.md:203` lists seven commands the binary does not have, and `nodal merge`
   and `nodal adopt` have never been used on real work.** Cut, or build?

## 1. Where the 34 came from, and whether Nodal was there

All 34 are independent clones, not worktrees. Each holds its own `.git`. Each was made by
`git clone` from `https://github.com/josh2c/nodal`, which the first line of each reflog
records. None was copied from another.

The window is narrow. The first clone is `nodal-t0.6`, made 2026-09-06 18:52:58 local. The
last is `nodal-t1.0c`, made 2026-09-07 23:22:46 local. Every clone falls in those 28.5
hours. None was made after.

That window opens 59 minutes after this project's first commit, `4993bd0`, at 2026-09-06
17:53. The early clones could not have been units, because Nodal did not exist. They are
where Nodal was written.

### The changeover is three minutes wide

| Event | Time |
| --- | --- |
| Last `git clone` (`nodal-t1.0c`) | 2026-09-08 06:22:48 UTC |
| First `nodal new` (`dogfood-first`) | 2026-09-08 06:26:04 UTC |
| Commit `fcea62f`, "The project's own recipe: nodal manages nodal" | 2026-09-08 06:27:09 UTC |

Three minutes and sixteen seconds separate the last clone from the first unit. Nothing
overlaps. After `dogfood-first` no clone was ever made again, and 46 units were.

The blocker was not `nodal new`. It was the project's own `nodal.toml`. The recipe commit
lands 65 seconds after the first unit. The tool became usable on this project at the moment
the project described itself to the tool.

### But the tool was ready eleven hours earlier

`nodal new` reached `main` in pull request #6, merged 2026-09-07 11:56. The shell handover
landed in #7 at 12:17. Warm bases landed in #8 at 12:27. From 12:27 on 2026-09-07, the
binary on `main` could create a unit.

**Eighteen of the 34 clones were made after that point**, and the earliest of them at 12:36,
nine minutes after warm bases landed. They hold 53.39 GB of the 53.41 GB the 34 hold in
total. Every one of the five large clones is in that group.

So the answer to the pointed version of the question is split, and the second half is the
uncomfortable one:

- For 16 clones, Nodal could not have been used. It did not exist or could not yet create
  a unit. The clones were necessary.
- For 18 clones, over roughly 11 hours, Nodal could create a unit and nobody asked it to.
  What was missing was a 50-line `nodal.toml`, which `nodal init` writes.

This rejects the first thing the brief wondered. The clones do not exist only because Nodal
was not ready. For the 18 that hold 99.9% of the bytes, Nodal was ready and was not
reached for. The gap was not capability. It was that nobody had run `nodal init` on the project
Nodal was being written in.

## 2. What a week of real use says

The registry holds 47 units across two projects, 1248 events, 106 recoverable operations
and 46 trash rows. Pull requests 34 to 51 were written inside units.

### What was used

| Command | Evidence | Times |
| --- | --- | --- |
| `new` | `operation` rows | 52, of which 5 rolled back |
| `reclaim` | `operation` rows | 46, all committed |
| `base build` | `operation` rows | 8, of which 2 rolled back |
| `run` | `event` rows of kind `command` | 1180 commands, in 20 of 47 units |
| `done` | `event` rows of kind `sync` | 18, in 17 of 47 units |
| `merge` | no unit ever reached status `merged` | 0 |
| `adopt` | every one of 47 environments is `managed` | 0 |

`new` and `reclaim` were adopted at once and used for everything. The rest of the lifecycle
was not. `nodal run` reached 43% of units. `nodal done` reached 36%, and its first use is
2026-09-09 21:57, two and a half days after the first unit. For those two and a half days
every unit was pushed by hand.

### Where engineers worked around the tool

The 1180 recorded commands are all the commands run through `nodal run`. Their shape says
what the units were used for:

| Program | Invocations |
| --- | --- |
| `cargo` | 548 |
| `git` | 176 |
| `gh` | 155 |

Inside that, `git push` appears 41 times and `gh pr create` 9 times. `nodal done` pushes a
branch and prints the compare link, and `nodal merge` squashes, rebases and fast-forwards.
Both exist. Engineers used `nodal run -- git push` and `gh pr create` instead, and merged on
GitHub. `gh pr checks` appears 108 times, which is a person watching CI by hand in a tool
that has no view of it.

The honest summary is that Nodal was adopted as an environment manager and not as a
workflow. It made and destroyed homes for a week without complaint. The commands that carry
work from a unit back to `main` went mostly unused.

### What was recovered, and what was lost

`reclaim` ran 46 times and pruned **218.67 GB**. The mean across all 46 is 4.75 GB, the
mean across the 36 units of this project is 6.07 GB, and the largest, `parallel-walk`, gave
back 20.29 GB. No trash row holds a snapshot, and no unit was
reported as holding unique work.

That number is the strongest one in this report. Over four days the unit model created and
reclaimed four times the byte volume that the clone era left sitting on disk, and left
nothing behind for a person to judge.

### The other project on this machine

The other project is a pnpm monorepo with Supabase. It has 10 units in the same
registry. The contrast is sharp in two ways.

| | `nodal` | the pnpm project |
| --- | --- | --- |
| Recipe `[commands]` | empty until this change | all eight filled |
| Recipe `[db]`, `[env]`, ports | empty | filled, with six fixed ports |
| Reclaims | 36 | 10 |
| Bytes pruned at reclaim | 218.67 GB | 0.0 GB |

The recipe line is the uncomfortable one. Of the two projects on this machine that use
Nodal, the one that describes itself to Nodal properly is not Nodal. `nodal.toml` in this
repository named a package manager and a toolchain and nothing else. No build command, no
test command. That is why no base was ever warmed, and it is the same root cause as the
eleven-hour gap in section 1: `nodal init` was never run in anger on this project. This
change fills the three commands in.

The pruning line explains where the disk claim comes from and where it does not. The
pnpm project returned nothing at reclaim, because pnpm keeps its dependencies in a
content store and the homes shared blocks with the base from the start. There was
nothing to prune. `nodal` returned 6.07 GB per unit on average, because `cargo` writes a
large, exclusive `target` into every home. This split was predicted, and so was the rule
behind it: the disk headline must come from build state. On this machine it does, and
all of it comes from the Rust project.

### Seven commands in the contract do not exist

`docs/contracts.md:203` lists the CLI as 28 commands. The binary has 21. These seven are in
the contract and not in the binary: `start`, `note`, `ask`, `handoff`, `sync`, `prune`,
`status`. The event schema has `handoff` and `sync` kinds, and 3 handoff events exist, so
the gap is the command surface and not the model.

## 3. Where Nodal could not answer, and I used `git`

The brief asked for these to be written down. They are the most useful output of the task,
because a gap that pushes the investigator out of the product pushes a user out of it too.

### The safety verdict rests on a cache, and does not say so

`nodal doctor --machine ~/Projects` reports the 34 clones as one group and ends the row with
`nothing unique`. `doctor/branches.rs:18` states the rule: a commit is unpushed when it
exists **on no remote-tracking ref**. Remote-tracking refs are a local cache of the remote,
written at the last fetch.

Ten of the 34 clones sit on branches the remote no longer has. `git ls-remote --heads
origin` returns 43 branches. `t0.3`, `t0.3b`, `t0.4`, `t0.5`, `t0.6`, `t0.7`, `t0.8`,
`t0.11`, `t0.12` and `t0.13` are not among them. Each clone still holds
`refs/remotes/origin/<branch>`, so `doctor` counts zero unpushed commits and calls the clone
safe.

I could not confirm or refute that inside Nodal. I used `git ls-remote` to read the live
remote, then `git merge-base --is-ancestor` to test each clone's HEAD against `main`, then
`git cherry` to test whether each commit had an equivalent already in `main`.

The verdict turned out to be correct. Across the 102 commits `git cherry` compared in those
ten clones, exactly one has no equivalent in `main`: `41c723d` in `nodal-t0.12`, which added
the startup benchmark. `benches/startup` is in `main` today and `main` is ahead of that commit
by 15 lines. Nothing is lost.

It was correct, but not for the reason `doctor` gave. Had a branch been deleted before its
work landed, `doctor` would have said `nothing unique` about the only copy. This is the one
finding in this report that concerns data loss, and it is a reporting gap, not a bug:
`doctor` is right to make no network call, and wrong to present a cached answer as a
current one without its age.

### The registry cannot say which commands were used

`runtime/run.rs:257` is the only writer of `command` events. Lifecycle operations write
their own rows. Nothing records an invocation of `ls`, `show`, `explain`, `cd`, `env`, `ps`,
`gc` or `doctor`. A tool whose first claim is visibility has no view of its own use. I could
measure adoption of `new`, `reclaim`, `run` and `done`, and nothing else.

### `doctor` reports a size that is not the size

`doctor --machine` sums file sizes per path. `cargo` hardlinks its artifacts, so a file with
two names is counted twice. On the five built-in clones that inflates the total from
53.35 GB to 57.85 GB, which is about 8%. `nodal-t0.10` alone holds 11,814 files with more
than one link.

`doctor` also cannot say what a clone shares with another tree and what it owns alone. That
second number is the one that says what removal frees. I used `du -sb`, which is
hardlink-aware, and `btrfs filesystem du -s`, which reports exclusive bytes. The two agree
with each other and not with `doctor`.

Compression is not the problem here, which surprised me. This filesystem runs
`compress=zstd:3`, and the on-disk size of these trees matches their apparent size. Rust
build artifacts do not give zstd anything to work with.

### Smaller ones

- `doctor --machine --json` gives per-clone rows. The table gives only the group. The
  question "which of these 34" is answered in the JSON and not on the screen.
- A cold base build needs the network. It clones `origin`, which is
  `https://github.com/josh2c/nodal`, even though a complete checkout sits on the same disk.
  With an unreachable proxy the build fails at the `clone` step. This is deliberate: a
  checkout is cloned only when the project names no `origin`. The claim in
  `README.md:80` still holds, because git does the work and the progress line names it.
- `nodal new` refuses to make a home inside another unit's home. That is correct, and the
  message said so clearly. It is recorded here as the one guard that fired during this task.

## 4. What should happen to the 34

**Nothing here was removed. The decision is the maintainer's.** This section says what is safe
and why, in enough detail to check.

The 34 clones hold 53.4 GB. That total is not spread across them. Five clones hold
53.35 GB of it, and all of that is `target`. The other 29 clones hold 57.6 MB between them,
because they were never built in.

Two sizes disagree here, and the smaller one is right. `doctor --machine` reports 57.85 GB
for the same five clones. `du` reports 53.35 GB. The difference is hardlinks: `cargo`
hardlinks its artifacts, `nodal-t0.10` alone holds 11,814 files with more than one link, and
a sum over paths counts each of them once per path. `btrfs filesystem du` puts
`nodal-t0.10` at 12.05 GiB exclusive, which agrees with `du` and not with `doctor`. For a
person deciding what to delete, `doctor` overstates the prize by about 8%.

Compression does not change the answer. This filesystem runs `compress=zstd:3`, but the
on-disk size of these five trees equals their apparent size to two decimal places.
Compression recovered nothing measurable on Rust build output.

| Group | Clones | Size | Verdict |
| --- | --- | --- | --- |
| Built in, fully represented on the remote and in `main` | 5 | 53,349 MB | Safe to remove |
| Never built in, fully represented | 19 | 48.5 MB | Safe to remove, frees nothing |
| Never built in, the only copy of their commits | 10 | 9.1 MB | Keep, or push the branches first |

The five are `nodal-t0.10`, `nodal-tls2`, `nodal-t2.7`, `nodal-t1.0b` and `nodal-t1.0c`.
Each one's HEAD is an ancestor of `main` in `~/Projects/nodal`, and each one's branch is
still on the remote. Removing those five frees 99.9% of the space and risks nothing.

The ten that need care are the cheapest to keep. They cost 9.1 MB together, which is 0.017%
of the total. Two ways to make them safe:

- Push the ten branches back to the remote. Then every clone is reproducible and all 34 can
  go.
- Keep the ten folders. They are smaller than a single build artifact.

Every clone is clean. No clone holds a stash, an uncommitted change, or an ignored file
outside `target` and `.claude`. I checked all 34.

### On the 50 GB

The brief calls the 50 GB the most persuasive number we own. I would not lead with it. The
measured figure is 53.4 GB across the 34, and the trash in `~/.nodal` holds 72 GB
at this moment from reclaimed units. The clone pile is not the larger mess. It is also not
the more interesting one: 29 of the 34 cost nothing, and the five that cost something are
five folders a person can delete in one command once they know it is safe.

The persuasive number is not 50 GB. It is 218.67 GB, which is what `reclaim` gave back over
four days without anyone deciding anything. The clone pile is what a person must decide
about. The reclaimed volume is what they did not have to.

### What it costs to learn this without Nodal

The brief guessed that visibility is the strongest claim we have, and that counting what a
person must type would show it. It does, with one correction.

| Route | What the person types | Median of 5 | What comes back |
| --- | --- | --- | --- |
| Nodal | `nodal doctor --machine ~/Projects`, 33 characters | 470 ms | 3 groups, sizes, ignored bulk, dirt, unpushed counts, and a verdict |
| By hand | 15 lines of shell, 894 characters | 639 ms | 37 unlabelled rows of numbers |

The hand-written equivalent does the same walk. It is 27 times more to type, it is slower,
and it stops one step short of an answer. It returns the inputs to a decision, and `doctor`
returns the decision.

```sh
cd ~/Projects || exit 1
for g in $(find . -maxdepth 6 -name .git -type d -printf '%h\n' 2>/dev/null); do
  origin=$(git -C "$g" remote get-url origin 2>/dev/null)
  size=$(du -sb "$g" 2>/dev/null | cut -f1)
  ign=$(du -sb "$g/target" 2>/dev/null | cut -f1)
  dirty=$(git -C "$g" status --porcelain 2>/dev/null | wc -l)
  unp=$(git -C "$g" for-each-ref --format='%(refname)' refs/heads |
        while read -r r; do git -C "$g" rev-list --count --not --remotes -- "$r" 2>/dev/null; done |
        awk '{s+=$1} END{print s+0}')
  last=$(git -C "$g" log -1 --format=%cI 2>/dev/null)
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$g" "$origin" "$size" "${ign:-0}" "$dirty" "$unp" "$last"
done | sort -t$'\t' -k2,2 -k3,3gr
```

The correction is the one in section 3. Neither route answers the question that was
actually asked. `doctor` says `nothing unique` and the hand-written version says
`unpushed=0`, and both are reading the same stale cache. To trust either I had to run `git
ls-remote` and `git cherry`, which is three more commands and a scratch repository. The
visibility claim is strong and it is worth leading with. What it needs is for the verdict to
carry its own uncertainty.

## 5. What the old way cost, and what the new way costs

The headline of this section changed twice while I measured it. The shipped behaviour does
not beat a plain clone. The design does, by a lot, and one defect stands between them.

### Making the folder

Median of five runs.

| How you start work | Median | What you get |
| --- | --- | --- |
| `git clone` from GitHub, which is what the 34 were | 1,157 ms | sources |
| `git clone` from the checkout on the same disk | 24 ms | sources |
| `nodal new`, first ever run, base built from the remote | 1,544 ms | sources |
| `nodal new`, base of sources already built | 291 ms | sources |
| `nodal new`, base of 7.7 GB that carries a build | 938 ms | sources and a build |

Against the thing this project actually did, `nodal new` is about four times faster. Against
a local `git clone` it is twelve times slower. Both are noise next to what comes next, and
the README's five seconds matches none of them.

### Making the folder work

A folder that arrives in half a second is worth nothing if the project will not run in it.
The gate is `cargo test --workspace` returning zero. Median of five runs.

| Starting point | Time to green | Crates compiled | Disk it owns after |
| --- | --- | --- | --- |
| Fresh `git clone` | 119.8 s | 102 | 7.29 GB |
| Unit from a base as `nodal base build --warm` delivers it | 124.3 s | 3 | 6.33 GiB |
| Unit from the same base, once that base is genuinely fresh | **45.9 s** | **0** | **104 KiB** |

Those three rows are the report. Read them in order.

**A warm base as Nodal delivers it makes this project slower.** 119.8 s cold against 124.3 s
warm. The warm home compiles three crates instead of 102 and still loses, because the 99
dependencies are small and the three workspace crates are where the time is. This project's
`target` is dominated by its own test binaries. It has 66 integration-test files across three
crates, and each one links the whole library, so rebuilding three crates rebuilds nearly
everything.

**The same base, made fresh, is 2.6 times faster than a cold clone** and costs 104 KiB per
unit instead of 7.29 GB. Nothing compiles. The 45.9 s is the test suite's own running time,
which is the floor. That is the product working exactly as the README describes it.

### The defect between those two rows

A base built by `nodal base build --warm` is stale when it is handed over. Running the
project's own test command inside the base itself, in its own directory, recompiles three
crates and takes 82.3 s. A base is supposed to be the thing that has already done that.

I isolated it against four alternatives. None of them is the cause.

| Control | Compiled | Time |
| --- | --- | --- |
| The base, in place, immediately after its own warm build | 3 | 82.3 s |
| The same base, in place, run a second time | 0 | 0.04 s |
| That tree copied with `cp -a --reflink` to a new path | 0 | 0.04 s |
| That tree moved to a new path with `mv` | 0 | 0.03 s |
| `cp -a --reflink` of the delivered base, no Nodal (3 runs) | 3 | 127.6 s |

So Nodal's copier is not the cause: a plain `cp -a` of the delivered base recompiles the
same three crates. And `nodal new` is not the cause: units made from the base after I made
it fresh compiled nothing at all.

The cause is the base build's own sequence. The `warm` step ran in a `.partial` directory
and `Promote` renamed that directory afterwards. Cargo records an absolute path for every
source file outside a package root, and this project has three of them: the shell start-up
files that `crates/nodal-core/src/runtime/init.rs` reads with `include_str!`. The
fingerprint of `nodal-core` therefore named a path under `.partial`, the rename took that
path away, and Cargo reported a missing file. `nodal-safety` and `nodal-cli` went stale
behind it. Those are the three crates.

Restoring only that one path, with nothing else changed, makes the same tree compile
nothing. Taking it away again brings the three crates back.

The fix is not a second run of the build command. It is that every step of a base build now
runs at the path the base is handed over at, with a mark beside the directory saying it is
not a base yet until the last step takes the mark off. The three crates are zero on the
delivered base, on a copy of it, and in a unit made from it.

### Disk

| State | Apparent | Exclusive |
| --- | --- | --- |
| Fresh clone after the suite runs | 7.29 GB | 7.29 GB |
| Unit from a stale warm base, after the suite runs | 7.69 GiB | 6.33 GiB |
| Unit from a fresh warm base, after the suite runs | 7.69 GiB | 104 KiB |

Copy-on-write works exactly as claimed, and the claim is worth more than the README says.
A unit that does not recompile owns 104 KiB after running the whole test suite. A unit that
recompiles three crates owns 6.33 GiB, because recompiling those three rewrites nearly every
artifact and every rewrite breaks the sharing.

### The tenth parallel task

Ten tasks on this project, on this machine, from a standing start.

| Model | Wall time of the work | Disk at the end |
| --- | --- | --- |
| The 34-clone era: clone from GitHub, build each | 10 x 121.0 s | 72.9 GB |
| Nodal as it ships: no warm build on the `new` path | 1.5 s + 10 x 120.1 s | 72.9 GB |
| Nodal with `base build --warm` as it behaves today | 119.0 s + 10 x 124.8 s | 70.1 GiB |
| Nodal with a base that is actually fresh | 119.0 s + 10 x 46.8 s | 7.7 GiB |

The tenth task costs about 120 s and 7 GB in the first three models and 47 s and 104 KiB in
the fourth. End to end the fourth model is 2.1 times faster than the clone era and uses 9.5
times less disk.

The brief guessed that a Rust project where `cargo build` dominates is close to our worst
case. On the shipped behaviour that guess is right, and the honest answer is worse than
"smaller than we claim": the saving is zero or negative on time and 13% on disk. On the
behaviour the design describes, the guess is wrong in our favour, and Rust is close to our
best case, because a compiled language is exactly where a shared build tree pays.

### So where does Nodal win today

Not on create, which is 0.9 s out of 120 s. Not on the build, which is the same work in
three of the four models. Today it wins on what happens after the work is done. The clone
era left 53.4 GB on disk for a person to judge, and it is still there. The unit era returned
218.67 GB automatically across 46 reclaims and left nothing to judge.

## 6. Where the measurement disagrees with what we wrote

| Document | Claim | Measured |
| --- | --- | --- |
| `README.md:110` | "a unit on this repository is ready in about five seconds" | `nodal new` returns in 0.29 s to 0.94 s, depending on the base. The project passes its own tests 119.8 s after a cold clone, or 45.9 s from a fresh base. No measurement produced five seconds. |
| `README.md:106` | "Ready, not empty. Dependencies are already there." | True for dependencies. `cargo fetch` runs in the base. No build runs, because `WARM_BUILD` is `false` on the `new` path. |
| `docs/contracts.md:203` | 28 commands | 21 commands. Seven do not exist. |
| Earlier conclusion | "Warm dependencies transfer; warm builds do not." | Measured on macOS with webpack. On this Linux host with btrfs and cargo, warm builds transfer completely: a unit from a fresh base compiles nothing and reaches green in 45.9 s against 119.8 s cold, owning 104 KiB. Its stated "revisit when" condition is met twice over. The host is Linux, and "Cargo target dirs" is the named trigger. |
| `README.md:80` | "Nodal makes no network calls of its own" | Holds. A cold base build clones `origin` over the network, which is git talking to a configured remote, and the progress line names it. |

That earlier conclusion is the one worth reopening. It held that warm builds do not
transfer, and it was right about the machine and the stack it measured. Both have changed.

## 7. What I would build next, in order

1. **Make a base build hand over a tree that is actually built.** This is first because it
   is worth 78 s and 6.3 GiB on every unit of this project, and because it silently costs
   that today. A base built with `--warm` recompiles three crates the moment anything runs
   in it, in its own directory, before it is copied anywhere. The fix is a step at the end
   of the base build: run the project's build command a second time and fail the build if it
   compiles anything. That converts a silent loss into a loud one, and whatever it catches
   is the real defect. Section 5 has the controls that rule out the copier, the path, the
   rename and `nodal new`.

2. **Give `doctor` an age for what it knows about the remote.** It reported `nothing unique`
   about ten clones on the strength of remote-tracking refs that are three days stale and
   name branches the remote has deleted. The verdict happened to be right. The method cannot
   tell a branch that was merged from a branch that was lost. `doctor` should print when each
   repository last fetched and mark a row whose safety rests on a ref older than that. This
   is the only finding here that touches data loss.

3. **Count a hardlink once, and report exclusive bytes.** `doctor` overstates this tree by
   about 8% because it sums file sizes per path and `cargo` hardlinks its artifacts. The
   number a person acts on is what removal frees, which is the exclusive size.

4. **Decide whether `nodal new` warms.** `WARM_BUILD` is hard-coded `false` at
   `lifecycle/ops/new.rs:97` and `lifecycle/ops/adopt.rs:92`, so the path every user takes
   never warms. With item 1 fixed, warming costs 119 s once and saves 74 s on every unit
   after the first. Without item 1 fixed, warming makes things worse. The order matters.

5. **Let a person read the registry's own log.** 1180 commands are recorded and no command
   prints them. A tool that claims visibility should answer "what did I run in this unit on
   Tuesday".

6. **Cut the seven commands from `docs/contracts.md`, or build them.** A contract that
   promises `status`, `handoff` and `sync` and ships none of them is a contract a reader
   stops trusting.

## Method

Everything ran on this machine: Linux 7.1.9, 28 cores, btrfs on `/home` with
`compress=zstd:3`, `nodal 0.1.0` built `--release` from `1b8b472`. Timed figures are the
median of five runs unless the row says otherwise. Each benchmark ran through `nodal run`,
so the registry holds it.

**Provenance of the 34.** `stat -c %w` for the birth time of each directory, and the first
line of `.git/logs/HEAD` for how it was made and when. `git log --first-parent` on `main`
for when each pull request landed. The registry's `unit.created_at` for when each unit was
made. Registry times are UTC and directory times are local, so both are stated in UTC where
they are compared.

**What is safe to delete.** `nodal doctor --machine ~/Projects --json` for sizes and
unpushed counts. `git ls-remote --heads origin` for what the remote actually holds today.
`git merge-base --is-ancestor <head> main` for whether a clone's tip is in `main`. `git
cherry` in a scratch repository holding `main` and all ten orphan heads, for whether each
commit has an equivalent already applied. `du -sb` for size, and `btrfs filesystem du -s`
for what removal would free.

**Create.** A fresh `NODAL_HOME` under `~/.cache` for a cold registry, and one shared
`NODAL_HOME` for warm runs. Timed with `date +%s%3N` around the command.

**Time to green.** `cargo test --workspace` returning zero. `grep -c Compiling` on the log
for how many crates were built. The cold arm is a fresh `git clone` of the checkout, removed
between runs.

**The four controls in section 5.** Each ran `cargo test --workspace --no-run` and counted
`Compiling` lines: in the base immediately after its own warm build, in the same base a
second time, in a `cp -a --reflink=always` copy of it at a new path, and in that copy after
`mv` to another path.

**The registry.** Read with `sqlite3` against `~/.nodal/registry.db`, because Nodal has no
command that prints its own history. That is item 5 in the list above.

Nothing in `~/Projects` was changed or removed. The benchmark wrote only to
`~/.cache/nodal-bench-benchmark-1` and to temporary registries under it.

## What this change contains

Two edits, both of them things the measurements proved wrong in this repository.

- `README.md` no longer claims a unit is ready in about five seconds. It states what was
  measured, and it says that a base carries a build only when `nodal base build --warm` made
  it. The old wording is the one that sent me looking for a five-second number that does not
  exist.
- `nodal.toml` gains the three commands this project actually uses. It had none. An empty
  `[commands]` is why no base here was ever warmed, and it is the same root cause as the
  eleven-hour gap in section 1.

No behaviour changed. Items 1 to 4 above are product decisions, and section 5 says what each
one is worth, so the maintainers can price them before anyone writes the code.

## The table again

| Part | Percent | LOE | Certainty |
| --- | --- | --- | --- |
| Where the 34 came from, and whether Nodal was there | 100% | M | 0.95 |
| What the old way cost, and what the new way costs | 100% | L | 0.95 |
| The defect that separates the two | diagnosed and repaired | M | 0.99 |
| What the registry says about a week of real use | 100% | M | 0.80 |
| Ready, or only installed | 100% | L | 1.00 |
| What should happen to the 34 | 100% | M | 0.98 |
| README corrected | 100% | S | 1.00 |
| The project's own recipe filled in | 100% | S | 1.00 |

Certainty rose on the cost question and on readiness, because the last measurements replaced
an inference with a control. It is 0.85 on the defect because I proved where it is not, and
did not prove where it is. It stays lowest on the registry, because the registry does not
record the read commands.

## `nodal show benchmark-1`

```
  unit       benchmark-1  (review)
  objective  measure nodal against the clones this project was built with
  branch     nodal/benchmark-1
  freshness  —
  main       open +2
  remote     —
  who        claude-code holds 7 h · claude-code 53
  age        1 h
  home       ~/.nodal/nodal/e/7XM8NZ76
  env        stopped · managed
  ports      app 20000
  running    —
  disk       —
  last       1 h ago

  WHEN        KIND     ACTOR        HOW  WHAT
  1 h ago     command  claude-code  saw  ~/Projects/nodal/target/release/nodal doctor --machine ~/Projects
  1 h ago     command  claude-code  saw  ~/Projects/nodal/target/release/nodal doctor --machine ~/Projects --json
  1 h ago     command  claude-code  saw  bash ~/.cache/nodal-bench-benchmark-1/create_bench.sh
  1 h ago     command  claude-code  saw  bash ~/.cache/nodal-bench-benchmark-1/ready_bench.sh
  44 min ago  command  claude-code  saw  bash ~/.cache/nodal-bench-benchmark-1/cold_bench.sh
  30 min ago  command  claude-code  saw  bash ~/.cache/nodal-bench-benchmark-1/best_bench.sh
  7 min ago   command  claude-code  saw  bash ~/.cache/nodal-bench-benchmark-1/fresh_bench.sh
  1 min ago   command  claude-code  saw  cargo test --workspace --locked
  now         sync     claude-code  saw  pushed refs/heads/nodal/benchmark-1 for review
```
