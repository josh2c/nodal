//! The Claude Code integration: four hooks, and what each of them is allowed to assume.
//!
//! # What was measured, and what follows from it
//!
//! Claude Code fires named events at commands a project declares in
//! `.claude/settings.json`. Four of them matter to Nodal, and they are not four of a
//! kind. One is a **provider**: Claude asks it where the work should happen and uses
//! the answer. Two are **observers**. One was never seen to fire at all.
//!
//! | event | what it is | what Nodal does |
//! |---|---|---|
//! | `WorktreeCreate` | provider: Claude requires an absolute path on standard output and ends the session without one | makes the unit, furnishes the home, records the attachment, and prints it |
//! | `SessionStart` | observer, fires more than once per session | prints the unit's memory, which Claude injects as context |
//! | `Stop` | observer | records the session's last message as a stated handoff |
//! | `WorktreeRemove` | observer that was never seen to fire | records a detach if it ever does, and removes nothing |
//!
//! # Where the observers are declared, and why the provider carries them
//!
//! Claude Code reads `.claude/settings.json` from the directory a session works in.
//! `nodal init` writes the four hooks into the project, and `WorktreeCreate` then moves
//! the session out of the project and into a unit home. The file that declared the hook
//! which made the home is no longer in scope.
//!
//! This was measured on 2026-09-08, headless and interactive, and it is the same in
//! both: with `--worktree`, `WorktreeCreate` fires in the project root and no observer
//! fires anywhere. Without `--worktree`, all three observers fire. So a home carries a
//! settings file of its own ([`carry_settings`]), and the observers fire for the rest
//! of the session.
//!
//! **Every home carries one, not only the ones this hook made.** The write is part of
//! furnishing a home ([`crate::context`]), beside the memory and the vendor pointers,
//! so a unit made by `nodal new` and one adopted in place carry it too. There is one
//! rule for every file Nodal puts in a home and it is stated once, in
//! [`crate::env::files::WRITTEN`]: Nodal writes it only where Git does not track it,
//! hides it in the home's `info/exclude` when it wrote it, and never touches it when
//! the project tracks it.
//!
//! **What goes there is the project's own file.** A regenerated set of four hooks would
//! be the only settings in scope for the rest of the session, so the project's
//! permissions, its deny rules and every hook somebody else installed would stop
//! applying the moment the session moved into the home. A project with no settings file
//! of its own gets the four hooks.
//!
//! Whether a project ignores `.claude/` is not something Nodal may assume, and neither
//! is whether the home already has a file. [`carry_settings`] states the three cases.
//! Wherever the file the home ends up with declares none of Nodal's hooks, that is one
//! line on standard error and one note event: nothing observes the session, and what
//! would work is committing the hooks or installing them in the person's own settings.
//!
//! The attachment is not left to an observer. `WorktreeCreate` is the one hook that is
//! certain to have run, so it is what records that Claude Code took the home. It is
//! recorded the way the settings are written: a failure is a line on standard error and
//! never a refusal, because a unit that was built and answered for must not be left
//! with nobody in it over a line that could not be logged.
//!
//! # A request from inside a home
//!
//! A home now carries the provider hook, so `WorktreeCreate` can fire in a unit home as
//! well as in a project. A home is a checkout of the project and carries the project's
//! recipe, so making a unit of it would register the home as a project of its own and
//! clone a unit of a unit. The request is answered with the home the session is already
//! in, and nothing is created.
//!
//! What counts as a home is [`crate::lifecycle::ops::new::containing_home`], which
//! `nodal new` reads for its own refusal. It asks the registry rather than the disk: a
//! `.nodal/id` says which unit a directory claims to be, and a row says which unit this
//! machine has.
//!
//! # The provider contract
//!
//! `WorktreeCreate` is the one hook that can end a session. Claude reads one line of
//! standard output and treats it as the directory to work in; empty output, or output
//! that is not an absolute path, is fatal. So this hook has exactly one good answer and
//! one bad one, and the bad one is deliberate: when Nodal cannot answer at all it prints
//! [`REFUSED`] — a relative path with a dot segment in it, which Claude will not accept
//! — and says why on standard error. A person then reads one sentence instead of
//! watching a session end for no stated reason.
//!
//! **A project with no recipe is not one of those cases.** The hooks go in the person's
//! own settings by default, so this hook fires in every project on the machine and most
//! of them are not Nodal projects. Ending a session in each of them would make the
//! integration unusable. So [`default_worktree`] makes the worktree Claude Code would
//! have made for itself, under the project's `.claude/worktrees`, and answers with it.
//! Nodal registers nothing there: no unit, no environment, no event. A directory that is
//! not in a Git repository has no worktree to make, and the session is answered with the
//! directory it is already in.
//!
//! [`acceptable`] is the same rule read from Nodal's side: what this hook prints is
//! checked before it is printed, so a home that is somehow not an absolute path is a
//! refusal Nodal states rather than a session Claude ends.
//!
//! # Correlating by path, never by identifier
//!
//! A session carries more than one session identifier across a worktree creation: the
//! create payload holds one, and the session that starts in the new directory holds
//! another. `SessionStart` fires twice, with different identifiers. Nothing here reads
//! a session identifier for any purpose. The `cwd` a payload carries is what says which
//! unit is meant, and a unit home says whose it is in `.nodal/id`
//! ([`crate::lifecycle::marker`]).
//!
//! # What does not clean up
//!
//! `WorktreeRemove` was registered and never fired: not on a headless exit, not on
//! ending a desktop session, not on stopping one and removing its directory. Nodal
//! therefore treats the end of a session as no signal at all. A unit outlives the
//! session that made it **on purpose**: it is in `nodal ls`, it holds its branch, and
//! `nodal done`, `nodal merge`, `nodal reclaim` and `nodal gc` are what retire it. A
//! unit still standing after Claude has gone is the product working, not a leak.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;

