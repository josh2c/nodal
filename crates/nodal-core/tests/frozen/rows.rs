//! What a Nodal of each version wrote, as that version could write it.
//!
//! One entry per schema version that gave a person something new to hold. The entry is
//! applied straight after that version's migration and before the next one, so a
//! fixture is built the way a registry is lived in: rows arrive at the version that
//! could hold them, and a back-fill a later migration performs runs over rows that were
//! already there rather than over rows written afterwards.
//!
//! Every fixture therefore tells one story at a different age. One project, and five
//! bases of work in it: a unit somebody is working in; a unit that was reclaimed, whose
//! name a second unit had to take a suffix to avoid; a unit whose work is in review; a
//! unit whose work was merged; and a unit a `--force` reclaim gave up over work that was
//! only in its home. The columns the story is told in grow with the versions; the story
//! does not change, which is what lets one set of expectations be read against seventeen
//! files.
//!
//! The last three are here because only an old registry can be in the state they are in.
//! A unit in review and a merged unit both still hold their handle when migration 13
//! narrows the rule to the units that hold one: the rule it relaxes is for archived rows
//! alone, and until these two nothing crossed that index under any other status. The
//! forced reclaim is the row [`Rested::Forced`] is read from — a snapshot ref with no
//! verdict beside it, which is what a `--force` wrote before migration 15 gave it a column
//! to write in, and which `nodal gc` must not re-ask about. Neither state is one a Nodal
//! of today can write, so this is the only place either can be tested.
//!
//! [`Rested::Forced`]: nodal_core::model::Rested::Forced
//!
//! A version whose migration added no place to put anything — an index rule, a
//! back-fill — has no entry here. Its fixture is the version before it, carried across
//! its migration, which is exactly what a person's registry does.

/// The project every fixture is about.
pub const PROJECT: &str = "01J8Z6H0000000000000000001";
/// The unit somebody is working in.
pub const UNIT: &str = "01J8Z6H0000000000000000002";
/// The base its home was cloned from.
pub const BASE: &str = "01J8Z6H0000000000000000003";
/// The database template of that project.
pub const TEMPLATE: &str = "01J8Z6H0000000000000000004";
/// The unit's one materialisation.
pub const ENVIRONMENT: &str = "01J8Z6H0000000000000000005";
/// The session open in it.
pub const SESSION: &str = "01J8Z6H0000000000000000006";
/// The commit event of that unit, the first of one per kind.
pub const EVENT: &str = "01J8Z6H0000000000000000007";
/// The unit that was reclaimed, which keeps the name it was made under.
pub const RECLAIMED: &str = "01J8Z6H0000000000000000008";
/// The unit beside it, which the old handle rule pushed onto a suffix.
pub const SUFFIXED: &str = "01J8Z6H0000000000000000009";
/// The reclaimed unit's home, which is the row in the trash.
pub const TRASHED: &str = "01J8Z6H0000000000000000011";
/// The run that made the unit.
pub const OPERATION: &str = "01J8Z6H0000000000000000012";
/// The verdict a reclaim wrote once there was a kind for it.
pub const VERDICT: &str = "01J8Z6H0000000000000000032";
/// The unit whose work is in review, which holds its handle throughout.
pub const IN_REVIEW: &str = "01J8Z6H0000000000000000041";
/// The unit whose work was merged, which holds its handle throughout.
pub const MERGED: &str = "01J8Z6H0000000000000000042";
/// The unit a `--force` reclaim gave up, over work that was only in its home.
pub const FORCED: &str = "01J8Z6H0000000000000000043";
/// The home of the unit in review.
pub const REVIEW_HOME: &str = "01J8Z6H0000000000000000044";
/// The home of the merged unit.
pub const MERGED_HOME: &str = "01J8Z6H0000000000000000045";
/// The forced unit's home, which is the second row in the trash.
pub const FORCED_HOME: &str = "01J8Z6H0000000000000000046";

/// The commit the unit's work is on, in the two places it is recorded.
pub const COMMIT: &str = "9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001";
/// The port the unit's home holds, in the allocation and in the home's own map.
pub const PORT: u16 = 20002;
/// What the prune dropped from the reclaimed home, in bytes.
pub const PRUNED: u64 = 12_884_901_888;
/// Where the forced reclaim committed the work it was about to move, inside the home it
/// moved. The one thing a row with no verdict beside it says about itself.
pub const SNAPSHOT: &str = "refs/nodal/01J8Z6H0000000000000000043/wip";

