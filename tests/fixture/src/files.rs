//! The contents of every fixture file, one constant per file.
//!
//! Each constant carries the reason its file is in the fixture: what a source reads out
//! of it, or what a working copy needs it for. A file nothing reads does not belong
//! here, and a file something reads and that is missing is a source that silently finds
//! nothing — which is the failure this fixture exists to turn into a red test.

/// The lockfile names the package manager. Its contents are never read by inference,
/// and it is deliberately a marker rather than a resolved tree: every dependency below
/// is pinned exactly, so resolution is deterministic without 60 KB of generated YAML
/// going stale in this repository. CI installs with `--no-frozen-lockfile`.
pub(crate) const LOCKFILE: &str = "lockfileVersion: '9.0'\n";

/// The workspace file makes it a monorepo, and names where the packages are.
pub(crate) const WORKSPACE: &str = "\
packages:
  - apps/*
  - packages/*
";

/// The task cache: a runner inference recognises, with the tasks the packages declare.
pub(crate) const TURBO_JSON: &str = r#"{
  "$schema": "https://turbo.build/schema.json",
  "tasks": {
    "build": {
      "dependsOn": ["^build"],
      "outputs": [".next/**", "!.next/cache/**", "dist/**"]
    },
    "typecheck": { "dependsOn": ["^build"] },
    "dev": { "cache": false, "persistent": true }
  }
}
"#;

/// The pin file a version manager reads. `engines` below repeats it, so the two agree
/// and inference records both under their own keys.
pub(crate) const NODE_VERSION: &str = "22.11.0\n";

/// The root manifest: the package-manager pin, the engines field, and every script the
/// recipe's commands are taken from. `db:*` are the migration commands; they wrap
/// `scripts/db.mjs` rather than a tool's own form, which is the case the migrations
/// source exists to prefer.
pub(crate) const PACKAGE_JSON: &str = r#"{
  "name": "nodal-fixture",
  "private": true,
  "packageManager": "pnpm@9.12.3",
  "engines": { "node": "22.11.0" },
  "scripts": {
    "dev": "turbo run dev",
    "build": "turbo run build",
    "lint": "node --check scripts/db.mjs && node --check tests/thing.test.mjs",
    "typecheck": "turbo run typecheck",
    "test": "node --test tests/*.test.mjs",
    "db:migrate": "node scripts/db.mjs migrate",
    "db:seed": "node scripts/db.mjs seed",
    "db:reset": "node scripts/db.mjs reset"
  },
  "devDependencies": {
    "turbo": "2.10.12",
    "typescript": "5.9.3"
  }
}
"#;

/// What a working copy regenerates and a base clone should therefore not carry.
pub(crate) const GITIGNORE: &str = "\
node_modules/
.next/
.turbo/
dist/
coverage/
test-results/
.env
";

/// The services the project develops against. Compose lists them; it does not say which
/// are safe to share, which is why the fixture carries a `nodal.toml` that does.
pub(crate) const COMPOSE: &str = r#"# The services this project develops against. `db` and `mailpit` are shared between
# working copies; each copy gets its own `redis`, because a shared cache is the fastest
# way for one copy's state to appear in another.
services:
  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: fixture
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD:-fixture}
      POSTGRES_DB: fixture
    ports:
      - "54322:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U fixture"]
      interval: 2s
      timeout: 2s
      retries: 15

  redis:
    image: redis:7-alpine
    command: ["redis-server", "--save", ""]
    ports:
      - "6379"

  mailpit:
    image: axllent/mailpit:latest
    ports:
      - "8025"
"#;

/// The recipe: the one thing about this project no file of its own states. Which
/// Compose services one instance serves every working copy from is a judgement, so it
/// is written rather than inferred, and answering it is what leaves the fixture with no
/// gaps. Everything else here is read out of the project.
pub(crate) const NODAL_TOML: &str = r#"# nodal.toml — the lines only a person could write.
#
# Everything else about this project `nodal init` reads from the project's own files.
# What is here is the judgement no file states: which services one instance serves every
# working copy from, and which each copy needs its own of. Compose lists the services;
# it does not say which of them are safe to share.

[db]
kind = "postgres"
url_var = ["DATABASE_URL"]

[services]
shared = ["db", "mailpit"]
per_unit = ["redis"]
"#;

/// The image definition, which the recipe records but does not act on.
pub(crate) const DOCKERFILE: &str = "\
FROM node:22-slim
WORKDIR /app
COPY . .
RUN corepack enable && pnpm install --frozen-lockfile && pnpm run build
CMD [\"pnpm\", \"--filter\", \"@fixture/web\", \"start\"]
";

/// The declared environment. Every name is either generated per working copy or a
/// credential, so nothing is left for a person to name and no gap is raised. No value
/// here is a secret: the credentials are declared empty, as they are in a real file.
pub(crate) const ENV_EXAMPLE: &str = "\
# Every name here is either generated per working copy or supplied by a secret source.
# Nothing in this file is a value: the credentials are declared empty, as they are in a
# real .env.example.
PORT=3000
APP_URL=http://localhost:3000
DATABASE_URL=postgresql://fixture@localhost:54322/fixture
POSTGRES_PASSWORD=
SESSION_SECRET=
RESEND_API_KEY=
CRON_SECRET=
SENTRY_DSN=
";

/// A second declaration file, so the union of two of them is what inference sorts. Both
/// declare `PORT`; the fixture holds the engine to naming it once.
pub(crate) const WEB_ENV_EXAMPLE: &str = "\
PORT=3000
NEXT_PUBLIC_APP_URL=http://localhost:3000
";

/// The migrations directory: a plain one, so the tool is not named by the layout and
/// the commands come from the project's own scripts instead.
pub(crate) const MIGRATION_0001: &str = "\
create table thing (
  id uuid primary key default gen_random_uuid(),
  name text not null
);
";

/// A second migration, so ordering is something the seed script has to get right.
pub(crate) const MIGRATION_0002: &str =
    "alter table thing add column created_at timestamptz not null default now();\n";

/// The seed, which the `db:seed` script applies after the migrations.
pub(crate) const SEED: &str = "\
insert into thing (name) values ('first'), ('second')
on conflict do nothing;
";

/// What `db:migrate`, `db:seed` and `db:reset` actually run. Dependency-free, so the
/// fixture installs and builds with no database anywhere near it.
pub(crate) const DB_SCRIPT: &str = r#"// Apply the project's migrations and seed with psql, in file-name order.
//
// Dependency-free on purpose: the fixture must install and build without a database,
// and this is the command `nodal.toml` names for migrate, seed and reset.
import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const MIGRATIONS = join(ROOT, "migrations");
const SEED = join(ROOT, "seed.sql");

const PLANS = {
  migrate: () => migrations(),
  seed: () => [SEED],
  reset: () => ["--reset", ...migrations(), SEED],
};

function migrations() {
  return readdirSync(MIGRATIONS)
    .filter((name) => name.endsWith(".sql"))
    .sort()
    .map((name) => join(MIGRATIONS, name));
}

function psql(url, argument) {
  const args =
    argument === "--reset"
      ? ["-c", "drop schema public cascade; create schema public;"]
      : ["-v", "ON_ERROR_STOP=1", "-f", argument];
  const { status } = spawnSync("psql", [url, ...args], { stdio: "inherit" });
  if (status !== 0) process.exit(status ?? 1);
}

const step = process.argv[2];
const plan = PLANS[step];
if (!plan) {
  console.error(`usage: db.mjs <${Object.keys(PLANS).join("|")}>`);
  process.exit(2);
}

const url = process.env.DATABASE_URL;
if (!url) {
  console.log(`${step}: DATABASE_URL is unset; would apply`);
  for (const item of plan()) console.log(`  ${item}`);
  process.exit(0);
}
for (const item of plan()) psql(url, item);
"#;

/// The project's own test suite, so `commands.test` names something that runs.
pub(crate) const SMOKE_TEST: &str = r#"import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

const root = new URL("..", import.meta.url).pathname;

test("every migration is applied in file-name order", () => {
  const names = readdirSync(`${root}/migrations`).sort();
  assert.deepEqual(names, ["0001_create_thing.sql", "0002_add_thing_created_at.sql"]);
});

test("the seed only inserts into a table a migration creates", () => {
  const seed = readFileSync(`${root}/seed.sql`, "utf8");
  const first = readFileSync(`${root}/migrations/0001_create_thing.sql`, "utf8");
  assert.match(seed, /insert into thing\b/);
  assert.match(first, /create table thing\b/);
});
"#;

/// The Next application's manifest. Its `dev` script reads `PORT`, which is one of the
/// names Nodal generates per working copy, so two copies can run side by side.
pub(crate) const WEB_PACKAGE_JSON: &str = r#"{
  "name": "@fixture/web",
  "version": "0.0.0",
  "private": true,
  "scripts": {
    "dev": "next dev --port ${PORT:-3000}",
    "build": "next build",
    "start": "next start --port ${PORT:-3000}",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "@fixture/config": "workspace:*",
    "next": "15.5.25",
    "react": "19.2.8",
    "react-dom": "19.2.8"
  },
  "devDependencies": {
    "@types/node": "22.20.1",
    "@types/react": "19.2.18",
    "typescript": "5.9.3"
  }
}
"#;

/// A plain build. `next start` serves it, which is what a working copy runs; a
/// standalone one traces every file under `node_modules` and is most of the build.
pub(crate) const WEB_NEXT_CONFIG: &str = "\
/** @type {import('next').NextConfig} */
export default {};
";