use crate::adapters::settings::{self, Hook, Scope};
use crate::env::files;
use crate::lifecycle::marker;
use crate::lifecycle::ops::new::{self, Request};
use crate::model::{
    Actor, ActorKind, ActorName, EnvId, Epistemic, EventKind, Objective, Slug, UnitId,
};
use crate::store::{Store, environments, events};
use crate::{Error, Result, context, recipe, substrate};

/// What the provider hook prints when it cannot answer with a unit home.
///
/// It is a path on purpose, and a path Claude Code rejects on purpose: relative, and
/// with a dot segment in it. Printing nothing would end the session just as certainly
/// and would say nothing about why.
pub const REFUSED: &str = "./nodal-worktree-create-refused";

/// The line that hides the settings file Nodal writes into a home, in the form
/// `.git/info/exclude` takes: anchored at the top of the tree, so a `.claude/` a
/// project keeps somewhere else is not covered by it.
///
/// It is the table's ([`files::WRITTEN`]), read rather than written again here.
pub const EXCLUDE: &str = "/.claude/settings.json";

/// Where Claude Code keeps the worktrees it makes for itself, under a project root.
///
/// It is Claude Code's path, not Nodal's. The provider hook answers with a directory
/// under it in a project Nodal has no recipe for, so `claude --worktree` puts the
/// session exactly where it would have been with no hook installed at all.
pub const DEFAULT_WORKTREES: &str = ".claude/worktrees";

/// The handle such a worktree gets when the payload carried none Nodal could read.
const UNNAMED: &str = "session";

/// How many names are tried before the provider says the directory is full.
const NAMES: usize = 100;

/// The name of the provider event.
pub const WORKTREE_CREATE: &str = "WorktreeCreate";

/// The name of the event that fires when a session begins.
pub const SESSION_START: &str = "SessionStart";

/// The name of the event that fires when a session ends.
pub const STOP: &str = "Stop";

/// The name of the event that was registered and never seen to fire.
pub const WORKTREE_REMOVE: &str = "WorktreeRemove";

/// How long the provider hook may take, in seconds.
///
/// The first unit of a project is made from a base that has to be built, which is one
/// clone and one dependency install. The default Claude Code allows a hook is a minute,
/// and a session that ends because a `pnpm install` took ninety seconds would be a
/// worse first use of Nodal than one that waits.
const CREATE_TIMEOUT: u32 = 900;

/// How many lines of a transcript are read before the search for the last message
/// gives up. A transcript is read forwards and the last answer found wins, so the limit
/// is what stops a very long session from being read whole.
const TAIL_LINES: usize = 4096;

/// What Claude Code is called in an event and in a report.
const AGENT: &str = "claude-code";

/// What separates the segments of a path, on either kind of host.
const SEPARATORS: [char; 2] = ['/', '\\'];

/// The payload Claude Code sends on standard input.
///
/// Every field is optional, because the shape differs between the desktop application
/// and the command line and has to keep working when it differs again: the desktop
/// sends no `transcript_path` and no scratchpad directory, and carries a `name` slug it
/// derived from the opening prompt.
///
/// There is no session identifier here, and that is the point. See the module note on
/// correlating by path.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Payload {
    /// The event Claude Code says it is firing.
    pub hook_event_name: String,
    /// The directory the event is about: the project for a creation, the unit home for
    /// everything after it.
    pub cwd: PathBuf,
    /// The handle Claude derived from the opening prompt. `WorktreeCreate` only.
    pub name: String,
    /// The session's transcript. Empty in the desktop application.
    pub transcript_path: String,
    /// The session's last message, which the command line sends and the desktop does
    /// not.
    pub last_assistant_message: String,
}

impl Payload {
    /// The payload on `reader`, or an empty one when there is nothing to read.
    ///
    /// A hook that cannot read its payload is not a failure by itself. Three of the
    /// four do nothing without one, and the fourth says so in the words its own
    /// contract requires.
    ///
    /// # Errors
    /// [`Error::Io`] when standard input cannot be read.
    pub fn read(mut reader: impl std::io::Read) -> Result<Self> {
        let mut text = String::new();
        reader.read_to_string(&mut text).map_err(Error::io("<stdin>"))?;
        Ok(serde_json::from_str(&text).unwrap_or_default())
    }

