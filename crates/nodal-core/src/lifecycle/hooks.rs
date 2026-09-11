//! Recipe hooks: the commands a project runs around a lifecycle operation, and the
//! approval that has to exist before one of them runs.
//!
//! A recipe declares up to four commands ([`crate::model::Hooks`]). They run in a
//! directory that exists, with the `NODAL_*` context of the unit they are about
//! (`docs/contracts.md`), and each one runs at the moment its name says:
//!
//! | phase          | when                                | where             |
//! |----------------|-------------------------------------|-------------------|
//! | `pre_new`      | before a unit's home is made        | the project root  |
//! | `post_new`     | after its rows are committed        | the unit's home   |
//! | `pre_merge`    | before a merge commits anything     | the unit's home   |
//! | `post_merge`   | after the target is fast-forwarded  | the project root  |
//! | `pre_reclaim`  | before anything is torn down        | the unit's home   |
//! | `post_reclaim` | after the home is in the trash      | the project root  |
//!
//! Three of them run in the project root because the home is not the subject at that
//! moment: before a create there is nothing yet, after a reclaim there is nothing left
//! where the home was, and a merge ends by moving the project's own branch.
//! `NODAL_ROOT` still names the home in every case, so a hook that wants the path has
//! it.
//!
//! Every command may also name the values of [`template`](super::template): a hook is
//! written once for every unit, so the branch, the two directories, a stable port and a
//! sanitised name are substituted into the text before the shell sees it.
//!
//! # Why hooks are not steps
//!
//! Every other part of an operation is a [`crate::lifecycle::Step`] with an undo. A
//! hook has no undo: it is somebody else's program and Nodal does not know what it did,
//! let alone how to take it back. Journalling it as a step would claim otherwise, and
//! the runner would call an undo that silently does nothing while reporting that the
//! operation was rolled back. So hooks bracket the plan instead of being inside it, and
//! a run killed after `pre_reclaim` reports a rolled-back reclaim whose hook did in fact
//! run — which is true, and is what the alternative would have hidden.
//!
//! # Approval
//!
//! A hook is a command line in a file that arrives with a `git pull`. Running one
//! because it is written down is how a repository becomes a way to run code on the
//! machines of everybody who clones it. So a command runs only if this machine has
//! approved that exact text: `nodal init` approves the set a project declares, each
//! command pinned by the digest of its text, and a command that has changed or that
//! nobody has seen is refused with the text it would have run.
//!
//! The record is per person, in `~/.config/nodal/hooks.toml`, beside the secrets
//! file and for the same reason: approving somebody else's command is a decision the
//! person at this machine made, and it does not travel with the project.
//!
//! # Every path a hook is given is resolved
//!
//! A hook is told about two directories, four times over: `NODAL_SOURCE` and
//! `NODAL_ROOT` in its environment, `{repo_root}` and `{unit_path}` in its text, the
//! directory it is started in, and the directory the report says it ran in. All six come
//! from the same two values, so all six are resolved once, here, on the way in
//! ([`guard::resolve`]).
//!
//! Resolving is not a nicety. A hook that compares a variable with its own `$PWD` is
//! comparing two answers from two sources: Nodal's, which is a registry row holding the
//! path a person typed, and the shell's, which comes from `getcwd` and has every link
//! taken out of it. On macOS those are different text for one directory, because `/var`
//! there is a link to `/private/var` and every temporary directory is under it. A hook
//! that tests `[ "$NODAL_ROOT" = "$PWD" ]` would be false on one host and true on the
//! other, which is not a difference a project should have to know about.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::fingerprint;
use crate::lifecycle::guard;
use crate::lifecycle::template::Variables;
use crate::model::{BranchName, CommandLine, Digest, EnvId, Hooks, Slug, UnitId};
use crate::{Error, Result};

/// The file a person records their own approvals in.
pub const FILE_NAME: &str = "hooks.toml";

/// The variable that moves that file, for tests and for a second profile.
pub const PATH_VAR: &str = "NODAL_HOOKS_FILE";

/// The program a command line is handed to. A hook is written as a shell line — a
/// pipe, a `&&`, a variable — so it is given to a shell rather than split here into
/// something that would only look like the line the person wrote.
const SHELL: &str = "sh";

