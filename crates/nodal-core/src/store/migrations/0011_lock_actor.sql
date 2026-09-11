-- 0011 lock actor: who holds the write on a unit, not only which host.
--
-- The lock table named a host and nothing else. One host is where two engineers both
-- log in, so a host is not a writer: two people in one home saw no sign of each other,
-- and the process table cannot tell them apart across Linux accounts either. The actor
-- is the answer, because it is the same answer sessions and events already record.
--
-- Five columns. `actor_kind` and `actor_name` say who holds it, in the spelling
-- `session` and `event` use. `pid` is the process that took it, recorded so a person
-- can look; nothing signals it and nothing expires a lock because it is gone. `taken_at`
-- is when the hold began and `refreshed_at` when it was last entered, which is the
-- stamp the idle window is measured from.
--
-- Null for the two actor columns, and null for every row written before this migration.
-- A row with no actor names a host and holds no actor: it is the record of a claim made
-- before there were actors, no product code ever wrote one, and the next entry into that
-- home rewrites it with an actor and says nothing. It is not deleted here, because a
-- migration describes rows and does not remove them.
--
-- `taken_at` and `refreshed_at` are not null and default to zero, which is the earliest
-- instant the model holds. An existing row therefore reads as idle since the epoch, so
-- the first read expires it rather than inventing a hold that never happened.
--
-- `expires_at` stays. It is the absolute lapse the transfer bundle carries. The idle
-- window is computed from `refreshed_at` and the recipe, and a lock lapses when either
-- one says so.

ALTER TABLE lock ADD COLUMN actor_kind TEXT;

ALTER TABLE lock ADD COLUMN actor_name TEXT;

ALTER TABLE lock ADD COLUMN pid INTEGER;

ALTER TABLE lock ADD COLUMN taken_at INTEGER NOT NULL DEFAULT 0;

ALTER TABLE lock ADD COLUMN refreshed_at INTEGER NOT NULL DEFAULT 0;
