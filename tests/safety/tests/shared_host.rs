//! One registry for two accounts on one host, and secrets that stay each person's own.
//!
//! Nodal's claim is one list of every unit. On a box two engineers log in to, one list
//! means one registry file both accounts may write, and one project for the two clones
//! of the repository they are both working on. Neither of those may cost a person their
//! credentials, and neither may change anything for the person who has the box to
//! themselves.
//!
//! Four properties, and each one is a way the arrangement could go wrong:
//!
//! | property | what a break would look like |
//! |---|---|
//! | a group-writable registry | the second account cannot open the list, or the first's registry is world-writable |
//! | a private host untouched | one person's own machine quietly starts sharing its state |
//! | one project per repository | two clones of one remote get two bases, two port blocks and two lists |
//! | secrets stay their owner's | the second account entering a home reads the first's credentials |
//!
//! ## The second account, without a second account
//!
//! Switching uid needs privileges CI does not have, so the second person here is a
//! second `HOME` with a secrets file of their own. That is exactly the seam the property
//! is about: which file a value is resolved from is decided by `HOME`, not by a uid, so
//! a test that moves `HOME` is testing the thing that decides. What it cannot show is
//! the kernel refusing a read, and [`GROUP_CLAIM`] says so out loud.

#![allow(clippy::unwrap_used, reason = "a test fails by panicking")]

use std::path::{Path, PathBuf};

use nodal_core::workspace::shared;

use nodal_core::store::{Store, projects};
use nodal_safety::machine::binary;
use nodal_safety::project::Workspace;
use nodal_safety::state::InState;
use nodal_safety::{git, git_ok, stderr, stdout};

/// The claim this suite does not make, because making it needs privileges CI has not
/// got: that the kernel refuses one uid the other's file. What is asserted instead is
/// the mode and the group the kernel would decide it from.
const GROUP_CLAIM: &str = "a second uid is refused a file it may not read; asserted here as the \
                           mode and group the kernel decides that from";

/// A recipe that declares one credential, so that a home has a name a person supplies.
const RECIPE: &str = "[env]\nsecrets = [\"SESSION_SECRET\"]\n";

/// A recipe that declares one hook, so that a create needs an approval to get past it.
const HOOKED: &str = "[hooks]\npre_new = \"true\"\n";

/// The value each fake account keeps in its own secrets file.
const FIRST: &str = "first-accounts-value";
const SECOND: &str = "second-accounts-value";

/// The permission bits of a path.
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::metadata(path)
        .unwrap_or_else(|_| panic!("{} is there", path.display()))
        .permissions()
        .mode()
        & 0o7777
}

/// The group a path belongs to.
fn group_of(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt as _;

    std::fs::metadata(path).unwrap_or_else(|_| panic!("{} is there", path.display())).gid()
}

/// Hand a directory to the group it already belongs to, with the setgid bit set.
///
/// The group is the one the runner is already in, because a test may not add itself to
/// another. What makes the root shared is the setgid bit, and that is what is set here.
fn make_shared(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::create_dir_all(root).unwrap();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o2775)).unwrap();
}

/// A directory standing in for one person's own home directory, with `value` in the
/// secrets file that lives under it.
fn account(root: &Path, name: &str, value: &str) -> PathBuf {
    let home = root.join(name);
    let config = home.join(".config").join("nodal");
    std::fs::create_dir_all(&config).unwrap();
    nodal_safety::activation::write_secrets(
        &config.join("secrets.env"),
        &format!("SESSION_SECRET={value}\n"),
    );
    home
}

/// The state root and the registry in it, made the way every command makes them.
///
/// `nodal ls` on a machine with no registry reads nothing and writes nothing, which is
/// a property of its own (`tests/doctor_writes_nothing.rs`), so a test that needs a
/// registry to look at opens one. It is the same call every command makes.
fn registry_of(workspace: &Workspace) -> PathBuf {
    drop(Store::open(workspace.registry()).unwrap());
    workspace.registry()
}

/// What `nodal env --export` prints for the account whose home directory is `home`.
///
/// The secrets-file override every command in this kit carries is removed, because the
/// default is the thing being tested: with nothing naming a file, the value has to come
/// from under this account's own home directory.
fn export_as(workspace: &Workspace, home: &Path, unit_home: &Path) -> String {
    let output = workspace
        .command(&["env", "--export", unit_home.to_str().unwrap()])
        .env_remove("NODAL_SECRETS_FILE")
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(output.status.success(), "nodal env --export: {}", stderr(&output));
    stdout(&output)
}

