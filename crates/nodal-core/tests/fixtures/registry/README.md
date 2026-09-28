# Frozen registry fixtures

One file per registry schema version: `vNN.sql` holds the schema version `NN` produced,
the rows a Nodal of that version held, and the `user_version` it stamped.

These files are **data**. Nothing regenerates them. A fixture rebuilt from today's
migrations moves whenever they move, so it agrees with the code by construction and says
nothing about the upgrade a person's registry crosses. `crates/nodal-core/tests/upgrade.rs`
opens every one of them with today's binary, migrates it to the current version, holds
every value in the file to the value it had, and holds every row to the meaning it has.

## What they prove

The only upgrade a released Nodal performs is 14 to 17, and it is covered by fixtures built
from the migrations that shipped. Every release so far — `v0.1.0-rc.1`, `rc.2` and `rc.3` —
carried `SCHEMA_VERSION = 14`, and migrations `0001` to `0014` are byte-identical in all
three tags and on `main`:

```
git diff v0.1.0-rc.3 HEAD -- crates/nodal-core/src/store/migrations/
```

names only `0015`, `0016` and `0017`. So `v01.sql` through `v14.sql` are artifacts of
migration SQL a person's registry really crossed, and a registry from any released binary
starts at `v14.sql`.

The seventeen files were written in one pass, from the migrations as `main` carried them at
commit `eb360f9`, which is why the paragraph above is a comparison against the tags rather
than a claim about how the files were made. From this point on they are the frozen record:
an edit to any migration that has already shipped fails
`a_fixture_holds_the_schema_its_version_produced`, naming the table it changed; a fixture
stamped at a version other than its name fails
`a_fixture_records_the_version_it_starts_from`; a value that does not survive the upgrade
fails `every_value_of_every_fixture_survives_the_upgrade`, naming the row and the column;
and a migration whose fixture says nothing about what that migration added fails
`a_fixture_holds_a_value_in_every_place_its_migration_made`.

## Adding a migration

1. Append the migration and raise `SCHEMA_VERSION`.
2. Write what the new version can hold into `crates/nodal-core/tests/frozen/rows.rs`, as
   an entry for that version. A migration that added no place to put anything — an index,
   a widened constraint, a back-fill — needs no entry, and the suite asks it for nothing.
   For every other migration the suite asks for a value in each table it made and each
   column it added, so step 3 with no entry written fails rather than producing a fixture
   that says nothing about the migration.
3. Freeze the fixture:

   ```
   cargo test -p nodal-core --test upgrade -- --ignored the_frozen_fixtures
   ```

4. Read the file it wrote, then commit it. Do not edit it again.
