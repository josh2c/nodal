//! The one error type for `nodal-core`.
//!
//! Variants are added per module as modules arrive; a variant carries the values a
//! caller needs to act on, never a pre-formatted string.

use std::path::PathBuf;

use thiserror::Error as ThisError;

use crate::git::preflight;
use crate::lifecycle::hooks::Phase;
use crate::lifecycle::uniqueness::Finding;
use crate::model::{BaseId, BranchName, EnvId, OperationId, Slug, UnitId};

/// Every failure `nodal-core` can return.
#[derive(Debug, ThisError)]
#[non_exhaustive]
pub enum Error {
    /// A path could not be read, written or inspected.
    #[error("{path}: {source}")]
    Io {
        /// The path the operation was attempted on.
        path: PathBuf,
        /// The underlying operating-system error.
        #[source]
        source: std::io::Error,
    },

    /// A domain value did not have the shape its type requires. `kind` names the type
    /// in the words a user sees, so the message is the same wherever it is raised.
    #[error("{value:?} is not a valid {kind}")]
    InvalidValue {
        /// What the value was meant to be, for example `branch name`.
        kind: &'static str,
        /// The value as it was given.
        value: String,
    },

    /// A `nodal.toml` could not be read as a recipe.
    #[error("{path}: {source}", path = path.display())]
    Recipe {
        /// The recipe file that was parsed.
        path: PathBuf,
        /// Why it was rejected, with the line and column.
        #[source]
        source: Box<toml::de::Error>,
    },

    /// `nodal init` was asked to write a recipe over one that is already there.
    #[error("{path} already exists; pass --force to rewrite it", path = path.display())]
    RecipeExists {
        /// The recipe that is already in place.
        path: PathBuf,
    },

    /// A tracing subscriber was already installed in this process.
    #[error("logging is already initialised for this process")]
    LoggingAlreadyInitialised,

    /// The log filter given in the environment could not be parsed.
    #[error("invalid log filter {filter:?}: {source}")]
    LogFilter {
        /// The filter string as it was read from the environment.
        filter: String,
        /// Why the filter was rejected.
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },

    /// The `git` binary could not be started.
    #[error("could not run git: {source}")]
    GitSpawn {
        /// Why the process could not be spawned.
        #[source]
        source: std::io::Error,
    },

    /// A `git` invocation exited non-zero.
    #[error("git {args} in {repo}: {stderr}", args = args.join(" "), repo = repo.display())]
    Git {
        /// The repository the command ran in.
        repo: PathBuf,
        /// The arguments given to `git`, without the leading `git`.
        args: Vec<String>,
        /// The exit code, or `None` when a signal ended the process.
        code: Option<i32>,
        /// What `git` wrote to standard error.
        stderr: String,
    },

    /// `git` produced output that is not valid UTF-8.
    #[error("git {args} produced output that is not UTF-8", args = args.join(" "))]
    GitEncoding {
        /// The arguments of the invocation whose output could not be decoded.
        args: Vec<String>,
    },

    /// A record of `git` output did not have its documented shape.
    #[error("git {args} produced an unreadable record {record:?}", args = args.join(" "))]
    GitParse {
        /// The arguments of the invocation whose output could not be parsed.
        args: Vec<String>,
        /// The record as it was read.
        record: String,
    },

    /// Text that should have been an object id was not one.
    #[error("{text:?} is not a Git object id")]
    GitOid {
        /// The text that was rejected.
        text: String,
    },

    /// A directory is not inside a Git repository.
    #[error("{path} is not a Git repository")]
    NotARepository {
        /// The directory that was opened.
        path: PathBuf,
    },

    /// A Git operation is in progress, so the repository must not be cloned or adopted.
    #[error("{repo} has Git operations in progress: {states:?}", repo = repo.display())]
    GitInProgress {
        /// The repository that was inspected.
        repo: PathBuf,
        /// Every in-progress state found.
        states: Vec<preflight::State>,
    },

    /// An operation that is only safe in an independent repository met a linked worktree.
    #[error("{repo} is a linked worktree of {git_dir}", repo = repo.display(), git_dir = git_dir.display())]
    GitLinkedWorktree {
        /// The checkout that was operated on.
        repo: PathBuf,
        /// Its per-worktree Git directory.
        git_dir: PathBuf,
    },