/// A registry in a group-owned state root is one the group can write.
///
/// The modes are read while a connection is open, because SQLite removes the two
/// sidecar files when the last connection closes cleanly. A group member who may write
/// the database and not its write-ahead log cannot commit anything.
#[test]
fn a_group_owned_state_root_gets_a_registry_the_group_can_write() {
    println!("NOT ASSERTED: {GROUP_CLAIM}");
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    make_shared(&state);

    let workspace = Workspace::new(binary());
    let made = workspace
        .command(&["ls"])
        .env(nodal_core::workspace::home::DIRECTORY_VAR, &state)
        .output()
        .unwrap();
    assert!(made.status.success(), "nodal ls: {}", stderr(&made));

    let registry = state.join("registry.db");
    let store = Store::open(&registry).unwrap();
    assert_eq!(store.path(), registry.as_path());
    let sidecars = shared::SIDECARS.map(|suffix| shared::sidecar(&registry, suffix));
    for path in std::iter::once(registry.clone()).chain(sidecars) {
        assert_eq!(
            mode(&path),
            0o660,
            "{} is not writable by the group that owns the state root",
            path.display()
        );
        assert_eq!(group_of(&path), group_of(&state), "{} left the group", path.display());
    }
}

/// A state root that is one person's own gains nothing from any of this.
///
/// The registry of somebody with the machine to themselves must not become readable by
/// a group because Nodal learned how to share one. The setgid bit is the whole switch,
/// and a root without it is left as it was.
#[test]
fn a_state_root_of_one_person_is_left_exactly_as_it_was() {
    let workspace = Workspace::new(binary());
    let registry = registry_of(&workspace);
    assert_eq!(mode(&registry) & 0o022, 0, "a private registry became writable by others");
    assert_eq!(mode(workspace.state_dir()) & 0o2000, 0, "a private state root became setgid");
}

/// Two clones of one repository are one project: one base, one block of ports, one list.
///
/// This is what a host two engineers share is for. Each of them clones the repository
/// into a directory of their own, and Nodal has to see the work as one project — while
/// still handing the two units ports that do not collide.
#[test]
fn two_clones_of_one_remote_are_one_project_with_distinct_ports() {
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin.git");
    let seed = root.path().join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("package.json"), "{\"name\":\"demo\"}\n").unwrap();
    git::init(&seed, "main");
    git_ok(&seed, &["add", "-A"]);
    git_ok(&seed, &["commit", "-qm", "first"]);
    git_ok(
        root.path(),
        &["clone", "--quiet", "--bare", seed.to_str().unwrap(), origin.to_str().unwrap()],
    );

    let state = root.path().join("state");
    make_shared(&state);

    let mut homes = Vec::new();
    for (index, who) in ["ada", "bo"].iter().enumerate() {
        let checkout = root.path().join(who);
        git_ok(
            root.path(),
            &["clone", "--quiet", origin.to_str().unwrap(), checkout.to_str().unwrap()],
        );
        git::identity(&checkout);
        let made = std::process::Command::new(binary())
            .args(["new", "--name", &format!("work-{index}")])
            .current_dir(&checkout)
            .env(nodal_core::workspace::home::DIRECTORY_VAR, &state)
            .env("NODAL_SECRETS_FILE", root.path().join("secrets.env"))
            .env("NODAL_HOOKS_FILE", root.path().join("hooks.toml"))
            .env_remove("NODAL_CD_FILE")
            .env("CLAUDE_CONFIG_DIR", root.path().join("claude"))
            .output()
            .unwrap();
        assert!(made.status.success(), "nodal new in {who}: {}", stderr(&made));
        homes.push(checkout);
    }

    let store = Store::open(state.join("registry.db")).unwrap();
    let rows = projects::list(store.conn()).unwrap();
    assert_eq!(rows.len(), 1, "two clones of one remote made {} projects", rows.len());
    assert!(rows[0].remote_url.is_some(), "the project row records no remote");

    let units = nodal_core::store::units::list(store.conn(), rows[0].id).unwrap();
    assert_eq!(units.len(), 2, "one project, two units: {units:?}");
    let ports: Vec<_> = units
        .iter()
        .flat_map(|unit| {
            nodal_core::store::environments::list_for_unit(store.conn(), unit.id).unwrap()
        })
        .map(|environment| environment.ports)
        .collect();
    assert_eq!(ports.len(), 2);
    assert_ne!(ports[0], ports[1], "two units of one project were granted one port block");

    // One list, read from either clone. This is the promise the whole arrangement is
    // for: neither engineer has to stand in the other's directory to see the work.
    for checkout in &homes {
        let listed = std::process::Command::new(binary())
            .arg("ls")
            .current_dir(checkout)
            .env(nodal_core::workspace::home::DIRECTORY_VAR, &state)
            .env("NODAL_SECRETS_FILE", root.path().join("secrets.env"))
            .env("NODAL_HOOKS_FILE", root.path().join("hooks.toml"))
            .env_remove("NODAL_CD_FILE")
            .env("CLAUDE_CONFIG_DIR", root.path().join("claude"))
            .output()
            .unwrap();
        assert!(listed.status.success(), "nodal ls: {}", stderr(&listed));
        let said = stdout(&listed);
        for slug in ["work-0", "work-1"] {
            assert!(said.contains(slug), "{} does not list {slug}: {said}", checkout.display());
        }
    }
}

