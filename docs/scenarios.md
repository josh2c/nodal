# Scenarios: what a user actually types and sees

Project: the project (pnpm monorepo, Next.js, Supabase). Agents: Claude Code, Codex.

## 1. First time on a project

```
$ cd ~/code/project
$ nodal init
  detected   pnpm 10.34 · node 24.19 (engines) · turborepo · supabase migrations (680) · Dockerfile
  services   supabase local stack → shared; per-unit: postgrest, gotrue
  env        45 names in .env.example: 12 generated per unit · 15 secrets · 18 other
  base       excluded from clones: .claude/worktrees .playbook/runs test-results .next

  needs you  which env names are required for local dev?  [edit nodal.toml → env.required_local]
  wrote      nodal.toml (review), .nodal/ (registry)

$ eval "$(nodal shell-init zsh)"            # optional: lets `nodal new` cd into the unit for you
```

## 2. Starting a feature with Claude

```
$ nodal new "worker import: handle missing supervisor_id"
  base       first use: clone origin/main … install … template (migrate 680, seed, freeze) … 1m 44s  (once)
  unit       worker-import   (nodal/worker-import)          [--name to choose]
  home       ~/.nodal/project/e/01J9X2K4/         2.1 s · 21 MB
  database   project_u_01j9x2k4   from template a41c … 0.9 s
  api        postgrest :54401 · gotrue :54402 · router http://localhost:54400
  app        PORT 41230 · APP_URL http://localhost:41230
  context    WORKUNIT.md written · CLAUDE.md and AGENTS.md pointer added
  isolation  db, rest, auth per unit · storage and realtime shared (see `nodal explain`)
  ready in 3.4 s → ~/.nodal/project/e/01J9X2K4

$ claude                                      # shell-init already cd'd you in; or cd there yourself
```

Claude reads `WORKUNIT.md` (objective, empty history), investigates, edits, runs `pnpm test` (the shim
records: 3 failing → 0 failing), commits twice. you closes the session.

```
$ nodal handoff "legacy date parser still fails on two-digit years"     # explicit; nothing prompts at exit
  snapshot   refs/nodal/01J9X2K4/wip (3 uncommitted files kept safe; also taken every 5 min while active)
```

## 3. A second feature in parallel with Codex, same afternoon

```
$ nodal new "payroll export CSV"
  unit       payroll-export · home e/01J9X3M8 · db project_u_01j9x3m8 · PORT 41231 · ready in 3.1 s
$ codex
```

Both dev servers run at once on different ports against different databases on the one Supabase stack.

```
$ nodal
  UNIT             STATE   BRANCH                        DISK    RUNNING          LAST
  worker-import    open    nodal/worker-import +2        287 MB  —                12 min ago
  payroll-export   open    nodal/payroll-export          301 MB  next dev :41231  now
  shared: supabase stack (10 containers) · pnpm store 2.4 GB · base 7f3e (1 pinned)
```

## 4. Claude's session dies mid-task

Laptop lid closed, terminal gone, Claude never wrote a handoff.

```
$ nodal cd worker-import
  recomputed  branch +2 commits · 3 files modified since last commit · last test run 14:02: 0 failing
  wip         snapshot 14:09 matches working tree
$ cat WORKUNIT.md
  # worker-import
  Objective: worker import: handle missing supervisor_id
  State: branch nodal/… · 2 commits ahead of main · 3 uncommitted files (src/import/parse.ts, dates.ts, test/import.spec.ts)
  Last commands: pnpm test (0 failing, 14:02) · pnpm test (3 failing, 13:41)
  Handoff (you, 14:10): legacy date parser still fails on two-digit years
  Notes: none
```

Nothing was lost; nothing had to be rebuilt.

## 5. Handing the same unit to Codex, then to a human reviewer

```
$ nodal start worker-import --agent codex
  context    WORKUNIT.md recompiled · AGENTS.md pointer present
  codex …    (works, runs tests, commits "fix two-digit year parsing", exits)
  handoff    (codex): parser fixed; import job may double-count on retry — unverified
$ nodal done worker-import
  pushed     branch + wip ref to origin · open a PR: https://github.com/…/compare/nodal/worker-import
  runtime    stopped (dev server, api layer) · database kept · home kept
```

Reviewer (A teammate, same machine or hers after `nodal pull`):

```
$ nodal open worker-import --readonly
  materialized read-only copy from branch + wip · preview http://localhost:41240
  no write lock taken; writer remains: you@laptop
```

## 6. A teammate continues the work on her machine

```
teammate$ nodal pull worker-import
  bundle     from github.com/…/nodal-state: manifest · 14 events · db dump (whole unit db, flagged, 48 MB)
  lock       taken by teammate@linux (you@laptop released at `done`)
  base       ws-fp 7f3e present locally (warm) · schema-fp a41c template present
  home       created 1.8 s · db restored 1.4 s · secrets: 2 of 10 missing → STRIPE_KEY, RESEND_API_KEY (unit starts anyway)
teammate$ nodal cd worker-import && claude
```

No install, no seed, no rebuild. Her Linux home keeps a warm build cache too.

## 7. Main moved for a week under an open unit

```
$ nodal
  payroll-export   open   STALE (deps, schema)   …
$ git rebase main                                  # you's own Git, his tools
$ nodal sync payroll-export
  deps       lockfile changed → pnpm install (incremental) … 6 s
  schema     2 new migrations on main → plan: apply 20260901_…, 20260903_…   (run with --apply)
  services   unchanged
  generated  prisma client regenerate … 4 s
$ nodal sync payroll-export --apply
  schema     applied 2 migrations to project_u_01j9x3m8 … 1.2 s
  runtime    restarted next dev :41231
  fresh
```

## 8. A team adopting Nodal without changing how they work

```
$ cd ~/code/project            # engineer's existing checkout on feature/auth-refresh
$ nodal adopt feature/auth-refresh --in-place
  home       this checkout (unmanaged: never reclaimed, no CoW savings)
  database   project_u_01j9y0aa from template a41c … 0.9 s
  activation .nodal/env + .envrc written, hidden via .git/info/exclude (git status unchanged)
  audit      .env.local has 6 names not in .env.example: R2_ACCOUNT_ID, …  (add to nodal.toml or ignore)
  staleness  tracked on every nodal command (no git hooks touched; husky left alone)
```

They keep their folder, their branch, their editor. They gained an isolated database, correct env, and context.

## 9. Cleaning up a machine that is already a mess

```
$ nodal doctor
  unmanaged, reclaimable
    .claude/worktrees/ (4 nested worktrees inside main checkout)         7.85 GB
    apps/web/.next in 3 worktrees                                        3.33 GB
    13 containers exited > 3 weeks (acme-replay stack)                   0.12 GB + 1.4 GB volumes
    2 databases matching project_u_* with no registry row               0.10 GB
  managed
    2 units · 0.6 GB private · shared base 1.16 GB · trash 0 B
  run `nodal doctor --clean` to remove the unmanaged items above (asks per group)
```

## 10. End of the unit

```
$ nodal reclaim worker-import
  check      branch merged (PR #1043) · no uncommitted work · no unique db rows flagged
  stop       0 processes · api layer containers removed · db dropped · ports released
  trash      home moved to ~/.nodal/project/trash/01J9X2K4 (gc in 7 days)
  verify     nothing left by id
$ nodal gc          # later, or automatically
  freed      312 MB
```