    /// The registry could not be opened, read or written.
    #[error("{path}: {source}", path = path.display())]
    Store {
        /// The database file the statement ran against.
        path: PathBuf,
        /// What SQLite reported.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// A write the registry keeps unique met a row that is already there: a branch
    /// another open unit holds, a lease another environment took, a home already
    /// recorded. The caller decides what to do about it, so it is its own variant.
    #[error("{path}: the registry already holds a conflicting row: {source}", path = path.display())]
    StoreConflict {
        /// The database file the statement ran against.
        path: PathBuf,
        /// The constraint SQLite reported.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// A column did not hold a value the model accepts, so the registry is corrupt.
    #[error("{table}.{column} could not be read: {source}")]
    StoreRow {
        /// The table the row came from.
        table: &'static str,
        /// The column that could not be read.
        column: &'static str,
        /// Why it could not be read.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// A model value could not be encoded into the form a column keeps.
    #[error("a {kind} could not be encoded for the registry")]
    StoreEncode {
        /// What was being encoded, in the words a user sees.
        kind: &'static str,
    },

    /// The registry could not be put into write-ahead logging mode, so concurrent
    /// readers and writers would not be safe.
    #[error("{path} is in {found:?} journal mode, not wal", path = path.display())]
    StoreJournalMode {
        /// The database file that was opened.
        path: PathBuf,
        /// The mode it reported after the pragma was applied.
        found: String,
    },

    /// A schema migration failed; the database is unchanged.
    #[error("{path}: migration {version} ({name}) failed: {source}", path = path.display())]
    StoreMigration {
        /// The database file that was being migrated.
        path: PathBuf,
        /// Which step failed.
        version: u32,
        /// What that step does.
        name: &'static str,
        /// What SQLite reported.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// The registry was written by a later version of Nodal, which may have changed
    /// the meaning of rows this build would read.
    #[error("{path} is at schema version {found}, and this build understands {supported}", path = path.display())]
    StoreTooNew {
        /// The database file that was opened.
        path: PathBuf,
        /// The version recorded in the file.
        found: u32,
        /// The newest version this build migrates to.
        supported: u32,
    },

    /// A read type could not be encoded as JSON. `kind` names the document, so the
    /// message says what was being written rather than only why it failed.
    #[error("a {kind} could not be rendered as JSON: {source}")]
    Render {
        /// What was being rendered, in the words a user sees.
        kind: &'static str,
        /// Why `serde_json` refused it.
        #[source]
        source: serde_json::Error,
    },

    /// A step of an operation failed. The steps before it were undone, so nothing the
    /// operation did is left; the failure that stopped it is the source.
    #[error("{kind} failed at step {key:?}: {source}")]
    OperationStep {
        /// The run, as the journal records it.
        operation: OperationId,
        /// Which operation it was.
        kind: &'static str,
        /// The step that failed.
        key: String,
        /// What the step reported.
        #[source]
        source: Box<Error>,
    },

    /// Undoing an operation failed. Unlike every other failure here, this one means
    /// something is left behind: the run stays in the journal as `failed` and every
    /// later invocation reports it.
    #[error("{kind} could not be undone at step {key:?}: {why}")]
    OperationUndo {
        /// The run, as the journal records it.
        operation: OperationId,
        /// Which operation it was.
        kind: &'static str,
        /// The step whose undo failed.
        key: String,
        /// Why it failed, rendered, because the undo carried on past it.
        why: String,
    },

    /// An operation reached a terminal state but the journal had no run of it still
    /// marked `running` to close. The registry and the journal disagree, which no
    /// sequence of operations produces; something else wrote to the journal.
    #[error("{kind} finished but its journal entry is not there to close")]
    OperationVanished {
        /// The run, as the journal recorded it.
        operation: OperationId,
        /// Which operation it was.
        kind: &'static str,
    },

    /// The per-machine secrets file grants access to an account other than its owner.
    /// Reading it is refused: the values in it are the one thing Nodal handles that a
    /// person cannot re-derive. The mode is reported, never the contents.
    #[error("{path} is mode {mode:o}; it must be {owner_only:o}", path = path.display(), owner_only = crate::env::secrets::OWNER_ONLY)]
    SecretsPermissions {
        /// The file that was refused.
        path: PathBuf,
        /// The mode it was found at, as the permission bits alone.
        mode: u32,
    },

    /// A manifest could not be rendered as TOML. It holds no secret, so the underlying
    /// error is safe to carry.
    #[error("a manifest could not be written: {source}")]
    ManifestEncode {
        /// Why `toml` refused it.
        #[source]
        source: toml::ser::Error,
    },

    /// A directory carries no `.nodal/manifest.toml`, so it is not an activated home.
    #[error("{path} is not a unit home; no {file} in it or any directory above it", path = path.display(), file = crate::env::files::MANIFEST)]
    NotAHome {
        /// The directory the search started from.
        path: PathBuf,
    },

    /// A row another row's foreign key points at was not there. No sequence of
    /// operations produces that, so the registry has been written to by something else.
    #[error("{table} row {id} is referenced but is not there")]
    StoreMissingRow {
        /// The table the row should have been in.
        table: &'static str,
        /// The identifier that was followed.
        id: String,
    },

    /// Every port block in the range is taken, so a new project cannot be given one.
    #[error("no port block is free between {first} and {last}")]
    PortBlockRangeFull {
        /// The lowest port the range covers.
        first: u16,
        /// The highest port the range covers.
        last: u16,
    },

    /// Every port in a project's block is held, so the environment gets none.
    #[error("ports {first} to {last} are all held")]
    PortBlockFull {
        /// The environment that asked for a port.
        environment: EnvId,
        /// The lowest port in the block.
        first: u16,
        /// The highest port in the block.
        last: u16,
    },

    /// A fixed port was refused, and by the time the holder was read it had let go,
    /// repeatedly. Something is taking and releasing that port in a loop.
    #[error("port {port} changed hands during each of {attempts} attempts to claim it")]
    PortFixedContended {
        /// The port that was being claimed.
        port: u16,
        /// How many times the claim was attempted.
        attempts: usize,
    },

    /// A listener scan was asked for on a host that does not publish the table it reads.
    #[error("a listener scan reads /proc/net/tcp, which {host} does not have")]
    ListenerScanUnsupported {
        /// The operating system the process is running on.
        host: &'static str,
    },

    /// A clone was asked for at a destination it cannot be made at.
    #[error("{destination} cannot hold a clone: {why}", destination = destination.display())]
    MaterializeDestination {
        /// The destination that was given.
        destination: PathBuf,
        /// Why it cannot be used.
        why: &'static str,
    },

    /// An exclusion list would leave a tracked path out of a copy.
    ///
    /// A copy that is missing a path the commit tracks is dirty the moment it is made:
    /// `git status` in it reports a deletion for every file under that path. So a
    /// materialization refuses the list instead of making such a copy.
    #[error(
        "these paths are tracked in {source_tree}, so a copy must not leave them out: {paths}",
        source_tree = source_tree.display(),
        paths = paths.iter().map(|path| path.display().to_string()).collect::<Vec<String>>().join(", ")
    )]
    ExcludesTrackedPath {
        /// The tree the copy is made from.
        source_tree: PathBuf,
        /// Every excluded path the tree tracks, in the order the list holds them.
        paths: Vec<PathBuf>,
    },

    /// A materialization backend was asked to work where it does not.
    #[error("the {backend} backend does not work on {path}", path = path.display())]
    MaterializeUnsupported {
        /// The backend that was called.
        backend: &'static str,
        /// The path it was called on.
        path: PathBuf,
    },

    /// Neither the override nor the operating system says where this user's own
    /// directory is, so Nodal cannot work out where its state belongs.
    #[error("no home directory; set {variable} to say where Nodal keeps its state")]
    NoHomeDirectory {
        /// The variable that would answer the question.
        variable: &'static str,
    },

    /// A home would overlap a tree Nodal already knows: the one it is cloned from, a
    /// project a person works in, or another unit's home. A home inside any of those
    /// makes the next clone copy a copy, and makes a reclaim remove what is not its own.
    #[error("{home} overlaps {tree}, which is {what}", home = home.display(), tree = tree.display())]
    InsideSource {
        /// The destination that was refused.
        home: PathBuf,
        /// The tree it overlapped.
        tree: PathBuf,
        /// What that tree is, in the words the message uses.
        what: &'static str,
    },

    /// A directory an operation was told to act on carries no `.nodal/id`, so it cannot
    /// be shown to be the home the registry named.
    #[error("{home} carries no marker, so it is not a home Nodal may act on", home = home.display())]
    HomeUnmarked {
        /// The directory that was inspected.
        home: PathBuf,
    },

    /// A home's marker names another unit. Something moved, copied or restored the
    /// directory, and acting on it would act on the wrong unit's work.
    #[error("{home} is marked for unit {found}, not {expected}", home = home.display())]
    HomeMarkedFor {
        /// The directory that was inspected.
        home: PathBuf,
        /// The unit its marker names.
        found: UnitId,
        /// The unit the caller expected.
        expected: UnitId,
    },

    /// Another open unit already holds the branch. The rule is the registry's partial
    /// unique index; this is the reading of it that can name the holder.
    #[error("branch {branch} is held by the open unit {slug} ({unit})")]
    UnitBranchHeld {
        /// The branch that was asked for.
        branch: BranchName,
        /// The handle of the unit that holds it, which is what `nodal ls` shows.
        slug: Slug,
        /// That unit's identity.
        unit: UnitId,
    },

    /// A shell was named that Nodal does not write an integration for.
    #[error("{name:?} is not a shell nodal speaks; it speaks {shells}", shells = crate::runtime::Shell::names().join(", "))]
    UnknownShell {
        /// The name that was given.
        name: String,
    },

    /// The directory a command was run in belongs to no recorded project.
    #[error("{path} is in no project Nodal knows; run `nodal new` in the project first", path = path.display())]
    ProjectNotFound {
        /// The directory the command was run in.
        path: std::path::PathBuf,
    },

    /// No unit of any project has the slug that was given.
    #[error("no unit is called {slug:?}")]
    UnitNotFound {
        /// The slug that was looked for.
        slug: String,
    },

    /// More than one project has a unit with that slug, so the target is not decided.
    #[error("{slug:?} is a unit of {projects}; run the command inside the project you mean", projects = projects.join(" and "))]
    UnitAmbiguous {
        /// The slug that was looked for.
        slug: String,
        /// The projects that each have one.
        projects: Vec<String>,
    },

    /// A unit exists but has no home on any host, so there is nowhere to enter.
    #[error("{slug:?} has no home; it has not been materialised or it was reclaimed")]
    UnitNotMaterialized {
        /// The slug of the unit.
        slug: String,
    },

    /// A process scan was asked for on a host whose process table Nodal cannot read.
    #[error("a process scan reads /proc, which {host} does not have")]
    ProcessScanUnsupported {
        /// The operating system the process is running on.
        host: &'static str,
    },

    /// A branch that had to exist did not.
    #[error("{repo} has no branch {branch:?}", repo = repo.display())]
    GitUnknownBranch {
        /// The repository that was inspected.
        repo: PathBuf,
        /// The branch that was expected.
        branch: String,
    },

    /// The commit a base is keyed to is in neither the remote nor the checkout.
    #[error("commit {commit} is not in the remote or the checkout it was asked for")]
    BaseCommitMissing {
        /// The commit that could not be found.
        commit: String,
    },

    /// A base still has units cloned from it, so it cannot be removed.
    #[error("base {base} still holds {pins} unit(s)")]
    BasePinned {
        /// The base that was not removed.
        base: BaseId,
        /// How many units hold it.
        pins: u32,
    },

    /// Nothing this project has built answers to the name that was given.
    #[error("no base of this project is called {name:?}")]
    BaseUnknown {
        /// What was asked for.
        name: String,
    },

    /// A tool a base build runs could not be started, usually because it is not
    /// installed or not on the path.
    #[error("could not run {program}: {source}")]
    ToolSpawn {
        /// The program that was to be started.
        program: String,
        /// Why it could not be.
        #[source]
        source: std::io::Error,
    },

    /// A unit holds work that exists nowhere but this machine, so a destructive
    /// operation refused it.
    #[error("{slug} holds work that is only here: {found}", found = Finding::summarise(findings))]
    NotUnique {
        /// The unit that was not removed.
        slug: Slug,
        /// What was found, in the order the check makes it.
        findings: Vec<Finding>,
    },

    /// A unit was asked for that has already been reclaimed.
    #[error("{slug} was reclaimed already")]
    AlreadyReclaimed {
        /// The unit that was asked for.
        slug: Slug,
    },

    /// A recipe hook was reached whose exact command line nobody has approved.
    #[error(
        "the {phase} hook of {project} is not approved: {command:?}; \
         run `nodal init` in that project to approve the hooks it declares",
        phase = phase.key(),
        project = project.display()
    )]
    HookNotApproved {
        /// Which hook it is.
        phase: Phase,
        /// The project whose recipe declares it.
        project: PathBuf,
        /// The command line as the recipe writes it.
        command: String,
    },

