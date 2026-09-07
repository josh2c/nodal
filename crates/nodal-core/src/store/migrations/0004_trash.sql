-- 0004 trash: the homes reclaim has taken away but not yet destroyed, and the hook
-- commands this machine has approved.
--
-- Reclaiming a unit moves its home to the project's trash directory. The row here is
-- what makes that directory findable again: without it a person who wants the work
-- back has a directory named by eight characters of an identifier and nothing that
-- says which unit it was. One row per environment, because an environment is
-- materialised once and reclaimed once.
--
-- `expires_at` is written when the home is moved, not worked out when `gc` runs. The
-- retention a project asked for is a promise made at the moment the work was taken
-- away, and a recipe edited afterwards must not shorten a window somebody is relying
-- on. `snapshot` is the ref inside the trashed repository that a forced reclaim
-- committed the work to, and is null when there was nothing to preserve.

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

-- The read `nodal gc` makes: what may go now.
CREATE INDEX trash_expiry ON trash (expires_at);
