//! `nodal uninstall`: what it would take away, and taking it away.
//!
//! The command is two halves, and they are separate on purpose. [`survey`] reads the
//! machine and answers with a list of items; [`apply`] removes exactly the items in
//! that list. What a person agrees to is therefore the same value that is then acted
//! on, and a `--json` reading of the plan is the same document a person read.
//!
//! Four kinds of item, and the last is the only one that can lose work:
//!
//! - the block in a start-up file, per shell that has one ([`super::rc`]);
//! - the shell script in the state directory, per shell that has one ([`super::shims`]);
//! - the hooks in a `.claude/settings.json`: the person's own file, then one per project
//!   the registry knows, then one per unit home it made
//!   ([`crate::adapters::claude_code`]). Those live in somebody's
//!   repository rather than on their machine, so they are removed the way they were
//!   added: what Nodal wrote and nothing else. The homes are surveyed because a home
//!   carries a settings file of its own — it is what makes the session's observers fire
//!   — and a home left with one after an uninstall runs a provider hook that says
//!   `nodal: not on PATH` and ends a session over a tool the person removed. A home
//!   whose copy the project tracks is left alone: it is the project's file, and the
//!   project's own copy is surveyed in its own right;
//! - the state directory itself, which is asked for by `--state` and never removed
//!   without it.
//!
//! The state directory holds every unit home Nodal made. So the survey asks
//! [`crate::lifecycle::uniqueness`] about every managed home first, like every other
//! destructive path in Nodal, and a home that holds work which is only there stops the
//! uninstall. `--force` records what was accepted rather than hiding it.

use std::path::{Path, PathBuf};

use crate::adapters::{claude_code, settings};
use crate::lifecycle::uniqueness::{self, Uniqueness};
use crate::model::Timestamp;
use crate::output::view::setup::{Installed, Item, Kind, Uninstall};
use crate::runtime::shells::Shell;
use crate::setup::{rc, shims};
use crate::store::{Store, environments, projects, units};
use crate::{Error, Result, recipe};

/// What an uninstall was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Nodal's state directory.
    pub state: PathBuf,
    /// The person's own directory, which the start-up files are under.
    pub home: PathBuf,
    /// Whether the state directory goes as well.
    pub state_too: bool,
    /// Whether a home that holds work only it has may be removed anyway.
    pub force: bool,
    /// The project the command was run in, when it was run in one.
    ///
    /// The registry knows every project a unit was made for, and that is where the
    /// settings files come from. It does not know a project somebody ran `nodal init`
    /// in and nothing else, because `init` writes a file into a project Nodal may never
    /// have heard of and does not open the registry to do it. So the directory a person
    /// is standing in is asked as well, and an uninstall run in that project takes back
    /// what the init there put in.
    pub project: Option<PathBuf>,
}

/// The project directory `start` is in, found by the recipe at its root.
///
/// `None` when nothing above `start` holds a `nodal.toml`, which is the answer for a
/// directory that is not in a project at all.
#[must_use]
pub fn project_at(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|directory| directory.join(recipe::FILE_NAME).is_file())
        .map(Path::to_path_buf)
}

/// Install the integration for one shell: write the script, add the block.
///
/// Both halves are idempotent. The script is written again over whatever was there,
/// which is how an upgrade that moved the binary is picked up; the block is added only
/// when the start-up file has none, so running the install twice is one block.
///
/// The start-up file's parent directory is made when it is not there. fish keeps its
/// start-up file two directories down, and a person who has never opened fish has
/// neither.
///
/// # Errors
/// [`Error::Io`] when the script, the start-up file or its directory cannot be written.
pub fn install(state: &Path, home: &Path, shell: Shell, binary: &Path) -> Result<Installed> {
    let shim = shims::write(state, shell, binary)?;
    let rc = shell.rc_path(home);
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    let (before, created) = match std::fs::read_to_string(&rc) {
        Ok(text) => (text, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), true),
        Err(error) => return Err(Error::io(&rc)(error)),
    };
    let added = match rc::add(&before, shell, &shim, created) {
        Some(text) => {
            std::fs::write(&rc, text).map_err(Error::io(&rc))?;
            true
        }
        None => false,
    };
    Ok(Installed { shell: shell.name().to_owned(), shim, rc, added })
}