    /// The handle to give the unit, when the payload carries one Nodal can use.
    #[must_use]
    pub fn slug(&self) -> Option<Slug> {
        Slug::parse(self.name.trim()).ok()
    }

    /// What the unit is for, as far as anything here knows.
    ///
    /// It is the slug Claude derived from somebody's opening prompt, which is a reading
    /// of an intent rather than a statement of one. It is recorded as such
    /// ([`Epistemic::Observed`]), so a person meeting the unit later is told they are
    /// reading a recovered line.
    #[must_use]
    pub fn objective(&self) -> Option<Objective> {
        Objective::parse(self.name.trim()).ok()
    }
}

/// The hooks `nodal init` installs, in the order they are written.
///
/// Each command names the binary by the word `nodal` and nothing else, so the file is
/// the same on every machine and can be committed. Each guards on `nodal` being on the
/// `PATH`, because the file may be committed and cloned by somebody who has never heard
/// of Nodal: for the three observers that means doing nothing quietly, and for the
/// provider it means the refusal its contract requires.
#[must_use]
pub fn hooks() -> Vec<Hook> {
    vec![
        Hook {
            event: WORKTREE_CREATE,
            command: format!(
                "if command -v nodal >/dev/null 2>&1; then exec nodal claude-code \
                 worktree-create; fi; echo 'nodal: not on PATH, so no unit was made for this \
                 session' >&2; echo '{REFUSED}'; exit 1"
            ),
            timeout: Some(CREATE_TIMEOUT),
        },
        observer(SESSION_START, "session-start"),
        observer(STOP, "stop"),
        observer(WORKTREE_REMOVE, "worktree-remove"),
    ]
}

/// One of the three hooks that may do nothing and say nothing.
fn observer(event: &'static str, verb: &str) -> Hook {
    Hook {
        event,
        command: format!(
            "if command -v nodal >/dev/null 2>&1; then exec nodal claude-code {verb}; fi"
        ),
        timeout: None,
    }
}

/// Whether Claude Code would accept `path` as the directory to work in.
///
/// Absolute, and with no dot segment in it. The second half is not pedantry: a path
/// holding `.` or `..` is the shape Claude rejects, and it is also the shape [`REFUSED`]
/// deliberately takes.
///
/// The segments are read out of the text rather than out of
/// [`std::path::Path::components`], which drops a `.` in the middle of a path before
/// anything can see it.
#[must_use]
pub fn acceptable(path: &Path) -> bool {
    let text = path.to_string_lossy();
    path.is_absolute() && !text.split(SEPARATORS).any(|part| part == "." || part == "..")
}

/// `WorktreeCreate`: make the unit Claude is about to work in, and answer with its home.
///
/// The project is the directory the payload names. A unit is made only where there is a
/// recipe: a project nobody has run `nodal init` in has nothing to say about how a home
/// is built, and guessing at that moment would put an agent in a directory that cannot
/// run the project's tests. Such a project gets Claude Code's own worktree instead
/// ([`default_worktree`]), because a refusal there ends the session.
///
/// A request made from inside a unit home is answered with that home, and creates
/// nothing. See the module note.
///
/// The order is the contract. The home is checked against [`acceptable`] before
/// anything durable is written about it, because a refusal ends the session: a home
/// Claude will not take must not leave behind a furnished directory and an event saying
/// a session took it. Furnishing and the attachment record come after, and neither may
/// fail the create.
///
/// # Errors
/// [`Error::InvalidValue`] when the directory that was made is not a path Claude would
/// accept, and whatever the create or the worktree itself reported.
pub fn worktree_create(store: &mut Store, payload: &Payload) -> Result<PathBuf> {
    let root = project_root(payload);
    if let Some((home, _)) = new::containing_home(store.conn(), &root)? {
        return answer(home);
    }
    if !recipe::load(&root)?.written {
        return default_worktree(&root, payload);
    }
    let request = Request {
        source: root.clone(),
        objective: payload.objective(),
        objective_epistemic: Epistemic::Observed,
        name: payload.slug(),
        parent_branch: None,
        hooks: true,
    };
    let progress: Arc<dyn substrate::Reporter> = substrate::sink(false);
    let report = new::create(store, &request, &progress)?;
    let unit = report.unit.id;
    let environment = report
        .unit
        .environment
        .ok_or_else(|| refusal(String::from("the unit was made without a home")))?;
    let home = answer(environment.home)?;
    let subject = (unit, Some(environment.id));
    furnish(store, &home, subject);
    told(
        store,
        subject,
        EventKind::Attached,
        Epistemic::Observed,
        String::from("claude code took this home for a session"),
    );
    Ok(home)
}

