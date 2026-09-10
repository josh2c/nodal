-- 0008 trash pruned: how much of a reclaimed home was build output and dependencies.
--
-- A reclaim moves a home to the trash and the trash keeps it for a fortnight. Most of
-- what it keeps is not work: a built Rust unit measured thirteen gigabytes, of which
-- under one was the tree a person could not get back. Reclaim now removes the build
-- output and the installed dependencies from the copy on its way in, and this column is
-- what it dropped.
--
-- The figure is on the row rather than in the journal because the journal is about one
-- run of one operation and this is a fact about a directory that outlives it. `nodal
-- doctor` reads the trash without reading any operation, and a person asking why the
-- trash is the size it is needs the answer from the same row that says where the home
-- went.
--
-- Zero, not null, and zero for every row written before this migration: a home that
-- held nothing to prune and a home that was trashed before Nodal pruned anything are
-- the same claim about what the directory holds now.

ALTER TABLE trash ADD COLUMN pruned_bytes INTEGER NOT NULL DEFAULT 0;
