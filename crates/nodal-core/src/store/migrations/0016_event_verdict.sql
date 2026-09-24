-- 0016 the verdict event: what a destructive operation decided, and on what.
--
-- A reclaim's verdict was computed, rendered and dropped. Nothing in the registry said
-- what the last reclaim of a unit decided, and nothing said what it decided on, so a
-- home that turned out to be wanted could be argued about afterwards and never checked.
-- `verdict` is that record, written by an executed reclaim and by nothing else: a
-- `nodal reclaim --check` is a reading and writes nothing at all.
--
-- The kind is a CHECK constraint rather than a lookup table, and SQLite cannot alter a
-- constraint, so the table is made again beside the old one and the rows are carried
-- across. Every column keeps its name, its type and its meaning; the only change is one
-- more permitted word. The identifier is the primary key and it sorts by time, so the
-- copy preserves the log's order exactly.
--
-- Foreign keys point *into* this table from nothing, so nothing has to be re-pointed.
-- The index is made again because dropping the table takes it.

CREATE TABLE event_new (
    id             TEXT    PRIMARY KEY,
    unit_id        TEXT    NOT NULL REFERENCES unit (id) ON DELETE RESTRICT,
    environment_id TEXT             REFERENCES environment (id) ON DELETE RESTRICT,
    ts             INTEGER NOT NULL,
    actor_kind     TEXT    NOT NULL CHECK (actor_kind IN ('human', 'agent')),
    actor_name     TEXT    NOT NULL,
    kind           TEXT    NOT NULL CHECK (kind IN (
        'attached', 'detached', 'command', 'commit', 'test_result', 'failure',
        'file_touched', 'finding', 'decision', 'question', 'handoff', 'sync', 'note',
        'verdict'
    )),
    epistemic      TEXT    NOT NULL CHECK (epistemic IN ('observed', 'stated')),
    body           TEXT    NOT NULL,
    refs           TEXT    NOT NULL,
    raw_ref        TEXT
) STRICT;

INSERT INTO event_new
    SELECT id, unit_id, environment_id, ts, actor_kind, actor_name, kind, epistemic,
           body, refs, raw_ref
    FROM event;

DROP TABLE event;

ALTER TABLE event_new RENAME TO event;

CREATE INDEX event_unit ON event (unit_id, id);
