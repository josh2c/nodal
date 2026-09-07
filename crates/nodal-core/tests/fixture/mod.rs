//! The fixture project the recipe acceptance test runs against.
//!
//! A small monorepo of the shape the inference engine was built for: a pnpm workspace
//! with a task cache, a pinned toolchain, a local Supabase stack with migrations, and a
//! declared environment where every name is either generated per unit or a credential.
//! That last property is what makes it the zero-gap case: nothing about it is left for
//! a person to say.
//!
//! It is generated rather than committed so that the test states what each file
//! contributes, and so that a source that starts reading a new file fails here rather
//! than silently finding nothing. No value in it is a secret: the credential names are
//! declared with empty values, which is what a real `.env.example` does.

#![allow(dead_code, reason = "each test binary uses the part of the fixture it needs")]

use std::path::{Path, PathBuf};

/// One file of the fixture: where it goes and what is in it.
type File = (&'static str, &'static str);

/// The lockfile names the package manager; its contents are never read.
const LOCKFILE: File = ("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");

/// The workspace file and the task cache make it a monorepo.
const WORKSPACE: File = ("pnpm-workspace.yaml", "packages:\n  - apps/*\n");
const TURBO: File = ("turbo.json", "{ \"tasks\": { \"build\": {} } }\n");

/// The pin file a version manager reads, and the manifest that repeats it.
const NODE_VERSION: File = (".node-version", "22.11.0\n");
const PACKAGE_JSON: File = (
    "package.json",
    r#"{
  "name": "fixture",
  "private": true,
  "packageManager": "pnpm@9.12.3",
  "engines": { "node": "22.11.0" },
  "scripts": {
    "dev": "turbo run dev",
    "build": "turbo run build",
    "test": "vitest run",
    "lint": "turbo run lint",
    "typecheck": "turbo run typecheck",
    "db:migrate": "supabase db push",
    "db:reset": "supabase db reset",
    "db:seed": "supabase db seed"
  }
}
"#,
);

/// The local stack: its pinned ports, and a migration so the directory is real.
const SUPABASE_CONFIG: File = (
    "supabase/config.toml",
    r#"project_id = "fixture"

[api]
enabled = true
port = 54321

[db]
port = 54322

[db.pooler]
port = 54329

[studio]
port = 54323

[storage]
enabled = true
"#,
);
const MIGRATION: File =
    ("supabase/migrations/0001_init.sql", "create table thing (id uuid primary key);\n");

/// Every declared name is either generated per unit or a credential, so nothing is
/// left over and the fixture has no gaps. Credentials are declared with no value.
const ENV_EXAMPLE: File = (
    ".env.example",
    "PORT=3000\n\
     APP_URL=http://localhost:3000\n\
     DATABASE_URL=postgresql://postgres@localhost:54322/postgres\n\
     NEXT_PUBLIC_SUPABASE_URL=http://localhost:54321\n\
     NEXT_PUBLIC_SUPABASE_ANON_KEY=\n\
     SUPABASE_SERVICE_ROLE_KEY=\n\
     RESEND_API_KEY=\n\
     CRON_SECRET=\n",
);

/// Regenerated output a base clone should not carry.
const TEST_RESULTS: File = ("test-results/.keep", "");
const COVERAGE: File = ("coverage/.keep", "");

/// The image definition, which the recipe records but does not act on.
const DOCKERFILE: File = ("Dockerfile", "FROM node:22-slim\n");

/// Every file, in no particular order.
const FILES: &[File] = &[
    LOCKFILE,
    WORKSPACE,
    TURBO,
    NODE_VERSION,
    PACKAGE_JSON,
    SUPABASE_CONFIG,
    MIGRATION,
    ENV_EXAMPLE,
    TEST_RESULTS,
    COVERAGE,
    DOCKERFILE,
];

/// Write the fixture under `root` and return it. Overwrites whatever is there.
///
/// # Panics
///
/// If the fixture cannot be written, which means the test cannot run at all.
pub fn write(root: impl AsRef<Path>) -> PathBuf {
    let root = root.as_ref().to_path_buf();
    for (relative, contents) in FILES {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|error| {
                panic!("fixture: create {}: {error}", parent.display());
            });
        }
        std::fs::write(&path, contents).unwrap_or_else(|error| {
            panic!("fixture: write {}: {error}", path.display());
        });
    }
    root
}