/// The compiler options Next would otherwise write into the file on first build.
/// Stating them here keeps a build from editing the fixture underneath itself.
pub(crate) const WEB_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["dom", "dom.iterable", "ES2022"],
    "jsx": "preserve",
    "module": "esnext",
    "moduleResolution": "bundler",
    "strict": true,
    "noEmit": true,
    "allowJs": true,
    "isolatedModules": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "resolveJsonModule": true,
    "incremental": true,
    "plugins": [{ "name": "next" }]
  },
  "include": ["app", "next-env.d.ts", ".next/types/**/*.ts"],
  "exclude": ["node_modules"]
}
"#;

/// The ambient types Next generates. Shipped for the same reason as the tsconfig above.
pub(crate) const WEB_NEXT_ENV: &str = "\
/// <reference types=\"next\" />
/// <reference types=\"next/image-types/global\" />
/// <reference path=\"./.next/types/routes.d.ts\" />

// NOTE: This file should not be edited
// see https://nextjs.org/docs/app/api-reference/config/typescript for more information.
";

/// The application shell.
pub(crate) const WEB_LAYOUT: &str = r#"import type { ReactNode } from "react";

export const metadata = { title: "Nodal fixture" };

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
"#;

/// The one page, which imports from the sibling package so the build has a real
/// dependency between two workspace packages to order.
pub(crate) const WEB_PAGE: &str = r#"import { appName, databaseUrl } from "@fixture/config";