/// The home the provider prints, when it is one Claude Code would take.
///
/// # Errors
/// [`Error::InvalidValue`] when it is not, so that Nodal states the refusal rather than
/// leaving Claude to end the session over a line it could not read.
fn answer(home: PathBuf) -> Result<PathBuf> {
    if acceptable(&home) {
        return Ok(home);
    }
    Err(refusal(format!("{} is not a path Claude Code would accept", home.display())))
}

/// Put the memory, the pointers and the settings in the home, and say what could not be
/// done.
///
/// The writing itself is [`context::refresh_at`], which every command that touches a
/// unit calls; this is the one caller that also has a unit to write the answer against.
///
/// Nothing here may fail the create. The session has a home and must start: a settings
/// file that could not be written costs it the observers it would have had, and an
/// event that could not be written costs a line of the log. Ending the session over
/// either would leave a fully built unit with nobody in it, which is the shape of the
/// bug this path exists to fix.
///
/// What could not be done is said twice: once on standard error, for the person
/// watching the session start, and once in the unit's log, for whoever reads the unit
/// afterwards and wonders why it recorded nothing.
fn furnish(store: &Store, home: &Path, subject: (UnitId, Option<EnvId>)) {
    let causes = match context::refresh_at(store.conn(), home) {
        Ok(report) => {
            context::report_notes(&report);
            report.notes.into_iter().map(|note| note.cause).collect()
        }
        Err(error) => {
            let cause = format!("the home could not be furnished: {error}");
            eprintln!("nodal: context: {cause}");
            vec![cause]
        }
    };
    for cause in causes {
        told(store, subject, EventKind::Note, Epistemic::Observed, cause);
    }
}

/// Put the settings a session will read into `home`, and say what could not be done.
///
/// Claude Code reads `.claude/settings.json` from the directory a session works in, and
/// `WorktreeCreate` moves the session out of the project and into the home. Without a
/// file here the three observers never fire: no memory is injected and no handoff is
/// recorded. It was measured that way.
///
/// The rule is the table's ([`files::WRITTEN`]) and `tracks` is the answer to its one
/// question. Three cases:
///
/// - **The project tracks the file.** It arrived with the clone and it is the
///   project's. Nodal does not touch it, whatever it says. A file that declares none of
///   Nodal's hooks is one note ([`hookless`]).
/// - **Git does not track it and the home has none.** Nodal writes one: the project's
///   own file ([`settings_for`]) rather than a regenerated four, so the permissions,
///   the deny rules and every hook somebody else installed keep applying after the
///   session moves.
/// - **Git does not track it and the home has one anyway** — a base build wrote it, or
///   a `post_new` hook did, or an earlier command of Nodal's. Its bytes are somebody
///   else's and are left alone; it is hidden all the same, because an untracked file in
///   a home is a home the uniqueness check calls dirty.
///
/// Every file Nodal may write here is hidden from `git status` through the home's
/// `.git/info/exclude`, and written atomically for the reason the memory beside it is:
/// an agent may be reading it.
///
/// # Errors
/// [`Error::Io`] when the project's file cannot be read or the home's cannot be
/// written, and [`Error::InvalidValue`] when the project's file is not a JSON object.
pub fn carry_settings(
    root: &Path,
    home: &Path,
    git: &crate::git::Git,
    tracks: &files::Tracks,
) -> Result<Vec<String>> {
    let path = home.join(settings::FILE);
    if !tracks.may_write(settings::FILE) {
        let held = read(&path)?;
        if tracks.tracks(settings::FILE) && !settings::holds_hooks(&held) {
            return Ok(vec![hookless()]);
        }
        return Ok(tracks.cause().map(|_| tracks.left_alone(settings::FILE)).into_iter().collect());
    }
    let mut notes = Vec::new();
    let held = read(&path)?;
    if held.is_empty() {
        let (text, note) = settings_for(root)?;
        notes.extend(note);
        let directory = home.join(settings::DIR);
        std::fs::create_dir_all(&directory).map_err(Error::io(&directory))?;
        context::atomic::write(&path, &text)?;
    } else if !settings::holds_hooks(&held) {
        notes.push(hookless());
    }
    notes.extend(hide(git));
    Ok(notes)
}

/// The one thing to say about a settings file that declares none of Nodal's hooks.
///
/// It is one sentence about a consequence and one about what to do, and what to do is
/// not `--claude-hooks=project`: that writes the hooks into the project's working file,
/// which changes nothing for a home whose copy came from a commit. The two things that
/// do work are committing the hooks, so the next clone carries them, and putting them in
/// the settings Claude Code reads for every project, which is what `nodal init
/// --claude-hooks` writes.
fn hookless() -> String {
    format!(
        "{} in this home declares none of nodal's hooks, so nothing observes this session \
         starting or stopping and this unit records only what a command writes. Commit the \
         hooks, or run `nodal init --claude-hooks` to put them in your own settings.",
        settings::FILE
    )
}

