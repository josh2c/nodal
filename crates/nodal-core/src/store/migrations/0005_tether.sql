-- 0005 tether: the process group `nodal run --tether` gave a command.
--
-- A tethered command runs in a process group of its own, and the group belongs to the
-- unit rather than to the `nodal run` that started it. This column is that record. A
-- session row already names one attachment of an actor to an environment, opens when
-- the attachment starts and closes when it ends, and is already what a reclaim gives
-- up; a tether is the same shape with one more identifier, so it is the same table.
--
-- The column is null for every session a process scan derived. Only a tether has a
-- group, because only a tether was put in one on purpose.
--
-- Group zero addresses the group the signalling process is in, and group one is the
-- system's own. Neither is a group Nodal may ever record, so neither may be written.

ALTER TABLE session ADD COLUMN pgid INTEGER CHECK (pgid IS NULL OR pgid > 1);

-- The read a reclaim makes: which groups of this environment are still to be stopped.
CREATE INDEX session_tether ON session (environment_id)
    WHERE pgid IS NOT NULL AND ended_at IS NULL;