/// Read the machine and say what an uninstall would take away.
///
/// Nothing of the machine's is removed. A start-up file with no block is not an item, a
/// shell with no script is not an item, and a state directory that is not there is not
/// an item, so a second uninstall on a machine that has already had one has nothing to
/// do and says so.
///
/// # Errors
/// [`Error::Io`] when a start-up file exists and cannot be read.
pub fn survey(request: &Request) -> Result<Uninstall> {
    let mut items = Vec::new();
    let mut notes = Vec::new();
    for shell in shims::shells() {
        let file = shell.rc_path(&request.home);
        if let Some(only) = block_in(&file)? {
            let detail = if only {
                format!("the whole file; it holds the {} block and nothing else", shell.name())
            } else {
                format!("the {} block, and nothing else in the file", shell.name())
            };
            items.push(Item { kind: Kind::RcBlock, path: file, detail });
        }
    }
    for file in shims::installed(&request.state) {
        let detail =
            file.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        items.push(Item { kind: Kind::Shim, path: file, detail });
    }
    claude_items(request, &mut items, &mut notes);
    let findings =
        if request.state_too { state_item(request, &mut items, &mut notes) } else { Vec::new() };
    Ok(Uninstall {
        now: Timestamp::now(),
        items,
        findings,
        forced: request.force,
        applied: false,
        notes,
    })
}

/// Take away exactly what the plan lists.
///
/// The order is the order of the list: the start-up files first, then the scripts, then
/// the state directory. A person who stops the machine half way through has a shell
/// that loads nothing rather than a shell that loads a file which is no longer there.
///
/// # Errors
/// [`Error::Io`] when a file cannot be written or removed, and
/// [`Error::InvalidValue`] when the plan is one the survey refused.
pub fn apply(plan: &Uninstall) -> Result<Uninstall> {
    if !plan.is_permitted() {
        return Err(Error::InvalidValue {
            kind: "uninstall",
            value: uniqueness_message(&plan.findings),
        });
    }
    for item in &plan.items {
        match item.kind {
            Kind::RcBlock => strip_block(&item.path)?,
            Kind::Shim => remove_file(&item.path)?,
            Kind::ClaudeHooks => drop(claude_code::uninstall(&item.path)?),
            Kind::State => remove_tree(&item.path)?,
        }
    }
    tidy_shims(plan)?;
    Ok(Uninstall { applied: true, ..plan.clone() })
}

/// Remove the scripts directory once the last script has gone from it.
///
/// A directory a person put something else in is left exactly as it is
/// ([`shims::tidy`]), and a state directory that went whole has no directory left to
/// tidy.
fn tidy_shims(plan: &Uninstall) -> Result<()> {
    let Some(shim) = plan.items.iter().find(|item| item.kind == Kind::Shim) else {
        return Ok(());
    };
    let Some(state) = shim.path.parent().and_then(Path::parent) else { return Ok(()) };
    shims::tidy(state).map(|_| ())
}

/// Whether a start-up file holds the block, and whether the block is the whole of it.
///
/// `None` means the file holds no block, which is also the answer for a file that is
/// not there.
fn block_in(file: &Path) -> Result<Option<bool>> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::io(file)(error)),
    };
    let made = rc::made_the_file(&text);
    Ok(rc::remove(&text).map(|kept| made && kept.is_empty()))
}

