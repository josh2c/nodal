-- 0002 journal: what an operation leaves behind while it is running.
--
-- A lifecycle operation is a list of idempotent steps, and every step is written down
-- before it is attempted and again once it has been done. The point of writing it down
-- is the case where nothing gets to finish: a process killed between two steps leaves
-- an `operation` row still `running` and an `operation_step` row per step it reached,
-- and that is enough for the next `nodal` invocation to undo exactly the work that
-- happened and no more.
--
-- Two tables rather than one. Whether an operation is finished is a property of the
-- operation, not of any step: a run whose every step is `applied` but whose registry
-- write never committed is precisely the case that must be undone, and there is no
-- step row that says so. `operation` carries that, and `operation_step` carries the
-- work. The registry write and the move of `operation.state` to `committed` happen in
-- the same transaction, so an operation is either wholly done or wholly not.
--
-- `params` is the plan's own input, as JSON, because the process that resolves an
-- interrupted operation is not the process that started it: it has to rebuild the plan
-- from the record before it can undo a step. `host` and `pid` say whose run it was, so
-- a `nodal` on another machine, or beside a run that is still going, leaves it alone.

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

-- The read every command makes on startup: which operations never finished. A partial
-- index keeps that read proportional to the number of unfinished ones, not to the
-- number that ever ran.
CREATE INDEX operation_unfinished ON operation (id) WHERE state = 'running';

CREATE TABLE operation_step (
    operation_id TEXT    NOT NULL REFERENCES operation (id) ON DELETE RESTRICT,
    position     INTEGER NOT NULL CHECK (position >= 0),
    key          TEXT    NOT NULL,
    state        TEXT    NOT NULL CHECK (state IN ('applying', 'applied', 'undone')),
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (operation_id, position)
) STRICT;