/// A registry written before projects had remotes is given them at the upgrade.
///
/// This is the half of migration 9 that SQL cannot do: which repository a checkout is a
/// clone of is a question for Git. The registry here is put back to the schema it had
/// before the migration, column and all, so the next command really does cross the
/// version and really does run the step.
#[test]
fn a_project_row_with_no_remote_is_given_one_from_its_own_checkout() {
    let workspace = Workspace::new(binary());
    let origin = workspace.root().join("origin.git");
    git_ok(
        workspace.root(),
        &[
            "clone",
            "--quiet",
            "--bare",
            workspace.source.to_str().unwrap(),
            origin.to_str().unwrap(),
        ],
    );
    git_ok(&workspace.source, &["remote", "add", "origin", origin.to_str().unwrap()]);
    drop(stdout(&workspace.nodal(&["new", "--name", "first"])));

    {
        let store = workspace.store();
        store
            .conn()
            // Every column added after version 8 goes, not only the one this test is
            // about: the version is what decides which migrations replay, and a schema
            // that already carries a later column would fail the step that adds it.
            .execute_batch(
                "DROP INDEX project_remote_url;\n\
                 ALTER TABLE project DROP COLUMN remote_url;\n\
                 ALTER TABLE unit DROP COLUMN base_commit;\n\
                 ALTER TABLE lock DROP COLUMN actor_kind;\n\
                 ALTER TABLE lock DROP COLUMN actor_name;\n\
                 ALTER TABLE lock DROP COLUMN pid;\n\
                 ALTER TABLE lock DROP COLUMN taken_at;\n\
                 ALTER TABLE lock DROP COLUMN refreshed_at;\n\
                 PRAGMA user_version = 8;",
            )
            .unwrap();
    }
    drop(stdout(&workspace.nodal(&["ls"])));

    let store = workspace.store();
    let rows = projects::list(store.conn()).unwrap();
    assert_eq!(rows.len(), 1);
    let expected = origin.to_str().unwrap().trim_end_matches(".git");
    assert_eq!(
        rows[0].remote_url.as_ref().map(ToString::to_string).as_deref(),
        Some(expected),
        "the row was not given the remote its own checkout names"
    );
}

/// A home holds no credential, and two accounts entering it get their own.
///
/// A unit home under a shared state root is a directory both accounts can enter. If the
/// value were written into the home, whoever entered second would read whoever created
/// it. So the file holds the unit and not the person, and the value is resolved on the
/// way in from the secrets file under that person's own home directory.
#[test]
fn a_second_account_entering_a_home_reads_its_own_secrets_and_not_the_first_s() {
    println!("NOT ASSERTED: {GROUP_CLAIM}");
    let workspace = Workspace::with_recipe(binary(), RECIPE);
    let ada = account(workspace.root(), "ada", FIRST);
    let bo = account(workspace.root(), "bo", SECOND);

    let made = workspace
        .command(&["new", "--name", "shared-work"])
        .env_remove("NODAL_SECRETS_FILE")
        .env("HOME", &ada)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(made.status.success(), "nodal new: {}", stderr(&made));
    let home = workspace.one_home();

    let written = std::fs::read_to_string(home.join(".nodal/env")).unwrap();
    assert!(!written.contains(FIRST), "the home carries the first account's value: {written}");
    assert!(!written.contains(SECOND), "the home carries the second account's value: {written}");
    assert!(written.contains("NODAL_UNIT"), "the home does not say which unit it is");

    let first = export_as(&workspace, &ada, &home);
    assert!(first.contains(FIRST), "the first account did not get its own value: {first}");
    assert!(!first.contains(SECOND), "the first account read the second's value: {first}");

    let second = export_as(&workspace, &bo, &home);
    assert!(second.contains(SECOND), "the second account did not get its own value: {second}");
    assert!(!second.contains(FIRST), "the second account read the first's value: {second}");
}

