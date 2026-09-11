-- 0010 unit base commit: the commit a unit forked from, recorded rather than re-derived.
--
-- Where a unit started was worked out on every read, as the merge base of its branch and
-- whatever ref the survey chose to call the base. That answer is right until somebody
-- rebases the branch, and after a rebase it names the commit the branch was moved onto.
-- The two are different facts and only one of them is a fact about the unit.
--
-- So the create writes it down. It is the commit the person's own checkout was at when
-- the unit was made, or the commit `--from` named. Nothing updates it afterwards: a unit
-- forks once.
--
-- Null, not a default. A row without one is a unit created before this column existed,
-- and it means "not recorded", never "forked from nothing" and never a commit this
-- migration invented. There is no back-fill, in code or in SQL, because the value cannot
-- be recovered: the merge base of a branch that has since been rebased is not the commit
-- the unit started at, and writing it here would turn a missing fact into a wrong one.
-- Every reader treats null as unknown and falls back to what it did before.

ALTER TABLE unit ADD COLUMN base_commit TEXT;