/// Which hook this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Before a unit is created.
    PreNew,
    /// After a unit is created.
    PostNew,
    /// Before a unit is merged.
    PreMerge,
    /// After a unit is merged, and before it is removed.
    PostMerge,
    /// Before a unit is reclaimed.
    PreReclaim,
    /// After a unit is reclaimed.
    PostReclaim,
}

/// Every phase, in the order they are declared and approved.
pub const PHASES: &[Phase] = &[
    Phase::PreNew,
    Phase::PostNew,
    Phase::PreMerge,
    Phase::PostMerge,
    Phase::PreReclaim,
    Phase::PostReclaim,
];

impl Phase {
    /// The key this phase has in `nodal.toml` and in the approvals file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PreNew => "pre_new",
            Self::PostNew => "post_new",
            Self::PreMerge => "pre_merge",
            Self::PostMerge => "post_merge",
            Self::PreReclaim => "pre_reclaim",
            Self::PostReclaim => "post_reclaim",
        }
    }

    /// The command a recipe declares for this phase, when it declares one.
    #[must_use]
    pub fn command(self, hooks: &Hooks) -> Option<&CommandLine> {
        match self {
            Self::PreNew => hooks.pre_new.as_ref(),
            Self::PostNew => hooks.post_new.as_ref(),
            Self::PreMerge => hooks.pre_merge.as_ref(),
            Self::PostMerge => hooks.post_merge.as_ref(),
            Self::PreReclaim => hooks.pre_reclaim.as_ref(),
            Self::PostReclaim => hooks.post_reclaim.as_ref(),
        }
    }
}

impl core::fmt::Display for Phase {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.key())
    }
}

/// The `NODAL_*` variables a hook is given.
///
/// `NODAL_HOME` is not among them, and that is deliberate: it is the variable that
/// moves Nodal's whole state directory, so a hook that runs `nodal` would
/// write into the unit's home instead of the registry. The home is `NODAL_ROOT`, which
/// is what an activated shell already carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// The project's own checkout.
    pub source: PathBuf,
    /// The unit's home, wherever it is at this moment.
    pub root: PathBuf,
    /// The unit.
    pub unit: UnitId,
    /// Its handle.
    pub slug: Slug,
    /// The branch the unit owns, which a command may name as `{branch}`.
    pub branch: BranchName,
    /// The tree the home was cloned from, when there was one. An adopted checkout has
    /// no parent and the variable is then empty.
    pub parent: Option<String>,
    /// The materialisation, so a hook that keeps its own state can key it the way the
    /// registry does.
    pub environment: EnvId,
}

impl Context {
    /// The same context with both of its directories resolved.
    ///
    /// The two paths are the only fields that name a place on a disk, and they are what
    /// every other form of the value is built from. Resolving them here is therefore the
    /// whole of the rule: the environment, the template values and the working directory
    /// all read from these.
    ///
    /// A home that is not there yet — `pre_new` runs before one is made — resolves as
    /// far as it exists, which is what [`guard::resolve`] answers.
    #[must_use]
    pub fn resolved(&self) -> Self {
        Self {
            source: guard::resolve(&self.source),
            root: guard::resolve(&self.root),
            ..self.clone()
        }
    }

    /// The variables, by name, in the order the contract lists them.
    #[must_use]
    pub fn vars(&self) -> Vec<(&'static str, String)> {
        vec![
            ("NODAL_SOURCE", self.source.to_string_lossy().into_owned()),
            ("NODAL_ROOT", self.root.to_string_lossy().into_owned()),
            ("NODAL_ID", self.unit.to_string()),
            ("NODAL_UNIT", self.slug.to_string()),
            ("NODAL_ENV", self.environment.to_string()),
            ("NODAL_PARENT_ID", self.parent.clone().unwrap_or_default()),
        ]
    }
}

/// What one hook did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ran {
    /// Which hook it was.
    pub phase: Phase,
    /// The command line, as the recipe writes it.
    pub command: String,
    /// The command line the shell was given, with every variable filled in.
    pub ran: String,
    /// The directory it ran in.
    pub directory: PathBuf,
}

/// What this machine has approved, by project root and phase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Approvals {
    /// Project root, then phase key, then the digest of the approved command.
    projects: BTreeMap<String, BTreeMap<String, String>>,
}

