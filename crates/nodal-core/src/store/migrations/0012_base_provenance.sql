-- 0012 base provenance: what built a base, and with what.
--
-- A base recorded where it came from and when, and nothing about what made it. So a
-- base that behaved unlike a fresh one could not be compared with one: which release
-- built it, which installs it ran, which tools answered them and which recipe it read
-- were all gone the moment the build finished.
--
-- Five columns, every one null for a row written before this migration. A base with no
-- provenance is exactly what this record exists to make visible: it was built by a
-- Nodal that did not record any, and a person can see that rather than assume the row
-- is complete. Nothing is invented for such a row and nothing is rebuilt because of one.
--
-- `nodal_version` is the version of the binary that ran the build, and it is what says
-- whether provenance is recorded at all: the other four are read only where it is set.
-- `install_argv` is the argument lists of every package manager install, in the order
-- they ran, as a JSON array of arrays, so a repository of three ecosystems keeps all
-- three and keeps them apart. `warm_argv` is the build command, empty where none ran.
-- `tool_versions` is what each tool answered when it was asked, by the name the recipe
-- records it under. `recipe_digest` is the effective recipe by content, which is the
-- same digest `project.recipe_hash` holds.
--
-- None of this is part of a base's key. A base is keyed by its workspace fingerprint
-- and its platform, and it stays warm across a change to any column here.

ALTER TABLE base ADD COLUMN nodal_version TEXT;

ALTER TABLE base ADD COLUMN install_argv TEXT;

ALTER TABLE base ADD COLUMN warm_argv TEXT;

ALTER TABLE base ADD COLUMN tool_versions TEXT;

ALTER TABLE base ADD COLUMN recipe_digest TEXT;