/// The text the home's settings file is written with, and what is worth saying about it.
///
/// The project's own file where there is one, and the four hooks where there is not. A
/// file holding only whitespace is no file: it would be copied into the home as a
/// document Claude Code cannot read, so it is treated as absent.
///
/// A project file that declares none of Nodal's hooks is still copied verbatim — the
/// hooks may be in the person's own settings, which is how the session reached this
/// code at all — but it is never copied silently.
fn settings_for(root: &Path) -> Result<(String, Option<String>)> {
    let held = read(&settings::path(root))?;
    if held.trim().is_empty() {
        return Ok((settings::add("", &hooks())?.unwrap_or_default(), None));
    }
    if settings::holds_hooks(&held) {
        return Ok((held, None));
    }
    Ok((held, Some(hookless())))
}

/// Tell the home's repository to leave the settings file alone.
///
/// The line comes from the table ([`files::WRITTEN`]), which is the only place the
/// names Nodal hides are written down. A file of Nodal's that shows as untracked is a
/// home the uniqueness check calls dirty, so `nodal reclaim`, `nodal done` and
/// `nodal gc` refuse it and `nodal merge` carries Nodal's file onto the unit's branch.
///
/// The block goes in the repository's **common** directory, for the reason
/// [`crate::context::pointer`] gives: it is the only `info/exclude` Git reads.
///
/// A repository that could not be asked is a note and not a failure. The file is
/// written either way, and the note is what says the home will look dirty.
fn hide(git: &crate::git::Git) -> Option<String> {
    let hidden = git.layout().and_then(|layout| files::exclude(&layout.common_dir, &[EXCLUDE]));
    hidden.err().map(|error| {
        format!("info/exclude: {error}; {} will show in git status in this home", settings::FILE)
    })
}

/// Append one event about this home, and never fail the caller over it.
///
/// `subject` is the unit and the environment the event is about, as
/// [`events::note_as`] takes them.
fn told(
    store: &Store,
    subject: (UnitId, Option<EnvId>),
    kind: EventKind,
    epistemic: Epistemic,
    body: String,
) {
    if let Err(error) = record(store, subject, kind, epistemic, body) {
        eprintln!("nodal: the unit's log could not be written: {error}");
    }
}

/// Append one event about this home, with Claude Code named as the actor.
fn record(
    store: &Store,
    subject: (UnitId, Option<EnvId>),
    kind: EventKind,
    epistemic: Epistemic,
    body: String,
) -> Result<()> {
    let line = events::Line { actor: claude()?, kind, epistemic, body };
    events::note_as(store.conn(), subject, line, &[])
}

/// `SessionStart`: the memory of the unit the session is starting in, when it is in one.
///
/// A directory that is not a unit home answers with nothing, and nothing is what the
/// hook prints: a session in an ordinary checkout is not one Nodal has anything to say
/// about. The event fires more than once per session and this is a read, so firing
/// twice injects the same text twice rather than doing anything twice.
///
/// # Errors
/// [`Error::Io`] when the home carries a memory that cannot be read.
pub fn session_start(payload: &Payload) -> Result<Option<String>> {
    let home = &payload.cwd;
    if marker::read(home)?.is_none() {
        return Ok(None);
    }
    let file = home.join(context::FILE);
    match std::fs::read_to_string(&file) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io(&file)(error)),
    }
}

/// `Stop`: record what the session said last as a handoff somebody stated.
///
/// It is a [`Epistemic::Stated`] event and never an observed one. Nodal did not watch
/// the work happen; an agent wrote a closing message, and what that message claims is
/// worth what the agent's claims are worth.
///
/// The message comes from the payload when Claude sends one, and from the transcript
/// when it names one that can be read. The desktop application sends neither, and that
/// is the case this degrades silently for: `Ok(false)` and not a word on any stream.
/// The unit loses nothing that was ever a fact, because the memory is compiled from the
/// registry rather than accumulated from what sessions said
/// ([`crate::context`]).
///
/// # Errors
/// [`Error::Store`] when the event cannot be written.
pub fn stop(store: &Store, payload: &Payload) -> Result<bool> {
    let Some(unit) = marker::read(&payload.cwd)? else { return Ok(false) };
    let Some(message) = last_message(payload) else { return Ok(false) };
    record(
        store,
        (unit, environment_at(store, unit, &payload.cwd)?),
        EventKind::Handoff,
        Epistemic::Stated,
        message,
    )?;
    Ok(true)
}

/// `WorktreeRemove`: note that a session let go of a home, and remove nothing.
///
/// This hook was measured and never fired. It is installed so that Nodal hears about a
/// version of Claude Code that does fire it, and it is written so that hearing about it
/// changes nothing that matters: one observed event saying an actor detached. The home
/// stays, the unit stays, the branch stays, and the commands that retire a unit are the
/// ones a person runs.
///
/// # Errors
/// [`Error::Store`] when the event cannot be written.
pub fn worktree_remove(store: &Store, payload: &Payload) -> Result<bool> {
    let Some(unit) = marker::read(&payload.cwd)? else { return Ok(false) };
    record(
        store,
        (unit, environment_at(store, unit, &payload.cwd)?),
        EventKind::Detached,
        Epistemic::Observed,
        String::from("claude code let go of this home; the unit was left as it is"),
    )?;
    Ok(true)
}