/// Where this person's approvals file is.
///
/// Three answers, in the order [`crate::env::secrets::MachineSecrets::path_in`] gives
/// them: [`PATH_VAR`] when it is set, then `~/.config/nodal/hooks.toml` when it is
/// there, then `<state dir>/hooks.toml` when *that* is there. A machine with neither
/// gets the second, which is where the first approval is written.
///
/// It is the person's own file and not the machine's, for the same reason the secrets
/// file is ([`crate::workspace::shared`]). An approval says that **this person** read a
/// command and accepts it running on their account. On a host whose state root a group
/// owns, an approvals file in that root would let one engineer's reading of a hook
/// decide that it runs under another engineer's account.
#[must_use]
pub fn path_in(state_dir: &Path) -> PathBuf {
    if let Some(named) = std::env::var_os(PATH_VAR).filter(|value| !value.is_empty()) {
        return PathBuf::from(named);
    }
    let Ok(own) = crate::workspace::home::config().map(|config| config.join(FILE_NAME)) else {
        return state_dir.join(FILE_NAME);
    };
    let legacy = state_dir.join(FILE_NAME);
    if !own.exists() && legacy.exists() { legacy } else { own }
}

impl Approvals {
    /// Read the file. One that is not there approves nothing, which is the right
    /// starting point: a machine that has never approved a hook runs none.
    ///
    /// # Errors
    /// [`Error::Io`] when the file exists and cannot be read, [`Error::Recipe`] when it
    /// is not the document this writes.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(Error::io(&path)(error)),
        };
        toml::from_str(&text).map_err(|source| Error::Recipe { path, source: Box::new(source) })
    }

    /// Approve every command a project declares now, forgetting whatever it declared
    /// before. Answers how many commands are approved for that project.
    ///
    /// Forgetting matters as much as approving: a hook a project has removed must not
    /// stay approved, or re-adding the old line later would run without being asked
    /// about.
    ///
    /// # Errors
    /// [`Error::InvalidValue`] if a command could not be digested.
    pub fn approve(&mut self, project: &Path, hooks: &Hooks) -> Result<usize> {
        let mut approved = BTreeMap::new();
        for phase in PHASES {
            if let Some(command) = phase.command(hooks) {
                approved.insert(phase.key().to_owned(), digest(command.as_str())?);
            }
        }
        let count = approved.len();
        let key = key_of(project);
        if approved.is_empty() {
            self.projects.remove(&key);
        } else {
            self.projects.insert(key, approved);
        }
        Ok(count)
    }

    /// Whether this exact command is approved for this project's phase.
    ///
    /// # Errors
    /// [`Error::InvalidValue`] if the command could not be digested.
    pub fn allows(&self, project: &Path, phase: Phase, command: &str) -> Result<bool> {
        let Some(approved) = self.projects.get(&key_of(project)).and_then(|p| p.get(phase.key()))
        else {
            return Ok(false);
        };
        Ok(approved == &digest(command)?)
    }

    /// Write the file, creating the state directory if it is not there.
    ///
    /// # Errors
    /// [`Error::Render`] when the document could not be encoded, [`Error::Io`] when it
    /// could not be written.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|_| Error::StoreEncode { kind: "hook approvals" })?;
        std::fs::write(path, text).map_err(Error::io(path))?;
        own_it(path)
    }
}

/// Make a file the owner's alone, whatever umask this process is running under.
///
/// The approvals are a record of what one person accepts running on their account. A
/// shared host runs Nodal under a umask that keeps the group's write bit
/// ([`crate::workspace::shared::UMASK`]), and a group-writable approvals file would let
/// another account add a command to the list this one acts on.
#[cfg(unix)]
fn own_it(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(OWNER_ONLY))
        .map_err(Error::io(path))
}

#[cfg(not(unix))]
fn own_it(_path: &Path) -> Result<()> {
    Ok(())
}

/// The mode the approvals file is kept at: the owner reads and writes it, nobody else.
pub const OWNER_ONLY: u32 = 0o600;

