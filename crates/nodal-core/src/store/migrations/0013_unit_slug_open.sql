-- 0013 unit handle: a handle is unique among the units that hold one.
--
-- A handle was unique over every row of the table, archived rows included. So a unit
-- that was reclaimed went on owning its name, and a person who made the unit again got
-- `<name>-2` on the branch the archived unit already had. Two units then worked
-- `nodal/<name>`.
--
-- The rule a person states is about the units that hold a handle, not about every unit
-- that ever held one. This index states that rule, the way `unit_open_branch` already
-- states the branch rule: a partial index, decided by the database rather than by a
-- check in code.
--
-- Nothing is rewritten and nothing is lost. An archived row keeps the name a person
-- typed, its identifier, its branch, its objective and its place in the log. What it
-- stops doing is holding a claim on a name another unit may want.
--
-- The index is dropped and made again, because SQLite cannot add a WHERE clause to an
-- index that is already there. No row is read or written by either statement, so a
-- database with a hundred units crosses this as fast as an empty one.

DROP INDEX unit_slug;

CREATE UNIQUE INDEX unit_slug ON unit (project_id, slug) WHERE status <> 'archived';
