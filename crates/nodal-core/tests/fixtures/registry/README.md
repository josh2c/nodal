# Frozen registry fixtures

One file per registry schema version: `vNN.sql` holds the schema version `NN` produced,
the rows a Nodal of that version held, and the `user_version` it stamped.

These files are **data**. Nothing regenerates them. A fixture rebuilt from today's
migrations moves whenever they move, so it agrees with the code by construction and says
nothing about the upgrade a person's registry crosses. `crates/nodal-core/tests/upgrade.rs`
opens every one of them with today's binary, migrates it to the current version, and holds
every row to the meaning it has in the file.

## What they prove, and what they do not

The seventeen files here were written in one pass, from the migrations as `main` carried
them at commit `eb360f9`. They are therefore not evidence that migration 5 was ever
shipped in the form the repository now states. They are the frozen record from this point
on: an edit to any migration that has already shipped now fails
`a_fixture_holds_the_schema_its_version_produced`, and a fixture stamped at a version
other than its name fails `a_fixture_records_the_version_it_starts_from`.

## Adding a migration

1. Append the migration and raise `SCHEMA_VERSION`.
2. Write what the new version can hold into `crates/nodal-core/tests/frozen/rows.rs`, as
   an entry for that version. A migration that added no place to put anything — an index,
   a back-fill — needs no entry.
3. Freeze the fixture:

   ```
   cargo test -p nodal-core --test upgrade -- --ignored the_frozen_fixtures
   ```

4. Read the file it wrote, then commit it. Do not edit it again.