/// The environment of `unit` whose home is `cwd`, when the two agree.
fn environment_at(store: &Store, unit: UnitId, cwd: &Path) -> Result<Option<EnvId>> {
    Ok(environments::latest_for_unit(store.conn(), unit)?
        .filter(|environment| environment.home == cwd)
        .map(|environment| environment.id))
}

/// The project the payload is about. An empty `cwd` means the directory Nodal is in.
fn project_root(payload: &Payload) -> PathBuf {
    if payload.cwd.as_os_str().is_empty() { PathBuf::from(".") } else { payload.cwd.clone() }
}

/// Who Claude Code is, in an event.
///
/// It is named outright rather than read from the environment: the hook runs as a child
/// of Claude Code, and the one thing that is certain about it is which agent started it.
fn claude() -> Result<Actor> {
    Ok(Actor { kind: ActorKind::Agent, name: ActorName::parse(AGENT)? })
}

/// The refusal the provider hook reports.
fn refusal(why: String) -> Error {
    Error::InvalidValue { kind: "claude code worktree", value: why }
}

/// The directory a session gets in a project Nodal knows nothing about.
///
/// The hooks live in the person's own settings file, so this hook fires in every
/// project on the machine and most of them are not Nodal projects. A refusal there
/// would end the session, which is the fault this path exists to prevent. So Nodal
/// makes the worktree Claude Code would have made for itself, in the directory Claude
/// Code uses ([`DEFAULT_WORKTREES`]), and answers with it. Nothing is registered: no
/// unit, no environment, no event. Nodal made a directory and stepped out of the way.
///
/// A directory that is not in a Git repository has no worktree to make. The session is
/// answered with the directory it is already in, which is where a session started
/// without `--worktree` would have run anyway.
///
/// # Errors
/// [`Error::InvalidValue`] when no free name was found, when the directory cannot be
/// named absolutely, and whatever `git worktree add` reported.
fn default_worktree(root: &Path, payload: &Payload) -> Result<PathBuf> {
    let Ok(git) = crate::git::Git::open(root) else { return work_in_place(root) };
    let top = git.top_level()?;
    let wanted = payload.slug().map_or_else(|| String::from(UNNAMED), |slug| slug.to_string());
    let directory = top.join(DEFAULT_WORKTREES);
    let name = free_name(&git, &directory, &wanted)?;
    let home = answer(directory.join(&name))?;
    if let Some(parent) = home.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    git.add_worktree(&home, &name)?;
    eprintln!(
        "nodal: {} has no {}, so this session was given the worktree claude code makes for \
         itself; run `nodal init` there to get a unit instead",
        top.display(),
        recipe::FILE_NAME
    );
    Ok(home)
}

/// The directory a session in no repository at all is answered with: the one it is in.
///
/// # Errors
/// [`Error::InvalidValue`] when it is not a path Claude Code would accept, which is a
/// directory that cannot be named absolutely.
fn work_in_place(root: &Path) -> Result<PathBuf> {
    let here = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    eprintln!(
        "nodal: {} is not a git repository, so there is no worktree to make; this session \
         works where it started",
        here.display()
    );
    answer(here)
}

/// A name no directory under `directory` holds and no branch of `git` holds.
///
/// The wanted name first, then the wanted name and a number. Both halves matter and for
/// the same reason: **a directory that is already there holds somebody's work**, and
/// `git worktree add` on a branch another worktree has checked out is refused. Taking
/// either would cost a person something they had, so a free name is found instead of a
/// collision being resolved.
///
/// # Errors
/// [`Error::Git`] when the branches could not be read, and [`Error::InvalidValue`] when
/// [`NAMES`] names were all taken.
fn free_name(git: &crate::git::Git, directory: &Path, wanted: &str) -> Result<String> {
    for attempt in 1..=NAMES {
        let name = if attempt == 1 { wanted.to_owned() } else { format!("{wanted}-{attempt}") };
        if !directory.join(&name).exists() && !git.branch_exists(&name)? {
            return Ok(name);
        }
    }
    Err(refusal(format!("{} already holds {NAMES} worktrees called {wanted}", directory.display())))
}

/// The session's last message: what Claude sent, or what the transcript holds.
fn last_message(payload: &Payload) -> Option<String> {
    let sent = payload.last_assistant_message.trim();
    if !sent.is_empty() {
        return Some(sent.to_owned());
    }
    let path = payload.transcript_path.trim();
    if path.is_empty() {
        return None;
    }
    last_in_transcript(Path::new(path))
}

/// The last thing the assistant said in a transcript, when the file can be read.
///
/// Tolerant throughout, as [`crate::doctor::intent`] is over the same files: a missing
/// file, an unreadable line and a record this does not recognise all mean no message
/// rather than an error.
fn last_in_transcript(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut found = None;
    for line in BufReader::new(file).lines().take(TAIL_LINES) {
        let Ok(line) = line else { break };
        let Ok(record) = serde_json::from_str::<Spoken>(&line) else { continue };
        if let Some(text) = record.text() {
            found = Some(text);
        }
    }
    found
}