/// Approve, on this machine, exactly the hooks a project declares now.
///
/// This is what `nodal init` calls. Approval is a set rather than a command at a time,
/// because a person reading a recipe reads all of it at once; and it is re-made from
/// scratch each time, so a hook the project has since removed stops being approved.
///
/// Answers how many commands are now approved for the project.
///
/// # Errors
/// [`Error::Io`] when the file could not be read or written, [`Error::Recipe`] when the
/// file that is there is not one of these.
pub fn approve(state_dir: &Path, project: &Path, hooks: &Hooks) -> Result<usize> {
    let path = path_in(state_dir);
    let mut approvals = Approvals::open(&path)?;
    let count = approvals.approve(project, hooks)?;
    approvals.save(&path)?;
    Ok(count)
}

/// A project's hooks, the approvals for them, and whether they run at all.
///
/// One value rather than four arguments at every call: an operation makes this once,
/// from the recipe it has already loaded, and then asks it for one phase at a time.
#[derive(Debug, Clone)]
pub struct Runner {
    /// The project whose recipe declares them, which is what an approval is keyed by.
    pub project: PathBuf,
    /// What it declares.
    pub hooks: Hooks,
    /// What this machine has approved.
    pub approvals: Approvals,
    /// `false` for an invocation that carried `--no-hooks`, which runs none of them.
    pub enabled: bool,
}

impl Runner {
    /// Run one phase's hook in `directory`, if there is one to run.
    ///
    /// `Ok(None)` means there was nothing to run: no command declared, or hooks are off
    /// for this invocation.
    ///
    /// Every path the hook is told about is resolved first, so that what Nodal says a
    /// directory is called and what the shell says it is called are one name.
    ///
    /// # Errors
    /// [`Error::HookNotApproved`] when the command is not the approved one,
    /// [`Error::HookVariable`] when a value a variable would fill in could be read as
    /// shell syntax, [`Error::HookFailed`] when it exited non-zero, and
    /// [`Error::ToolSpawn`] when the shell could not be started.
    pub fn run(&self, phase: Phase, directory: &Path, context: &Context) -> Result<Option<Ran>> {
        if !self.enabled {
            return Ok(None);
        }
        let Some(command) = phase.command(&self.hooks) else { return Ok(None) };
        let command = command.as_str();
        if !self.approvals.allows(&self.project, phase, command)? {
            return Err(Error::HookNotApproved {
                phase,
                project: self.project.clone(),
                command: command.to_owned(),
            });
        }
        let context = context.resolved();
        let directory = guard::resolve(directory);
        let filled = Variables::of(&context)?.expand(phase, command)?;
        execute(phase, &filled, &directory, &context)?;
        Ok(Some(Ran { phase, command: command.to_owned(), ran: filled, directory }))
    }
}

/// Start the shell, wait for it, and turn a non-zero exit into the error that names
/// which hook it was. The one place this module spawns a process.
fn execute(phase: Phase, command: &str, directory: &Path, context: &Context) -> Result<()> {
    let mut shell = Command::new(SHELL);
    shell.arg("-c").arg(command).current_dir(directory);
    for (name, value) in context.vars() {
        shell.env(name, value);
    }
    let output =
        shell.output().map_err(|source| Error::ToolSpawn { program: SHELL.to_owned(), source })?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::HookFailed {
        phase,
        command: command.to_owned(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_owned(),
    })
}

/// The digest an approval records, as text.
fn digest(command: &str) -> Result<String> {
    fingerprint::compute_command(command).map(|value: Digest| value.to_string())
}