export default function Page() {
  return (
    <main>
      <h1>{appName}</h1>
      <p>{databaseUrl() === "" ? "no database configured" : "database configured"}</p>
    </main>
  );
}
"#;

/// The second workspace package: what makes `^build` mean something.
pub(crate) const CONFIG_PACKAGE_JSON: &str = r#"{
  "name": "@fixture/config",
  "version": "0.0.0",
  "private": true,
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit"
  },
  "devDependencies": {
    "@types/node": "22.20.1",
    "typescript": "5.9.3"
  }
}
"#;

/// Emits declarations into `dist`, which is what the Next app resolves against.
pub(crate) const CONFIG_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "nodenext",
    "moduleResolution": "nodenext",
    "strict": true,
    "declaration": true,
    "outDir": "dist",
    "rootDir": "src",
    "types": ["node"]
  },
  "include": ["src"]
}
"#;

/// Reads `DATABASE_URL`, one of the names Nodal generates per working copy.
pub(crate) const CONFIG_INDEX: &str = r#"export const appName = "nodal fixture";

/** The connection string the app reads, or an empty string when it is unset. */
export function databaseUrl(): string {
  return process.env.DATABASE_URL ?? "";
}
"#;

/// Regenerated output a base clone should not carry. Present so that the exclusion
/// table has something to match, and empty so that carrying it would be pure waste.
pub(crate) const KEEP: &str = "";
