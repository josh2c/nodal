-- 0003 ports: the block a project hands ports out from, and the ports held in it.
--
-- Two tables, because the two rows answer different questions. `port_block` is a
-- property of a project and never changes once written: every unit of a project takes
-- its ports from the same range, so a person reading a port number can tell which
-- project it belongs to. `port_allocation` is one port one environment holds.
--
-- `port_allocation.port` is the primary key and the whole of the no-double-grant rule.
-- Two processes creating units at the same moment both try to write the same row, and
-- SQLite decides between them; neither reads first and then writes, so there is no
-- window between the read and the write for the other one to fit into. A port is a
-- host-wide resource, so the key is the port alone rather than the port within a
-- project.
--
-- A port a project pins is not here. That is a lease (`lease`, resource `port:<number>`)
-- because a pinned port is held by one environment at a time and the claim expires, so
-- a crashed session cannot keep it forever. A port allocated from a block does not
-- expire: the environment holds it for as long as the environment exists, and reclaim
-- gives it back.

CREATE TABLE port_block (
    project_id TEXT    PRIMARY KEY REFERENCES project (id) ON DELETE RESTRICT,
    first      INTEGER NOT NULL CHECK (first BETWEEN 1 AND 65535),
    last       INTEGER NOT NULL CHECK (last BETWEEN first AND 65535)
) STRICT;

-- Blocks are equal in length and aligned, so two that start at the same port are the
-- same block: one unique index is the whole of the rule that blocks do not overlap.
CREATE UNIQUE INDEX port_block_first ON port_block (first);

CREATE TABLE port_allocation (
    port           INTEGER PRIMARY KEY CHECK (port BETWEEN 1 AND 65535),
    project_id     TEXT    NOT NULL REFERENCES project (id) ON DELETE RESTRICT,
    environment_id TEXT    NOT NULL REFERENCES environment (id) ON DELETE RESTRICT,
    name           TEXT    NOT NULL
) STRICT;

-- An environment holds one port per name, so asking for the same name twice returns the
-- port it already has instead of granting a second one.
CREATE UNIQUE INDEX port_allocation_name ON port_allocation (environment_id, name);

-- What reclaim reads: every port one environment holds.
CREATE INDEX port_allocation_environment ON port_allocation (environment_id);
