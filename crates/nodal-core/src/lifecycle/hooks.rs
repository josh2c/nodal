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
//! # No process a hook starts becomes invisible
//!
//! A hook's shell is started in a process group of its own, and what that group holds
//! once the shell has exited decides what happens next ([`execute`]). A hook that leaves
//! nothing running behaves exactly as it always has. A hook that deliberately backgrounds
//! work leaves a group that is still running, and that group is recorded in the registry
//! as the unit's ([`Registered`]) before the lifecycle command reports success — the same
//! row, read by the same two commands, that `nodal run --tether` writes.
//!
//! The alternative was a process nothing could name. A hook's descendant may run under
//! `env -i` and change directory, so neither the `NODAL_*` variables nor the working
//! directory attributes it, and on a host with no readable process table
//! ([`crate::runtime::processes`]) neither signal exists in the first place. A recorded
//! group is the one handle that answers on every host, because `kill` answers for a group
//! everywhere.
//!
//! So a group that cannot be recorded is stopped, and the operation says why. That is the
//! case for `pre_new`, which runs before the unit has any rows for a group to belong to,
//! and for a hook that failed, and for a storage failure after the shell has already
//! started. The obligation is the same in all three: leave no survivor that nothing owns.
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
use std::process::{Child, Command, Stdio};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::fingerprint;
use crate::lifecycle::guard;
use crate::lifecycle::template::Variables;
use crate::model::{
    Actor, ActorName, BranchName, CommandLine, Digest, EnvId, Hooks, Session, SessionId, Slug,
    Timestamp, UnitId,
};
use crate::runtime::stop::{self, Signals as _, Target};
use crate::store::sessions;
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
    pub fn run(
        &self,
        phase: Phase,
        directory: &Path,
        context: &Context,
        owner: &dyn Ownership,
    ) -> Result<Option<Ran>> {
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
        execute(phase, &filled, &directory, &context, owner)?;
        Ok(Some(Ran { phase, command: command.to_owned(), ran: filled, directory }))
    }
}

/// Where the process group a hook left running is written down.
///
/// A hook is somebody else's program, and the shell Nodal starts for it is put in a
/// process group of its own so that whatever it starts is attributable afterwards. Most
/// hooks leave nothing: the shell exits, the group empties, and there is nothing to
/// record. A hook that deliberately backgrounds work leaves a group that is still
/// running, and that group is the unit's — it was started by the unit's recipe, in the
/// unit's home, with the unit's environment.
///
/// This is the seam that says where such a group is recorded. It has two
/// implementations, and which one a phase gets is what decides whether a surviving group
/// may live: [`Registered`] writes the row every later `nodal reclaim` and `nodal gc`
/// read, and [`Unattachable`] is the answer for a phase that has no row to write into.
///
/// # Why a failure here stops the group
///
/// The alternative is a process nothing on the machine can name: not by variable, since
/// a hook's child may wipe its environment; not by directory, since it may change it;
/// and not by row, since there is none. That is the one outcome this whole seam exists
/// to prevent, so an attachment that cannot be made ends with the group stopped and the
/// reason reported, rather than with a lifecycle command claiming a clean operation
/// around a process it has lost.
pub trait Ownership {
    /// Record `pgid` as this unit's group to stop, durably enough that a later command
    /// finds it.
    ///
    /// # Errors
    /// Whatever the storage reported, and [`Error::HookGroupUnattachable`] from a phase
    /// that has nowhere to put the row.
    fn attach(&self, phase: Phase, context: &Context, pgid: u32) -> Result<()>;
}

/// The registry, which is where a surviving group belongs.
///
/// The row is the same shape a tether's is ([`crate::model::Session`]): the unit's
/// materialisation, an open row, and the process group in `pgid`. That is deliberate
/// and it is the whole of the mechanism. `nodal reclaim` already stops every open group
/// of the environment it is ending, and `nodal gc` already stops every open group of an
/// environment that has been reclaimed, on both supported hosts and without a process
/// table. A second ownership mechanism for a group whose producer happened to be a hook
/// would be a second thing to keep those two commands in step with.
///
/// The actor names the phase, so a person reading `nodal ps` sees which hook left it
/// rather than a group with no story.
pub struct Registered<'a> {
    /// The registry this row goes in.
    pub conn: &'a Connection,
}

impl Ownership for Registered<'_> {
    fn attach(&self, phase: Phase, context: &Context, pgid: u32) -> Result<()> {
        let session = Session {
            id: SessionId::from_ulid(ulid::Ulid::new()),
            environment_id: context.environment,
            actor: Actor {
                kind: crate::runtime::actor::current()?.kind,
                name: ActorName::parse(format!("{ACTOR_PREFIX}{phase}"))?,
            },
            pid: Some(pgid),
            pgid: Some(pgid),
            started_at: Timestamp::now(),
            ended_at: None,
        };
        sessions::insert(self.conn, &session)?;
        tracing::debug!(pgid, phase = phase.key(), "a hook left a process group behind");
        Ok(())
    }
}