/// The part of a transcript record this module reads.
#[derive(Debug, Deserialize)]
struct Spoken {
    /// What kind of record it is. An answer is `assistant`.
    #[serde(rename = "type", default)]
    kind: String,
    /// Whether the record belongs to a sub-agent rather than to the session itself.
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    /// Where the text is.
    #[serde(default)]
    message: Option<Spoke>,
}

impl Spoken {
    /// The text this record holds, when it is one the session itself spoke.
    fn text(self) -> Option<String> {
        if self.kind != "assistant" || self.sidechain {
            return None;
        }
        let blocks = self.message?.content;
        let text: Vec<String> = blocks.into_iter().filter_map(|block| block.text).collect();
        let joined = text.join("\n").trim().to_owned();
        (!joined.is_empty()).then_some(joined)
    }
}

/// The message of an assistant record.
#[derive(Debug, Deserialize)]
struct Spoke {
    /// Its blocks. Text blocks are the only ones read.
    #[serde(default)]
    content: Vec<Said>,
}

/// One block of an assistant message.
#[derive(Debug, Deserialize)]
struct Said {
    /// Its text, which only a text block has.
    #[serde(default)]
    text: Option<String>,
}

// Putting the hooks in a project, and taking them out again.

/// What an install of the hooks did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The settings file that was written.
    pub path: PathBuf,
    /// Which file it was: the person's own, or the project's.
    pub scope: Scope,
    /// The events it now answers, in the order they were written.
    pub events: Vec<&'static str>,
    /// Whether the install made the file.
    pub created: bool,
}

/// Write Nodal's hooks into the settings file at `path`.
///
/// The caller says which file that is, because the two are chosen differently:
/// [`settings::user_path`] reads an environment variable, and [`settings::path`] takes
/// a project root. `scope` says which one was chosen, so the answer can be reported
/// without asking the path where it came from.
///
/// `Ok(None)` means the file already reads that way, which is what a second
/// `nodal init --claude-hooks` finds. Nothing else in the file is touched
/// ([`crate::adapters::settings`]).
///
/// # Errors
/// [`Error::InvalidValue`] when the settings file is not a JSON object, and
/// [`Error::Io`] when it cannot be read or written.
pub fn install(path: &Path, scope: Scope) -> Result<Option<Installed>> {
    let before = read(path)?;
    let hooks = hooks();
    let Some(after) = settings::add(&before, &hooks)? else { return Ok(None) };
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory).map_err(Error::io(directory))?;
    }
    std::fs::write(path, after).map_err(Error::io(path))?;
    Ok(Some(Installed {
        path: path.to_path_buf(),
        scope,
        events: hooks.iter().map(|hook| hook.event).collect(),
        created: before.is_empty(),
    }))
}

/// Take Nodal's hooks out of the settings file at `path`.
///
/// A file that holds nothing else afterwards is removed rather than left as an empty
/// document, and the `.claude` directory goes with it when the removal empties that
/// too. A directory a person keeps anything else in is left exactly as it is.
///
/// # Errors
/// [`Error::Io`] when the file cannot be read, written or removed.
pub fn uninstall(path: &Path) -> Result<bool> {
    let before = read(path)?;
    let Some(after) = settings::remove(&before, &hooks()) else { return Ok(false) };
    if !settings::is_empty(&after) {
        std::fs::write(path, after).map_err(Error::io(path))?;
        return Ok(true);
    }
    std::fs::remove_file(path).map_err(Error::io(path))?;
    if let Some(directory) = path.parent() {
        drop(std::fs::remove_dir(directory));
    }
    Ok(true)
}