    /// A recipe hook ran and exited non-zero.
    #[error(
        "the {phase} hook failed: {command:?} {outcome}: {stderr}",
        phase = phase.key(),
        outcome = code.map_or_else(
            || String::from("was ended by a signal"),
            |code| format!("exited {code}")
        )
    )]
    HookFailed {
        /// Which hook it is.
        phase: Phase,
        /// The command line that was run.
        command: String,
        /// Its exit code, or `None` when a signal ended it.
        code: Option<i32>,
        /// What it wrote to standard error.
        stderr: String,
    },

    /// A tool a base build ran exited non-zero.
    #[error("{program} {args} in {dir}: {stderr}", args = args.join(" "), dir = dir.display())]
    Tool {
        /// The program that was run.
        program: String,
        /// The arguments it was given.
        args: Vec<String>,
        /// The directory it ran in.
        dir: PathBuf,
        /// Its exit code, or `None` when a signal ended it.
        code: Option<i32>,
        /// What it wrote to standard error.
        stderr: String,
    },
}

impl Error {
    /// Attach a path to an [`std::io::Error`], for use with `map_err`.
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { path: path.into(), source }
    }
}

/// The result type every fallible `nodal-core` function returns.
pub type Result<T, E = Error> = std::result::Result<T, E>;