/// Take the block out of a start-up file and leave every other byte of it alone.
///
/// A file the install made, and which holds nothing but the block, is removed rather
/// than left empty. That is the one case where writing an empty file back would leave a
/// machine different from the way the install found it. A file that was already there
/// keeps its own bytes, including none.
fn strip_block(file: &Path) -> Result<()> {
    let text = std::fs::read_to_string(file).map_err(Error::io(file))?;
    let made = rc::made_the_file(&text);
    let Some(kept) = rc::remove(&text) else { return Ok(()) };
    if made && kept.is_empty() {
        return remove_file(file);
    }
    std::fs::write(file, kept).map_err(Error::io(file))
}

/// Remove one file, and treat one that has already gone as done.
fn remove_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// Remove one directory and everything under it.
fn remove_tree(path: &Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

/// Add one item per project whose settings file holds hooks Nodal wrote.
///
/// A registry that is not there is not an error, and neither is one that cannot be
/// read: it is a note, because "no project has hooks" and "I could not look" are
/// different answers and a person deciding what to remove needs the right one.
fn claude_items(request: &Request, items: &mut Vec<Item>, notes: &mut Vec<String>) {
    match settings_files(request, notes) {
        Ok(found) => items.extend(found),
        Err(error) => notes
            .push(format!("the registry could not be read, so no project was checked: {error}")),
    }
}

/// Every settings file that holds Nodal's hooks, over every directory Nodal can name.
///
/// The person's own file, then the project roots, then the unit homes, and the three are
/// not treated alike.
///
/// The **person's own file** is where `nodal init --claude-hooks` writes by default. It
/// is under their own directory and no registry knows about it, so it is looked for by
/// path rather than found: [`settings::user_path`], which reads Claude Code's own
/// variable for a person who moved that directory. It is removed the way a project's is,
/// which is what leaves the file byte for byte the file it was.
///
/// In a **project**, `nodal init` wrote the hooks into the person's own file, committed
/// or not, and an uninstall takes back exactly what that install put in.
///
/// In a **home**, Nodal writes the file only where Git does not track it
/// ([`crate::env::files::WRITTEN`]). A home whose copy the project commits is the
/// project's file, arrived with the clone; editing or removing it would leave every
/// home of that project modified from birth, and the removal would ship in the pull
/// request the unit opens. The project's own copy is surveyed anyway, one directory up
/// the list, so nothing is missed by leaving the clones alone.
fn settings_files(request: &Request, notes: &mut Vec<String>) -> Result<Vec<Item>> {
    let mut found = Vec::new();
    found.extend(hooked(&settings::user_path(&request.home), notes));
    for root in project_roots(&request.state, request.project.as_deref())? {
        found.extend(hooked(&settings::path(&root), notes));
    }
    for (home, _) in homes(&request.state)? {
        if crate::env::files::tracked_of(&home).tracks(settings::FILE) {
            continue;
        }
        found.extend(hooked(&settings::path(&home), notes));
    }
    Ok(found)
}

/// The item for one settings file, when it holds hooks Nodal wrote and can be read.
///
/// A file that cannot be read is one note and no item. The survey visits every project
/// and every home on the machine, and one unreadable file among forty must not be the
/// end of the whole answer: a person then sees what can be removed, and one line saying
/// which file was not looked at.
fn hooked(path: &Path, notes: &mut Vec<String>) -> Option<Item> {
    let text = match claude_code::read(path) {
        Ok(text) => text,
        Err(error) => {
            notes.push(format!(
                "{} could not be read, so it was not checked: {error}",
                path.display()
            ));
            return None;
        }
    };
    settings::holds_hooks(&text).then(|| Item {
        kind: Kind::ClaudeHooks,
        path: path.to_path_buf(),
        detail: kept(&text),
    })
}

/// Every project root to look in: the registry's, and the one the person is in.
fn project_roots(state: &Path, here: Option<&Path>) -> Result<Vec<PathBuf>> {
    let registry = state.join("registry.db");
    let mut roots: Vec<PathBuf> = if registry.is_file() {
        let store = Store::open(&registry)?;
        projects::list(store.conn())?.into_iter().map(|project| project.root).collect()
    } else {
        Vec::new()
    };
    if let Some(root) = here
        && !roots.contains(&root.to_path_buf())
    {
        roots.push(root.to_path_buf());
    }
    Ok(roots)
}

/// What is left in a settings file once Nodal's hooks have gone.
fn kept(text: &str) -> String {
    let others = settings::other_events(text);
    if others.is_empty() {
        return String::from("the hooks nodal wrote, and nothing else in the file");
    }
    format!("the hooks nodal wrote; {} stays", others.join(crate::output::human::JOIN))
}

/// Add the state directory to the plan, and read every home it holds first.
fn state_item(
    request: &Request,
    items: &mut Vec<Item>,
    notes: &mut Vec<String>,
) -> Vec<Uniqueness> {
    if !request.state.is_dir() {
        return Vec::new();
    }
    let homes = match homes(&request.state) {
        Ok(found) => found,
        Err(error) => {
            notes.push(format!("the registry could not be read, so no home was checked: {error}"));
            Vec::new()
        }
    };
    let findings =
        homes.iter().filter_map(|(home, project)| unique_work(home, project.as_deref())).collect();
    items.push(Item {
        kind: Kind::State,
        path: request.state.clone(),
        detail: format!("the registry and {}", plural(homes.len(), "unit home", "unit homes")),
    });
    findings
}

/// Every managed home the registry knows about, with the project checkout to compare
/// it against.
///
/// A registry that is not there is not an error: a person may have installed the shell
/// integration and never made a unit.
fn homes(state: &Path) -> Result<Vec<(PathBuf, Option<PathBuf>)>> {
    let registry = state.join("registry.db");
    if !registry.is_file() {
        return Ok(Vec::new());
    }
    let store = Store::open(&registry)?;
    let conn = store.conn();
    let mut found = Vec::new();
    for environment in environments::list_all(conn)? {
        if !environment.managed || !environment.home.is_dir() {
            continue;
        }
        let root = units::get(conn, environment.unit_id)?
            .and_then(|unit| projects::get(conn, unit.project_id).ok().flatten())
            .map(|project| project.root);
        found.push((environment.home, root));
    }
    Ok(found)
}

/// What one home holds that exists nowhere else, or nothing when it holds nothing and
/// nothing when it could not be read.
///
/// A home Git cannot read is not a claim that it is safe. It is left out of the
/// findings because there is nothing to report about it; the state item still names how
/// many homes there are, and `--state` still asks before it removes any of them.
fn unique_work(home: &Path, project: Option<&Path>) -> Option<Uniqueness> {
    uniqueness::check(home, project).ok().filter(|answer| !answer.is_clear())
}

/// The message a refused uninstall carries.
fn uniqueness_message(findings: &[Uniqueness]) -> String {
    let named: Vec<String> = findings
        .iter()
        .map(|answer| {
            format!(
                "{}: {}",
                answer.home.display(),
                uniqueness::Finding::summarise(&answer.findings)
            )
        })
        .collect();
    format!(
        "{} holds work that exists nowhere else; pass --force to remove it anyway ({})",
        plural(findings.len(), "one home", "more than one home"),
        named.join("; ")
    )
}

/// A count with the word that goes with it, so a report never says `1 unit homes`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use tempfile::TempDir;

    use super::{Request, apply, survey};
    use crate::output::view::setup::Kind;
    use crate::runtime::shells::Shell;
    use crate::setup::shims;

    /// A machine with a start-up file and a state directory of its own.
    struct Machine {
        /// The person's own directory.
        home: TempDir,
        /// Nodal's state directory.
        state: TempDir,
    }

    impl Machine {
        fn new() -> Self {
            Self { home: TempDir::new().unwrap(), state: TempDir::new().unwrap() }
        }

        fn request(&self, state_too: bool) -> Request {
            Request {
                state: self.state.path().to_path_buf(),
                home: self.home.path().to_path_buf(),
                state_too,
                force: false,
                project: None,
            }
        }

        /// Put `before` in the start-up file and install the integration over it.
        fn install(&self, shell: Shell, before: &str) -> std::path::PathBuf {
            let file = shell.rc_path(self.home.path());
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, before).unwrap();
            let done =
                super::install(self.state.path(), self.home.path(), shell, Path::new("/opt/nodal"))
                    .unwrap();
            assert!(done.added, "the install did nothing");
            file
        }

        /// Install into a shell whose start-up file does not exist yet.
        fn install_into_no_file(&self, shell: Shell) -> std::path::PathBuf {
            let done =
                super::install(self.state.path(), self.home.path(), shell, Path::new("/opt/nodal"))
                    .unwrap();
            assert!(done.added, "the install did nothing");
            done.rc
        }
    }

    #[test]
    fn a_machine_with_nothing_installed_has_nothing_to_do() {
        let machine = Machine::new();
        let plan = survey(&machine.request(false)).unwrap();
        assert!(plan.is_empty(), "{plan:?}");
        assert!(plan.is_permitted());
    }

    #[test]
    fn every_installed_thing_is_one_item_a_person_can_read() {
        let machine = Machine::new();
        machine.install(Shell::Bash, "PS1='$ '\n");
        machine.install(Shell::Zsh, "");
        let plan = survey(&machine.request(true)).unwrap();
        let kinds: Vec<Kind> = plan.items.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            vec![Kind::RcBlock, Kind::RcBlock, Kind::Shim, Kind::Shim, Kind::State],
            "{plan:?}"
        );
        assert!(plan.items.iter().all(|item| !item.detail.is_empty()), "{plan:?}");
    }

    #[test]
    fn the_state_directory_goes_only_when_it_was_asked_for() {
        let machine = Machine::new();
        machine.install(Shell::Bash, "PS1='$ '\n");
        let plan = survey(&machine.request(false)).unwrap();
        assert!(plan.items.iter().all(|item| item.kind != Kind::State), "{plan:?}");
        assert!(machine.state.path().is_dir(), "and it is still there afterwards");
    }

    #[test]
    fn a_start_up_file_the_install_made_is_removed_rather_than_left_empty() {
        let machine = Machine::new();
        let file = machine.install_into_no_file(Shell::Bash);
        apply(&survey(&machine.request(false)).unwrap()).unwrap();
        assert!(!file.exists(), "an install that made the file left a nought-byte one behind");
    }

    #[test]
    fn an_empty_start_up_file_that_was_already_there_is_still_there_and_still_empty() {
        let machine = Machine::new();
        let file = machine.install(Shell::Bash, "");
        apply(&survey(&machine.request(false)).unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "", "a file of the person's went");
    }

    #[test]
    fn a_start_up_file_is_byte_identical_after_an_install_and_an_uninstall() {
        for before in ["PS1='$ '\n", "PS1='$ '", "", "# one\n\n# two\n"] {
            let machine = Machine::new();
            let file = machine.install(Shell::Bash, before);
            apply(&survey(&machine.request(false)).unwrap()).unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), before, "{before:?}");
        }
    }

    #[test]
    fn an_uninstall_that_ran_leaves_nothing_for_a_second_one() {
        let machine = Machine::new();
        machine.install(Shell::Fish, "set -g fish_greeting\n");
        let done = apply(&survey(&machine.request(false)).unwrap()).unwrap();
        assert!(done.applied);
        assert!(survey(&machine.request(false)).unwrap().is_empty());
        assert!(
            !shims::directory(machine.state.path()).exists(),
            "the scripts directory outlived the last script in it"
        );
    }

    #[test]
    fn removing_the_state_directory_takes_everything_under_it() {
        let machine = Machine::new();
        std::fs::write(machine.state.path().join("hooks.toml"), "").unwrap();
        machine.install(Shell::Bash, "PS1='$ '\n");
        let plan = survey(&machine.request(true)).unwrap();
        apply(&plan).unwrap();
        assert!(!machine.state.path().exists());
    }
}