/// The rows for one version, applied right after that version's migration.
pub const ROWS: &[(u32, &str)] = &[
    (1, V1),
    (2, V2),
    (3, V3),
    (4, V4),
    (5, V5),
    (7, V7),
    (8, V8),
    (9, V9),
    (10, V10),
    (11, V11),
    (12, V12),
    (14, V14),
    (15, V15),
    (16, V16),
    (17, V17),
];

/// Version 1: the project, its substrate, two units, their homes, a session, the log.
///
/// The log holds one event of every kind version 1 had, so a fixture can say that a
/// kind crossed the upgrade rather than that events did.
const V1: &str = "
INSERT INTO project (id, root, name, recipe_hash, created_at)
VALUES ('01J8Z6H0000000000000000001', '/home/dev/acme', 'acme', '0f1e2d', 1788688800);

INSERT INTO base (id, project_id, ws_fingerprint, platform, commit_id, path, built_at, last_used)
VALUES ('01J8Z6H0000000000000000003', '01J8Z6H0000000000000000001', '7c1e9a2b',
        'x86_64-unknown-linux-gnu', '9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001',
        '/home/dev/.nodal/acme/b/7c1e9a2b', 1788688810, 1788688820);

INSERT INTO db_template (id, project_id, schema_fingerprint, db_name, parent_template_id, built_at)
VALUES ('01J8Z6H0000000000000000004', '01J8Z6H0000000000000000001', '41bd', 'acme_t_41bd',
        NULL, 1788688830);

INSERT INTO unit (id, project_id, slug, objective, branch, parent_branch, status,
                  created_at, updated_at)
VALUES ('01J8Z6H0000000000000000002', '01J8Z6H0000000000000000001', 'fix-worker-import',
        'fix the worker import', 'nodal/fix-worker-import', 'main', 'open',
        1788688860, 1788688860),
       ('01J8Z6H0000000000000000008', '01J8Z6H0000000000000000001', 'worker-import',
        'import the workers', 'nodal/worker-import', 'main', 'archived',
        1788688870, 1788688880),
       ('01J8Z6H0000000000000000009', '01J8Z6H0000000000000000001', 'worker-import-2',
        'import the workers', 'nodal/worker-import', 'main', 'open',
        1788688890, 1788688890),
       ('01J8Z6H0000000000000000041', '01J8Z6H0000000000000000001', 'retry-the-probe',
        'retry the probe once', 'nodal/retry-the-probe', 'main', 'review',
        1788688700, 1788688710),
       ('01J8Z6H0000000000000000042', '01J8Z6H0000000000000000001', 'name-the-queue',
        'name the queue after what it carries', 'nodal/name-the-queue', 'main', 'merged',
        1788688600, 1788688610),
       ('01J8Z6H0000000000000000043', '01J8Z6H0000000000000000001', 'spike-the-pool',
        'spike a pool and throw it away', 'nodal/spike-the-pool', 'main', 'archived',
        1788688500, 1788688520);

INSERT INTO environment (id, unit_id, attempt, home, managed, base_id, ws_fp_materialized,
                         schema_fp_materialized, host, db_name, ports, fixed_port, state,
                         created_at, last_active)
VALUES ('01J8Z6H0000000000000000005', '01J8Z6H0000000000000000002', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H0', 1, '01J8Z6H0000000000000000003', '7c1e9a2b',
        '41bd', 'laptop', 'acme_e_01j8z6h0', '{\"app\":20002}', 5432, 'stopped',
        1788689040, 1788689040),
       ('01J8Z6H0000000000000000011', '01J8Z6H0000000000000000008', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H1', 1, NULL, NULL, NULL, 'laptop', NULL,
        '{}', NULL, 'absent', 1788688900, 1788688960),
       ('01J8Z6H0000000000000000044', '01J8Z6H0000000000000000041', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H4', 1, '01J8Z6H0000000000000000003', '7c1e9a2b',
        '41bd', 'laptop', NULL, '{}', NULL, 'stopped', 1788688710, 1788688720),
       ('01J8Z6H0000000000000000045', '01J8Z6H0000000000000000042', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H5', 1, '01J8Z6H0000000000000000003', '7c1e9a2b',
        '41bd', 'laptop', NULL, '{}', NULL, 'stopped', 1788688610, 1788688620),
       ('01J8Z6H0000000000000000046', '01J8Z6H0000000000000000043', 1,
        '/home/dev/.nodal/acme/e/01J8Z6H6', 1, NULL, NULL, NULL, 'laptop', NULL,
        '{}', NULL, 'absent', 1788688510, 1788688520);