/// The settings file's text, or nothing at all when there is no file.
///
/// # Errors
/// [`Error::Io`] when the file is there and cannot be read.
pub fn read(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(Error::io(path)(error)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

    use std::path::Path;

    use super::{
        Payload, REFUSED, SESSION_START, STOP, WORKTREE_CREATE, WORKTREE_REMOVE, acceptable, hooks,
        last_message,
    };
    use crate::adapters::settings::MARKER;

    /// The payload the desktop application sends when it makes a worktree, as it was
    /// recorded: no transcript, no scratchpad directory, and a slug it derived from the
    /// opening prompt.
    const DESKTOP_CREATE: &str = r#"{"session_id":"65c3d11f-676b-40d3-962a-d4f001021ba4","transcript_path":"","cwd":"/Users/dev/project","hook_event_name":"WorktreeCreate","name":"say-hi-6fac65"}"#;

    /// The payload a session start sends. The identifier is not the one the create
    /// carried, and nothing here reads it.
    const DESKTOP_START: &str = r#"{"session_id":"8e18d129-e9d9-4893-b8dc-429739268683","transcript_path":"","cwd":"/Users/dev/project-wt-60452","hook_event_name":"SessionStart","source":"startup"}"#;

    #[test]
    fn the_payload_the_desktop_sends_is_read_for_what_it_carries() {
        let payload: Payload = serde_json::from_str(DESKTOP_CREATE).unwrap();
        assert_eq!(payload.hook_event_name, WORKTREE_CREATE);
        assert_eq!(payload.cwd, Path::new("/Users/dev/project"));
        assert_eq!(payload.slug().unwrap().as_str(), "say-hi-6fac65");
        assert_eq!(payload.objective().unwrap().as_str(), "say-hi-6fac65");
        assert!(payload.transcript_path.is_empty(), "the desktop sends no transcript");
    }

    #[test]
    fn a_payload_with_a_field_nodal_has_never_seen_is_still_read() {
        let payload: Payload =
            serde_json::from_str(r#"{"cwd":"/tmp/x","name":"a-slug","invented":42}"#).unwrap();
        assert_eq!(payload.cwd, Path::new("/tmp/x"));
        assert_eq!(payload.slug().unwrap().as_str(), "a-slug");
    }

    #[test]
    fn a_payload_that_is_not_json_is_an_empty_payload_and_not_a_panic() {
        let payload = Payload::read("not json".as_bytes()).unwrap();
        assert!(payload.cwd.as_os_str().is_empty());
        assert!(payload.slug().is_none());
    }

    #[test]
    fn the_refusal_is_a_path_claude_code_would_not_accept() {
        assert!(!acceptable(Path::new(REFUSED)), "the refusal would be taken for a directory");
    }

    #[test]
    fn only_an_absolute_path_with_no_dot_segment_is_answered_with() {
        assert!(acceptable(Path::new("/home/dev/.nodal/p/e/01J")));
        assert!(!acceptable(Path::new("home/dev/unit")));
        assert!(!acceptable(Path::new("/home/dev/../dev/unit")));
        assert!(!acceptable(Path::new("/home/dev/./unit")));
        assert!(!acceptable(Path::new("")));
    }

    #[test]
    fn every_installed_command_names_nodal_and_no_path_of_this_machine() {
        let installed = hooks();
        let events: Vec<&str> = installed.iter().map(|hook| hook.event).collect();
        assert_eq!(events, vec![WORKTREE_CREATE, SESSION_START, STOP, WORKTREE_REMOVE]);
        for hook in &installed {
            assert!(hook.command.contains(MARKER), "{}", hook.command);
            let absolute: Vec<&str> = hook
                .command
                .split(|character: char| character.is_whitespace() || character == '>')
                .filter(|token| token.starts_with('/'))
                .collect();
            assert_eq!(
                absolute,
                vec!["/dev/null"],
                "a command names a directory of this machine: {}",
                hook.command
            );
            assert!(hook.command.contains("command -v nodal"), "{}", hook.command);
        }
    }

    #[test]
    fn only_the_provider_answers_when_nodal_is_missing() {
        let installed = hooks();
        let provider = &installed[0];
        assert!(provider.command.contains(REFUSED), "{}", provider.command);
        assert!(provider.timeout.is_some(), "a create may have to build a base first");
        for observer in &installed[1..] {
            assert!(!observer.command.contains(REFUSED), "{}", observer.command);
            assert!(!observer.command.contains("exit 1"), "{}", observer.command);
        }
    }

    #[test]
    fn the_last_message_comes_from_the_payload_when_the_payload_carries_one() {
        let payload = Payload {
            last_assistant_message: String::from("  the parser still fails on two-digit years  "),
            ..Payload::default()
        };
        assert_eq!(
            last_message(&payload).unwrap(),
            "the parser still fails on two-digit years",
            "the message was not trimmed"
        );
    }

    #[test]
    fn a_session_that_sent_neither_a_message_nor_a_transcript_is_not_recorded() {
        let payload: Payload = serde_json::from_str(DESKTOP_START).unwrap();
        assert!(last_message(&payload).is_none(), "the desktop's silence became a handoff");
    }

    #[test]
    fn the_last_thing_the_session_said_is_read_out_of_a_transcript() {
        let directory = tempfile::TempDir::new().unwrap();
        let file = directory.path().join("session.jsonl");
        std::fs::write(
            &file,
            "{\"type\":\"user\",\"message\":{\"content\":\"do the thing\"}}\n\
             {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"first\"}]}}\n\
             {\"type\":\"assistant\",\"isSidechain\":true,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"a subagent\"}]}}\n\
             not a line of json\n\
             {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"last\"}]}}\n",
        )
        .unwrap();
        let payload = Payload { transcript_path: file.display().to_string(), ..Payload::default() };
        assert_eq!(last_message(&payload).unwrap(), "last");
    }

    #[test]
    fn a_transcript_that_is_not_there_is_silence_and_not_a_failure() {
        let payload = Payload {
            transcript_path: String::from("/nowhere/at/all.jsonl"),
            ..Payload::default()
        };
        assert!(last_message(&payload).is_none());
    }
}