/// How a project is named in the file: its path, resolved, as text.
///
/// Resolved because the two sides write it differently and have to agree. `nodal init`
/// is given the directory a person typed, which is usually `.`; a reclaim is given the
/// project row's root, which is what `git rev-parse --show-toplevel` said. Both name one
/// directory, and an approval keyed by the text as typed would be an approval the other
/// side never finds.
///
/// Through [`guard::resolve`], which is the one place a path is normalised, rather than
/// a rule of this file's own. A project since deleted resolves as far as it exists,
/// which is still a key that matches itself.
fn key_of(project: &Path) -> String {
    guard::resolve(project).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use tempfile::TempDir;

    use super::{Approvals, PHASES, Phase};
    use crate::model::{CommandLine, Hooks};

    /// A context with both directories somewhere that does not matter.
    fn sample() -> super::Context {
        super::Context {
            source: "/p".into(),
            root: "/h".into(),
            unit: "01J8Z6H0000000000000000001".parse().unwrap(),
            slug: crate::model::Slug::parse("worker-import").unwrap(),
            branch: crate::model::BranchName::parse("nodal/worker-import").unwrap(),
            parent: None,
            environment: "01J8Z6H0000000000000000002".parse().unwrap(),
        }
    }

    fn hooks(pre: &str) -> Hooks {
        Hooks { pre_reclaim: Some(CommandLine::parse(pre).unwrap()), ..Hooks::default() }
    }

    #[test]
    fn every_phase_has_its_own_key_and_reads_its_own_command() {
        let keys: Vec<&str> = PHASES.iter().map(|phase| phase.key()).collect();
        assert_eq!(
            keys,
            ["pre_new", "post_new", "pre_merge", "post_merge", "pre_reclaim", "post_reclaim"]
        );
        let declared = hooks("echo one");
        assert_eq!(Phase::PreReclaim.command(&declared).unwrap().as_str(), "echo one");
        assert!(Phase::PreNew.command(&declared).is_none());
    }

    #[test]
    fn nothing_is_approved_until_a_project_approves_it() {
        let approvals = Approvals::default();
        assert!(!approvals.allows(Path::new("/p"), Phase::PreReclaim, "echo one").unwrap());
    }

    #[test]
    fn a_command_that_changed_is_no_longer_the_approved_one() {
        let mut approvals = Approvals::default();
        assert_eq!(approvals.approve(Path::new("/p"), &hooks("echo one")).unwrap(), 1);
        assert!(approvals.allows(Path::new("/p"), Phase::PreReclaim, "echo one").unwrap());
        assert!(!approvals.allows(Path::new("/p"), Phase::PreReclaim, "echo two").unwrap());
        assert!(!approvals.allows(Path::new("/q"), Phase::PreReclaim, "echo one").unwrap());
    }

    #[test]
    fn approving_again_forgets_the_hooks_a_project_has_dropped() {
        let mut approvals = Approvals::default();
        approvals.approve(Path::new("/p"), &hooks("echo one")).unwrap();
        assert_eq!(approvals.approve(Path::new("/p"), &Hooks::default()).unwrap(), 0);
        assert!(!approvals.allows(Path::new("/p"), Phase::PreReclaim, "echo one").unwrap());
    }

    #[test]
    fn a_resolved_context_names_one_directory_the_way_the_filesystem_does() {
        let root = TempDir::new().unwrap();
        let real = root.path().join("real");
        let link = root.path().join("by-another-name");
        std::fs::create_dir_all(real.join("home")).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let context = super::Context { source: link.clone(), root: link.join("home"), ..sample() };
        let resolved = context.resolved();
        assert_eq!(resolved.source, std::fs::canonicalize(&real).unwrap());
        assert_eq!(resolved.root, std::fs::canonicalize(real.join("home")).unwrap());
        // And the values a hook is given all come from those two.
        let vars = resolved.vars();
        let named = |name: &str| {
            vars.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone()).unwrap()
        };
        assert_eq!(named("NODAL_ROOT"), resolved.root.to_string_lossy());
        assert_eq!(named("NODAL_SOURCE"), resolved.source.to_string_lossy());
    }

    #[test]
    fn a_home_that_is_not_there_yet_resolves_as_far_as_it_exists() {
        let root = TempDir::new().unwrap();
        let real = root.path().join("real");
        let link = root.path().join("by-another-name");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // `pre_new` is told about a home nothing has made yet.
        let context =
            super::Context { source: link.clone(), root: link.join("unmade"), ..sample() };
        let resolved = context.resolved();
        assert_eq!(resolved.root, std::fs::canonicalize(&real).unwrap().join("unmade"));
    }

    #[test]
    fn the_context_never_sets_the_variable_that_moves_the_state_directory() {
        let context = sample();
        let names: Vec<&str> = context.vars().iter().map(|(name, _)| *name).collect();
        assert!(!names.contains(&crate::workspace::home::DIRECTORY_VAR), "{names:?}");
        assert!(names.contains(&"NODAL_ROOT") && names.contains(&"NODAL_SOURCE"));
    }
}
