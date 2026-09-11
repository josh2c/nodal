-- 0009 project remote: which repository a project is, apart from where it is checked out.
--
-- A project was keyed by the path of one checkout. That is the right key for one person
-- on one machine and the wrong one for a host two people log in to: each engineer clones
-- the same repository into their own directory, and Nodal saw two projects. Two projects
-- means two bases built from the same commits, two blocks of ports, and two lists where
-- the tool promises one.
--
-- The remote is what the two clones have in common, so it is what the row records. It is
-- the normalised form of `origin`, not the URL as typed: one engineer clones over SSH and
-- the other over HTTPS, and neither of them should have to type the other's spelling.
--
-- Null, not a default, and null for every row written before this migration. A project
-- with no remote is an ordinary thing — a repository that has never been pushed — and its
-- identity stays the checkout path it has always been. The back-fill is in code, because
-- reading a checkout's origin is a question for Git and not for SQLite.
--
-- The index is partial for the same reason: a null remote is not an identity, and several
-- rows may carry one.

ALTER TABLE project ADD COLUMN remote_url TEXT;

CREATE UNIQUE INDEX project_remote_url ON project (remote_url) WHERE remote_url IS NOT NULL;