/// A phase with nowhere to record a group, and the reason in the words the report uses.
///
/// `pre_new` is the one this exists for. It runs before the unit has rows at all, so
/// there is no materialisation for a session to belong to and no later command that
/// would ever read one; a row written against an identifier the create then fails to
/// commit would name nothing. Every other phase runs after the environment row exists
/// and can therefore hold a group, `post_reclaim` included: its environment is
/// [`crate::model::EnvState::Absent`] by then, which is exactly the state `nodal gc`
/// sweeps.
pub struct Unattachable {
    /// Why this phase holds no row a group can belong to.
    pub why: &'static str,
}

impl Ownership for Unattachable {
    fn attach(&self, phase: Phase, _context: &Context, _pgid: u32) -> Result<()> {
        Err(Error::HookGroupUnattachable { phase, why: self.why })
    }
}

/// What `pre_new` answers with, which is the one phase that has no row of its own.
#[must_use]
pub const fn before_the_rows() -> Unattachable {
    Unattachable { why: "the unit's rows are written after this hook runs" }
}

/// The prefix the actor of a hook's own session row carries.
const ACTOR_PREFIX: &str = "hook:";

/// Start the shell, wait for it, and turn a non-zero exit into the error that names
/// which hook it was. The one place this module spawns a process.
///
/// The shell is put in a process group of its own before it starts, so that everything
/// it starts is one identifier afterwards. Three outcomes follow from what that group
/// holds once the shell has exited:
///
/// - it is empty, which is what an ordinary hook leaves, and there is nothing to record;
/// - it is not empty and the hook succeeded, so the group is `owner`'s to write down;
/// - it is not empty and something went wrong, so the group is stopped.
///
/// A hook that failed is in the third case whatever it left running. The operation is
/// about to be refused and its caller will see an error, and leaving a process behind
/// under an error is the shape this seam exists to rule out.
///
/// # Why the hook is given no terminal input
///
/// A process in a group of its own is not the terminal's foreground group, so a read
/// from the terminal stops it with `SIGTTIN` rather than answering it — and a stopped
/// hook is a lifecycle command that never returns. End of file is the honest answer, and
/// it is the one [`crate::runtime::run`] already gives a tether for the same reason. A
/// hook that prompted was not interactive before this either: its output is read through
/// a pipe and appears only once it has exited, so the question was never on the screen
/// when the answer was wanted.
fn execute(
    phase: Phase,
    command: &str,
    directory: &Path,
    context: &Context,
    owner: &dyn Ownership,
) -> Result<()> {
    let mut shell = Command::new(SHELL);
    shell.arg("-c").arg(command).current_dir(directory);
    shell.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (name, value) in context.vars() {
        shell.env(name, value);
    }
    grouped(&mut shell);
    let child =
        shell.spawn().map_err(|source| Error::ToolSpawn { program: SHELL.to_owned(), source })?;
    let group = group_of(&child);
    let output = child
        .wait_with_output()
        .map_err(|source| Error::ToolSpawn { program: SHELL.to_owned(), source })?;
    let left = group.filter(|pgid| stop::Live.alive(Target::Group(*pgid)));

    if !output.status.success() {
        if let Some(pgid) = left
            && !stop_group(pgid)
        {
            tracing::warn!(
                pgid,
                phase = phase.key(),
                "a failed hook left a group that would not stop"
            );
        }
        return Err(Error::HookFailed {
            phase,
            command: command.to_owned(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_owned(),
        });
    }
    let Some(pgid) = left else { return Ok(()) };
    match owner.attach(phase, context, pgid) {
        Ok(()) => Ok(()),
        Err(reason) => Err(Error::HookGroupUnowned {
            phase,
            pgid,
            reason: reason.to_string(),
            stopped: stop_group(pgid),
        }),
    }
}

/// Put the hook's shell in a process group of its own.
///
/// Zero means "a new group, whose identifier is this child's own identifier", which is
/// what [`crate::runtime::run`] already asks for a tether.
#[cfg(unix)]
fn grouped(command: &mut Command) {
    use std::os::unix::process::CommandExt as _;

    command.process_group(0);
}

/// A host with no process groups starts the shell as it always did.
#[cfg(not(unix))]
fn grouped(_command: &mut Command) {}

/// The group the shell was put in, and nothing on a host that has none.
///
/// The identifier is the child's own, which is what [`grouped`] asked the system for.
/// The four identifiers that are never a target are never an answer either: a group
/// this process made is not group zero, not the system, and not the group this process
/// or its caller is in. Asking anyway is a cheap way of never recording one of them,
/// and a machine that somehow answered with one is told about rather than acted on.
#[cfg(unix)]
fn group_of(child: &Child) -> Option<u32> {
    let pgid = child.id();
    if stop::is_spared(Target::Group(pgid)) {
        tracing::warn!(pgid, "a hook's shell is not in a group of its own; it is not recorded");
        return None;
    }
    Some(pgid)
}

#[cfg(not(unix))]
fn group_of(_child: &Child) -> Option<u32> {
    None
}

/// Stop one group with the ladder every other teardown uses, and answer whether it has
/// gone.
fn stop_group(pgid: u32) -> bool {
    stop::processes(&stop::Live, &[Target::Group(pgid)], stop::GRACE).is_clear()
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

    use std::cell::Cell;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{Approvals, Context, Ownership, PHASES, Phase};
    use crate::model::{CommandLine, Hooks};
    use crate::runtime::stop::{Live, Signals as _, Target};
    use crate::{Error, Result};

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

    /// A shell line that backgrounds a process which outlives the shell, and writes that
    /// process's identifier into `record`.
    ///
    /// The whole background list is redirected, so that nothing holds the pipes the hook
    /// output is read from open. Otherwise this would wait five minutes rather than
    /// assert anything.
    fn backgrounds(record: &Path) -> String {
        format!("( exec sleep 300 ) >/dev/null 2>&1 & printf %s \"$!\" > {}", record.display())
    }

    /// The process the line above backgrounded.
    fn left_behind(record: &Path) -> u32 {
        std::fs::read_to_string(record).unwrap().trim().parse().unwrap()
    }

    /// Whether a process is still there, asked the way [`super::stop`] asks it.
    fn alive(pid: u32) -> bool {
        Live.alive(Target::Process(pid))
    }

    /// An owner that cannot write the row, which is what a registry failure looks like
    /// from here.
    struct Refuses;

    impl Ownership for Refuses {
        fn attach(&self, phase: Phase, _context: &Context, _pgid: u32) -> Result<()> {
            Err(Error::HookGroupUnattachable { phase, why: "the storage refused" })
        }
    }

    /// An owner that accepts, and keeps what it was handed.
    #[derive(Default)]
    struct Accepts(Cell<Option<u32>>);

    impl Ownership for Accepts {
        fn attach(&self, _phase: Phase, _context: &Context, pgid: u32) -> Result<()> {
            self.0.set(Some(pgid));
            Ok(())
        }
    }

    #[test]
    fn a_group_whose_row_could_not_be_written_is_stopped_and_the_reason_is_the_error() {
        let out = TempDir::new().unwrap();
        let record = out.path().join("backgrounded");
        let command = backgrounds(&record);

        let refused =
            super::execute(Phase::PostNew, &command, out.path(), &sample(), &Refuses).unwrap_err();

        match refused {
            Error::HookGroupUnowned { phase, reason, stopped, .. } => {
                assert_eq!(phase, Phase::PostNew);
                assert!(reason.contains("the storage refused"), "{reason}");
                assert!(stopped, "the group was left running: {reason}");
            }
            other => panic!("the failed write was reported as {other}"),
        }
        assert!(!alive(left_behind(&record)), "a failed ownership write left a child alive");
    }

    #[test]
    fn a_group_that_survives_the_shell_is_handed_to_the_owner_and_left_running() {
        let out = TempDir::new().unwrap();
        let record = out.path().join("backgrounded");
        let owner = Accepts::default();

        super::execute(Phase::PostNew, &backgrounds(&record), out.path(), &sample(), &owner)
            .unwrap();

        let Some(pgid) = owner.0.get() else { panic!("the surviving group was not handed over") };
        let child = left_behind(&record);
        assert!(alive(child), "a recorded group was stopped anyway");
        // The identifier is a group, and it is the one the child is in: signalling it
        // reaches the child, which is the whole claim the row makes.
        assert!(Live.alive(Target::Group(pgid)), "the identifier is not a live group");
        assert!(super::stop_group(pgid), "the group would not stop");
        assert!(!alive(child), "the group did not hold the child");
    }

    #[test]
    fn a_hook_that_leaves_nothing_running_hands_the_owner_nothing() {
        let out = TempDir::new().unwrap();
        let owner = Accepts::default();

        super::execute(Phase::PostNew, "true", out.path(), &sample(), &owner).unwrap();

        assert_eq!(owner.0.get(), None, "a synchronous hook was recorded as a group");
    }

    #[test]
    fn a_failed_hook_stops_what_it_backgrounded_and_still_reports_its_own_failure() {
        let out = TempDir::new().unwrap();
        let record = out.path().join("backgrounded");
        let command = format!("{}; echo no >&2; exit 3", backgrounds(&record));
        let owner = Accepts::default();

        let failed =
            super::execute(Phase::PostNew, &command, out.path(), &sample(), &owner).unwrap_err();

        match failed {
            Error::HookFailed { code, stderr, .. } => {
                assert_eq!(code, Some(3));
                assert_eq!(stderr, "no");
            }
            other => panic!("a hook that exited 3 was reported as {other}"),
        }
        assert_eq!(owner.0.get(), None, "a failed hook's group was recorded");
        assert!(!alive(left_behind(&record)), "a failed hook left a child alive");
    }
}