INSERT INTO session (id, environment_id, actor_kind, actor_name, pid, started_at, ended_at)
VALUES ('01J8Z6H0000000000000000006', '01J8Z6H0000000000000000005', 'agent',
        'claude-code', 4242, 1788689100, NULL);

INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind,
                   epistemic, body, refs, raw_ref)
VALUES
 ('01J8Z6H0000000000000000007', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689160, 'human', 'dev', 'commit', 'observed', 'wired the worker import',
  '{\"commit\":\"9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001\"}', NULL),
 ('01J8Z6H0000000000000000020', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689110, 'agent', 'claude-code', 'attached', 'observed', 'the agent attached', '{}', NULL),
 ('01J8Z6H0000000000000000021', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689120, 'agent', 'claude-code', 'detached', 'observed', 'the agent detached', '{}', NULL),
 ('01J8Z6H0000000000000000022', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689130, 'agent', 'claude-code', 'command', 'observed', 'pnpm test', '{\"exit\":\"0\"}',
  '/home/dev/.nodal/acme/e/01J8Z6H0/.nodal/raw/01J8Z6H0000000000000000022.log'),
 ('01J8Z6H0000000000000000023', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689140, 'agent', 'claude-code', 'test_result', 'observed', '41 passed, 0 failed',
  '{\"passed\":\"41\"}', NULL),
 ('01J8Z6H0000000000000000024', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689150, 'agent', 'claude-code', 'failure', 'observed', 'the worker pool would not start',
  '{}', NULL),
 ('01J8Z6H0000000000000000025', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689170, 'agent', 'claude-code', 'file_touched', 'observed', 'src/worker/pool.ts',
  '{\"file\":\"src/worker/pool.ts\"}', NULL),
 ('01J8Z6H0000000000000000026', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689180, 'agent', 'claude-code', 'finding', 'stated', 'the import is circular', '{}', NULL),
 ('01J8Z6H0000000000000000027', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689190, 'human', 'dev', 'decision', 'stated', 'the pool is constructed by the caller',
  '{}', NULL),
 ('01J8Z6H0000000000000000028', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689200, 'agent', 'claude-code', 'question', 'stated', 'which module owns the pool?',
  '{}', NULL),
 ('01J8Z6H0000000000000000029', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689210, 'agent', 'claude-code', 'handoff', 'stated', 'the pool is wired, the tests are not',
  '{}', NULL),
 ('01J8Z6H000000000000000002A', '01J8Z6H0000000000000000002', '01J8Z6H0000000000000000005',
  1788689220, 'human', 'dev', 'sync', 'observed', 'the lockfile moved, dependencies reinstalled',
  '{}', NULL),
 ('01J8Z6H000000000000000002B', '01J8Z6H0000000000000000002', NULL,
  1788689230, 'human', 'dev', 'note', 'stated', 'ask the platform team about the pool', '{}', NULL);

INSERT INTO lease (resource, environment_id, expires_at)
VALUES ('port:5432', '01J8Z6H0000000000000000005', 1788692700);

INSERT INTO lock (unit_id, host, expires_at)
VALUES ('01J8Z6H0000000000000000002', 'laptop', 1788692700);
";

/// Version 2: the run that made the unit, and the steps it got through.
const V2: &str = "
INSERT INTO operation (id, kind, subject, params, recovery, state, host, pid, started_at,
                       ended_at)
VALUES ('01J8Z6H0000000000000000012', 'new', 'fix-worker-import',
        '{\"slug\":\"fix-worker-import\"}', 'roll_back', 'committed', 'laptop', 4100,
        1788688840, 1788688860);

INSERT INTO operation_step (operation_id, position, key, state, updated_at)
VALUES ('01J8Z6H0000000000000000012', 0, 'branch', 'applied', 1788688845),
       ('01J8Z6H0000000000000000012', 1, 'materialize', 'applied', 1788688855);
";

/// Version 3: the project's block of ports, and the one the home holds out of it.
const V3: &str = "
INSERT INTO port_block (project_id, first, last)
VALUES ('01J8Z6H0000000000000000001', 20000, 20009);

INSERT INTO port_allocation (port, project_id, environment_id, name)
VALUES (20002, '01J8Z6H0000000000000000001', '01J8Z6H0000000000000000005', 'app');
";

