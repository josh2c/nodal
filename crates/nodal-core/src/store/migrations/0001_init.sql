-- 0001 init: the registry tables, one per type in the domain model.
--
-- Every table is STRICT, so a column that should hold a ULID cannot quietly hold a
-- number: the shape rules the model enforces in Rust are not silently widened here.
-- Identifiers and validated scalars are TEXT in the exact form the model prints them.
-- Instants are INTEGER seconds since the Unix epoch, which sorts and compares without
-- a parser. `ports` and `refs` are JSON text, because they are maps the store never
-- queries into; every other value has its own column.
--
-- Deletes are RESTRICT everywhere. Which rows an operation may remove is a lifecycle
-- decision with an undo, never a side effect of removing a parent row.

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

-- One base per workspace fingerprint per platform: a base built elsewhere is not warm.
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

-- The rule that two units cannot work the same branch at once, as an index rather than
-- as a check in code: a branch is free again once its unit is no longer open.
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

-- The log is read per unit in identifier order, which is creation order.
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