/// A person's secrets file is under their own directory and never in the shared root.
///
/// One registry for a host is the point of shared mode; one secrets file for a host is
/// the opposite of it. A third account with no file of its own gets no value and no
/// failure, and nothing is written into the state root either way.
#[test]
fn the_secrets_file_lives_under_the_persons_own_directory() {
    let workspace = Workspace::with_recipe(binary(), RECIPE);
    let ada = account(workspace.root(), "ada", FIRST);
    let nobody = workspace.root().join("nobody");
    std::fs::create_dir_all(&nobody).unwrap();

    let made = workspace
        .command(&["new", "--name", "shared-work"])
        .env_remove("NODAL_SECRETS_FILE")
        .env("HOME", &ada)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(made.status.success(), "nodal new: {}", stderr(&made));
    let home = workspace.one_home();

    // The value is read from under this account's own home directory. Nothing names a
    // file, so where it was read from is the whole claim.
    let mine = export_as(&workspace, &ada, &home);
    assert!(mine.contains(FIRST), "the value under the account's own home was not read: {mine}");

    let bare = export_as(&workspace, &nobody, &home);
    assert!(!bare.contains(FIRST), "an account with no file read another's value: {bare}");
    assert!(bare.contains("NODAL_UNIT"), "an account with no file lost the unit: {bare}");

    assert!(
        !workspace.state_dir().join("secrets.env").exists(),
        "a secrets file was written into the state root"
    );
    assert_eq!(mode(&ada.join(".config").join("nodal").join("secrets.env")) & 0o077, 0);
}

/// One person's approval of a hook does not approve it for another's account.
///
/// An approval says that this person read a command line and accepts it running under
/// their own account. The record used to sit in the state root, and a state root a group
/// owns would have made one engineer's reading decide what runs as another. So the file
/// is under each person's own home directory, beside their secrets.
#[test]
fn an_approval_by_one_account_does_not_approve_a_hook_for_another() {
    let workspace = Workspace::new(binary());
    let ada = account(workspace.root(), "ada", FIRST);
    let bo = account(workspace.root(), "bo", SECOND);
    workspace.write_recipe(HOOKED);

    let approved = workspace
        .command(&["init", "--force"])
        .env_remove("NODAL_HOOKS_FILE")
        .env_remove("NODAL_SECRETS_FILE")
        .env("HOME", &ada)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(approved.status.success(), "nodal init: {}", stderr(&approved));
    assert!(
        ada.join(".config").join("nodal").join("hooks.toml").is_file(),
        "the approval was not written under the account that made it"
    );
    assert!(
        !workspace.state_dir().join("hooks.toml").exists(),
        "an approval was written into the state root"
    );
    assert_eq!(
        mode(&ada.join(".config").join("nodal").join("hooks.toml")) & 0o077,
        0,
        "another account may write the list of commands this one accepts"
    );

    let refused = workspace
        .command(&["new", "--name", "not-yours"])
        .env_remove("NODAL_HOOKS_FILE")
        .env_remove("NODAL_SECRETS_FILE")
        .env("HOME", &bo)
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert!(!refused.status.success(), "a hook the second account never approved ran");
    let said = stderr(&refused);
    assert!(said.contains("not approved"), "the refusal does not say why: {said}");
}

/// A group nobody has is refused, and the message names it.
///
/// An error without its reason sends a person to look at the wrong thing. A state root
/// handed to a group that does not exist would be one nobody but its owner could reach,
/// which is the arrangement the flag exists to end, so nothing is changed at all.
#[test]
fn a_group_this_host_has_not_got_is_refused_and_named() {
    let workspace = Workspace::new(binary());
    drop(registry_of(&workspace));
    let refused = workspace.nodal(&["init", "--shared", "no-such-group-here"]);
    assert!(!refused.status.success(), "an unknown group was accepted");
    let said = stderr(&refused);
    assert!(said.contains("no-such-group-here"), "the message does not name the group: {said}");
    assert_eq!(mode(workspace.state_dir()) & 0o2000, 0, "the state root became setgid anyway");
}

/// `nodal init --shared` makes the root a group's, and says what it did.
#[test]
fn init_shared_hands_the_state_root_to_a_group_and_says_so() {
    let workspace = Workspace::new(binary());
    drop(registry_of(&workspace));
    let group = group_of(workspace.state_dir());

    let done = workspace.nodal(&["init", "--force", "--shared", &group.to_string()]);
    assert!(done.status.success(), "nodal init --shared: {}", stderr(&done));
    let said = stderr(&done);
    assert!(said.contains(&group.to_string()), "the command did not say what it did: {said}");
    assert_eq!(mode(workspace.state_dir()), 0o2775);
    assert_eq!(mode(&workspace.registry()), 0o660);
}
