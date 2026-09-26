-- A frozen registry fixture: the schema and rows of version 4.
--
-- Data, not a script. It was written once, by the Nodal that appended migration
-- 4, and nothing regenerates it: a fixture rebuilt from today's migrations
-- moves whenever they move and proves nothing about the upgrade a person's
-- registry crosses. Edit it only to correct what version 4 really held.
--
-- `crates/nodal-core/tests/upgrade.rs` opens this with the current binary,
-- migrates it, and asserts every row below is still there with the meaning it
-- had here.

PRAGMA user_version = 4;

CREATE TABLE project (
    id          TEXT    PRIMARY KEY,
    root        TEXT    NOT NULL UNIQUE,
    name        TEXT    NOT NULL,
    recipe_hash TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
) STRICT;

CREATE TABLE base (
    id             TEXT    PRIMARY KEY,
    project_id     TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    ws_fingerprint TEXT    NOT NULL,
    platform       TEXT    NOT NULL,
    commit_id      TEXT    NOT NULL,
    path           TEXT    NOT NULL,
    built_at       INTEGER NOT NULL,
    last_used      INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX base_key ON base (project_id, ws_fingerprint, platform);

CREATE TABLE db_template (
    id                 TEXT    PRIMARY KEY,
    project_id         TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    schema_fingerprint TEXT    NOT NULL,
    db_name            TEXT    NOT NULL UNIQUE,
    parent_template_id TEXT             REFERENCES db_template (id) ON DELETE RESTRICT,
    built_at           INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX db_template_key ON db_template (project_id, schema_fingerprint);

CREATE TABLE unit (
    id            TEXT    PRIMARY KEY,
    project_id    TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    slug          TEXT    NOT NULL,
    objective     TEXT,
    branch        TEXT    NOT NULL,
    parent_branch TEXT,
    status        TEXT    NOT NULL CHECK (status IN ('open', 'review', 'merged', 'archived')),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX unit_slug ON unit (project_id, slug);

CREATE UNIQUE INDEX unit_open_branch ON unit (project_id, branch) WHERE status = 'open';

CREATE TABLE environment (
    id                    TEXT    PRIMARY KEY,
    unit_id               TEXT    NOT NULL REFERENCES unit (id) ON DELETE RESTRICT,
    attempt               INTEGER NOT NULL CHECK (attempt >= 1),
    home                  TEXT    NOT NULL,
    managed               INTEGER NOT NULL CHECK (managed IN (0, 1)),
    base_id               TEXT             REFERENCES base (id) ON DELETE RESTRICT,
    ws_fp_materialized    TEXT,
    schema_fp_materialized TEXT,
    host                  TEXT    NOT NULL,
    db_name               TEXT,
    ports                 TEXT    NOT NULL,
    fixed_port            INTEGER          CHECK (fixed_port IS NULL OR fixed_port BETWEEN 1 AND 65535),
    state                 TEXT    NOT NULL CHECK (state IN ('absent', 'stopped', 'running')),
    created_at            INTEGER NOT NULL,
    last_active           INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX environment_attempt ON environment (unit_id, attempt);

CREATE INDEX environment_state ON environment (state);

CREATE TABLE session (
    id             TEXT    PRIMARY KEY,
    environment_id TEXT    NOT NULL REFERENCES environment (id) ON DELETE RESTRICT,
    actor_kind     TEXT    NOT NULL CHECK (actor_kind IN ('human', 'agent')),
    actor_name     TEXT    NOT NULL,
    pid            INTEGER          CHECK (pid IS NULL OR pid >= 0),
    started_at     INTEGER NOT NULL,
    ended_at       INTEGER
) STRICT;

CREATE INDEX session_open ON session (environment_id) WHERE ended_at IS NULL;

CREATE TABLE event (
    id             TEXT    PRIMARY KEY,
    unit_id        TEXT    NOT NULL REFERENCES unit (id) ON DELETE RESTRICT,
    environment_id TEXT             REFERENCES environment (id) ON DELETE RESTRICT,
    ts             INTEGER NOT NULL,
    actor_kind     TEXT    NOT NULL CHECK (actor_kind IN ('human', 'agent')),
    actor_name     TEXT    NOT NULL,
    kind           TEXT    NOT NULL CHECK (kind IN (
        'attached', 'detached', 'command', 'commit', 'test_result', 'failure',
        'file_touched', 'finding', 'decision', 'question', 'handoff', 'sync', 'note'
    )),
    epistemic      TEXT    NOT NULL CHECK (epistemic IN ('observed', 'stated')),
    body           TEXT    NOT NULL,
    refs           TEXT    NOT NULL,
    raw_ref        TEXT
) STRICT;

CREATE INDEX event_unit ON event (unit_id, id);

CREATE TABLE lease (
    resource       TEXT    PRIMARY KEY,
    environment_id TEXT    NOT NULL REFERENCES environment (id) ON DELETE RESTRICT,
    expires_at     INTEGER NOT NULL
) STRICT;

CREATE TABLE lock (
    unit_id    TEXT    PRIMARY KEY REFERENCES unit (id) ON DELETE RESTRICT,
    host       TEXT    NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE TABLE operation (
    id         TEXT    PRIMARY KEY,
    kind       TEXT    NOT NULL,
    subject    TEXT    NOT NULL,
    params     TEXT    NOT NULL,
    recovery   TEXT    NOT NULL CHECK (recovery IN ('resume', 'roll_back')),
    state      TEXT    NOT NULL CHECK (state IN ('running', 'committed', 'rolled_back', 'failed')),
    host       TEXT    NOT NULL,
    pid        INTEGER NOT NULL CHECK (pid >= 0),
    started_at INTEGER NOT NULL,
    ended_at   INTEGER
) STRICT;

CREATE INDEX operation_unfinished ON operation (id) WHERE state = 'running';

CREATE TABLE operation_step (
    operation_id TEXT    NOT NULL REFERENCES operation (id) ON DELETE RESTRICT,
    position     INTEGER NOT NULL CHECK (position >= 0),
    key          TEXT    NOT NULL,
    state        TEXT    NOT NULL CHECK (state IN ('applying', 'applied', 'undone')),
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (operation_id, position)
) STRICT;

CREATE TABLE port_block (
    project_id TEXT    PRIMARY KEY REFERENCES project (id) ON DELETE RESTRICT,
    first      INTEGER NOT NULL CHECK (first BETWEEN 1 AND 65535),
    last       INTEGER NOT NULL CHECK (last BETWEEN first AND 65535)
) STRICT;

CREATE UNIQUE INDEX port_block_first ON port_block (first);

CREATE TABLE port_allocation (
    port           INTEGER PRIMARY KEY CHECK (port BETWEEN 1 AND 65535),
    project_id     TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    environment_id TEXT    NOT NULL REFERENCES environment (id) ON DELETE RESTRICT,
    name           TEXT    NOT NULL
) STRICT;

CREATE UNIQUE INDEX port_allocation_name ON port_allocation (environment_id, name);

CREATE INDEX port_allocation_environment ON port_allocation (environment_id);

CREATE TABLE trash (
    environment_id TEXT    PRIMARY KEY REFERENCES environment (id) ON DELETE RESTRICT,
    unit_id        TEXT    NOT NULL REFERENCES unit (id) ON DELETE RESTRICT,
    project_id     TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    slug           TEXT    NOT NULL,
    home           TEXT    NOT NULL,
    path           TEXT    NOT NULL,
    snapshot       TEXT,
    trashed_at     INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL
) STRICT;

CREATE INDEX trash_expiry ON trash (expires_at);

INSERT INTO project (id, root, name, recipe_hash, created_at)
VALUES ('01J8Z6H0000000000000000001', '/home/dev/acme', 'acme', '0f1e2d', 1788688800);
INSERT INTO base (id, project_id, ws_fingerprint, platform, commit_id, path, built_at, last_used)
VALUES ('01J8Z6H0000000000000000003', '01J8Z6H0000000000000000001', '7c1e9a2b', 'x86_64-unknown-linux-gnu', '9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001', '/home/dev/.nodal/acme/b/7c1e9a2b', 1788688810, 1788688820);
INSERT INTO db_template (id, project_id, schema_fingerprint, db_name, parent_template_id, built_at)
VALUES ('01J8Z6H0000000000000000004', '01J8Z6H0000000000000000001', '41bd', 'acme_t_41bd', NULL, 1788688830);
INSERT INTO unit (id, project_id, slug, objective, branch, parent_branch, status, created_at, updated_at)
VALUES ('01J8Z6H0000000000000000002', '01J8Z6H0000000000000000001', 'fix-worker-import', 'fix the worker import', 'nodal/fix-worker-import', 'main', 'open', 1788688860, 1788688860);
INSERT INTO unit (id, project_id, slug, objective, branch, parent_branch, status, created_at, updated_at)
VALUES ('01J8Z6H0000000000000000008', '01J8Z6H0000000000000000001', 'worker-import', 'import the workers', 'nodal/worker-import', 'main', 'archived', 1788688870, 1788688880);
INSERT INTO unit (id, project_id, slug, objective, branch, parent_branch, status, created_at, updated_at)
VALUES ('01J8Z6H0000000000000000009', '01J8Z6H0000000000000000001', 'worker-import-2', 'import the workers', 'nodal/worker-import', 'main', 'open', 1788688890, 1788688890);
INSERT INTO environment (id, unit_id, attempt, home, managed, base_id, ws_fp_materialized, schema_fp_materialized, host, db_name, ports, fixed_port, state, created_at, last_active)
VALUES ('01J8Z6H0000000000000000005', '01J8Z6H0000000000000000002', 1, '/home/dev/.nodal/acme/e/01J8Z6H0', 1, '01J8Z6H0000000000000000003', '7c1e9a2b', '41bd', 'laptop', 'acme_e_01j8z6h0', '{"app":20002}', 5432, 'stopped', 1788689040, 1788689040);
INSERT INTO environment (id, unit_id, attempt, home, managed, base_id, ws_fp_materialized, schema_fp_materialized, host, db_name, ports, fixed_port, state, created_at, last_active)
VALUES ('01J8Z6H0000000000000000011', '01J8Z6H0000000000000000008', 1, '/home/dev/.nodal/acme/e/01J8Z6H1', 1, NULL, NULL, NULL, 'laptop', NULL, '{}', NULL, 'absent', 1788688900, 1788688960);
INSERT INTO session (id, environment_id, actor_kind, actor_name, pid, started_at, ended_at)
VALUES ('01J8Z6H0000000000000000006', '01J8Z6H0000000000000000005', 'agent', 'claude-code', 4242, 1788689100, NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000007', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689160, 'human', 'dev', 'commit', 'observed', 'wired the worker import', '{"commit":"9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001"}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000020', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689110, 'agent', 'claude-code', 'attached', 'observed', 'the agent attached', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000021', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689120, 'agent', 'claude-code', 'detached', 'observed', 'the agent detached', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000022', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689130, 'agent', 'claude-code', 'command', 'observed', 'pnpm test', '{"exit":"0"}', '/home/dev/.nodal/acme/e/01J8Z6H0/.nodal/raw/01J8Z6H0000000000000000022.log');
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000023', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689140, 'agent', 'claude-code', 'test_result', 'observed', '41 passed, 0 failed', '{"passed":"41"}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000024', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689150, 'agent', 'claude-code', 'failure', 'observed', 'the worker pool would not start', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000025', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689170, 'agent', 'claude-code', 'file_touched', 'observed', 'src/worker/pool.ts', '{"file":"src/worker/pool.ts"}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000026', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689180, 'agent', 'claude-code', 'finding', 'stated', 'the import is circular', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000027', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689190, 'human', 'dev', 'decision', 'stated', 'the pool is constructed by the caller', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000028', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689200, 'agent', 'claude-code', 'question', 'stated', 'which module owns the pool?', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000029', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689210, 'agent', 'claude-code', 'handoff', 'stated', 'the pool is wired, the tests are not', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H000000000000000002A', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005', 1788689220, 'human', 'dev', 'sync', 'observed', 'the lockfile moved, dependencies reinstalled', '{}', NULL);
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H000000000000000002B', '01J8Z6H0000000000000000002', NULL, 1788689230, 'human', 'dev', 'note', 'stated', 'ask the platform team about the pool', '{}', NULL);
INSERT INTO lease (resource, environment_id, expires_at)
VALUES ('port:5432', '01J8Z6H0000000000000000005', 1788692700);
INSERT INTO lock (unit_id, host, expires_at)
VALUES ('01J8Z6H0000000000000000002', 'laptop', 1788692700);
INSERT INTO operation (id, kind, subject, params, recovery, state, host, pid, started_at, ended_at)
VALUES ('01J8Z6H0000000000000000012', 'new', 'fix-worker-import', '{"slug":"fix-worker-import"}', 'roll_back', 'committed', 'laptop', 4100, 1788688840, 1788688860);
INSERT INTO operation_step (operation_id, position, key, state, updated_at)
VALUES ('01J8Z6H0000000000000000012', 0, 'branch', 'applied', 1788688845);
INSERT INTO operation_step (operation_id, position, key, state, updated_at)
VALUES ('01J8Z6H0000000000000000012', 1, 'materialize', 'applied', 1788688855);
INSERT INTO port_block (project_id, first, last)
VALUES ('01J8Z6H0000000000000000001', 20000, 20009);
INSERT INTO port_allocation (port, project_id, environment_id, name)
VALUES (20002, '01J8Z6H0000000000000000001', '01J8Z6H0000000000000000005', 'app');
INSERT INTO trash (environment_id, unit_id, project_id, slug, home, path, snapshot, trashed_at, expires_at)
VALUES ('01J8Z6H0000000000000000011', '01J8Z6H0000000000000000008', '01J8Z6H0000000000000000001', 'worker-import', '/home/dev/.nodal/acme/e/01J8Z6H1', '/home/dev/.nodal/acme/trash/01J8Z6H1', NULL, 1788688960, 1789898560);