/// Version 4: the two reclaimed homes, in the trash and findable again.
///
/// The second was given up by a `--force` over work that existed nowhere else, so the
/// reclaim committed that work inside the home and wrote the ref in `snapshot`. Nothing
/// rewrites that row afterwards — no migration touches it and no Nodal reads it until the
/// retention runs out — so it is still what it was in every later fixture: the state
/// `Rested::Forced` is read from.
const V4: &str = "
INSERT INTO trash (environment_id, unit_id, project_id, slug, home, path, snapshot,
                   trashed_at, expires_at)
VALUES ('01J8Z6H0000000000000000011', '01J8Z6H0000000000000000008',
        '01J8Z6H0000000000000000001', 'worker-import',
        '/home/dev/.nodal/acme/e/01J8Z6H1', '/home/dev/.nodal/acme/trash/01J8Z6H1',
        NULL, 1788688960, 1789898560),
       ('01J8Z6H0000000000000000046', '01J8Z6H0000000000000000043',
        '01J8Z6H0000000000000000001', 'spike-the-pool',
        '/home/dev/.nodal/acme/e/01J8Z6H6', '/home/dev/.nodal/acme/trash/01J8Z6H6',
        'refs/nodal/01J8Z6H0000000000000000043/wip', 1788688520, 1789898120);
";

/// Version 5: the process group the session's tethered command was given.
const V5: &str = "
UPDATE session SET pgid = 4240 WHERE id = '01J8Z6H0000000000000000006';
";

/// Version 7: what the step that materialised the home told the registry write.
const V7: &str = "
UPDATE operation_step SET output = '{\"ports\":{\"app\":20002}}'
WHERE operation_id = '01J8Z6H0000000000000000012' AND position = 1;
";

/// Version 8: what the prune dropped from the reclaimed home on its way in.
const V8: &str = "
UPDATE trash SET pruned_bytes = 12884901888
WHERE environment_id = '01J8Z6H0000000000000000011';
";

/// Version 9: which repository the project is, apart from where it is checked out.
const V9: &str = "
UPDATE project SET remote_url = 'github.com/acme/acme'
WHERE id = '01J8Z6H0000000000000000001';
";

/// Version 10: the commit the unit forked from, recorded rather than re-derived.
const V10: &str = "
UPDATE unit SET base_commit = '9a3f1c2d4e5b6a7980c1d2e3f4a5b6c7d8e9f001'
WHERE id = '01J8Z6H0000000000000000002';
";

/// Version 11: who holds the write on the unit, and the process that took it.
const V11: &str = "
UPDATE lock SET actor_kind = 'agent', actor_name = 'claude-code', pid = 4242,
                taken_at = 1788689100, refreshed_at = 1788689240
WHERE unit_id = '01J8Z6H0000000000000000002';
";

/// Version 12: what built the base, and with what.
const V12: &str = "
UPDATE base SET provenance = '{\"nodal_version\":\"0.1.0-rc.1\",\"install\":[[\"pnpm\",\"install\",\
\"--frozen-lockfile\"]],\"warm\":[\"pnpm\",\"build\"],\"tools\":{\"pnpm\":\"9.12.3\"},\
\"recipe\":\"0f1e2d\"}'
WHERE id = '01J8Z6H0000000000000000003';
";

/// Version 14: the POSIX session the hold was taken from.
const V14: &str = "
UPDATE lock SET session = 4200 WHERE unit_id = '01J8Z6H0000000000000000002';
";

/// Version 15: what the reclaim's uniqueness check decided, and what it rested on.
const V15: &str = "
UPDATE trash SET rested = '{\"kind\":\"safe\",\"copies\":[{\"repository\":\"/home/dev/acme\",\
\"references\":[\"refs/remotes/origin/nodal/worker-import\"],\"commits\":3}]}'
WHERE environment_id = '01J8Z6H0000000000000000011';
";

/// Version 16: the verdict that reclaim wrote, in the kind that arrived for it.
const V16: &str = "
INSERT INTO event (id, unit_id, environment_id, ts, actor_kind, actor_name, kind,
                   epistemic, body, refs, raw_ref)
VALUES ('01J8Z6H0000000000000000032', '01J8Z6H0000000000000000008', NULL, 1788688950,
        'human', 'dev', 'verdict', 'observed',
        'every commit of the home is on refs/remotes/origin/nodal/worker-import',
        '{\"repository\":\"/home/dev/acme\"}', NULL);
";

/// Version 17: the instant the holder's process began, beside its identifier.
const V17: &str = "
UPDATE lock SET pid_started_at = 1788689090 WHERE unit_id = '01J8Z6H0000000000000000002';
";
