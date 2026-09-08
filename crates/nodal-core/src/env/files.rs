//! The activation files: `.nodal/env`, `.envrc`, `.nodal/manifest.toml`.
//!
//! Three files, two consumers. direnv reads `.envrc`, which reads `.nodal/env`. A shell
//! with no direnv gets the same set from `nodal env --export`, which renders the same
//! variables as shell assignments. `.nodal/manifest.toml` is what a tool reads to learn
//! what the directory is, and it holds names only.
//!
//! Two renderings of one set, because the two consumers do not quote alike.
//! [`dotenv`] writes `NAME="value"` with the escapes a dotenv reader decodes.
//! [`export`] writes `export NAME='value'` with the one escape a POSIX shell needs.
//! Writing one form for both would mean picking which consumer to be wrong for.
//!
//! Everything here is idempotent: writing the same activation twice leaves the same
//! bytes. [`WriteFiles`] is the step form, for a lifecycle operation that has to be
//! able to undo it.

use std::path::{Path, PathBuf};

use crate::env::{Activation, secrets};
use crate::lifecycle::Step;
use crate::model::{EnvName, Manifest};
use crate::{Error, Result};

/// The per-unit directory inside a home.
pub const DIR: &str = ".nodal";

/// The dotenv file, relative to a home.
pub const ENV: &str = ".nodal/env";

/// The direnv file, relative to a home.
pub const ENVRC: &str = ".envrc";

/// The manifest, relative to a home.
pub const MANIFEST: &str = ".nodal/manifest.toml";

/// The unit's memory, relative to a home: what `nodal show` writes again each time it
/// is asked, and what an agent reads to learn what the unit is for and what its
/// siblings changed. It sits at the top of the home rather than inside `.nodal/`
/// because it is written for a person and for an agent to open, and both of them look
/// there first.
pub const WORKUNIT: &str = "WORKUNIT.md";

/// The whole of `.envrc`. One line, because direnv is the reader and `.nodal/env` is
/// the content; anything else here would be a second place to keep the truth.
pub const ENVRC_CONTENTS: &str = "dotenv .nodal/env\n";

/// The mode `.nodal/env` is created with. It holds resolved secret values, so it is
/// owner-only for the same reason the per-machine file is.
pub const ENV_MODE: u32 = secrets::OWNER_ONLY;

/// The paths a home's activation writes, and which Git is told to ignore.
///
/// [`WORKUNIT`] is hidden with them and is not one of them: the activation writes it
/// no more than the activation compiles it, and a home whose memory has never been
/// asked for simply has none.
pub const PATHS: &[&str] = &[ENV, ENVRC, MANIFEST];

/// What a home's Git repository is told to leave alone, written to `.git/info/exclude`.
///
/// The unit's own files are not the project's, so they must not appear in
/// `git status`: a unit whose activation files show as untracked is a unit the
/// uniqueness check calls dirty, and no one could ever reclaim it.
///
/// The same rule is what makes adoption in place possible at all. A checkout adopted
/// where it stands is a directory Nodal did not create and a person is working in, and
/// these three lines are why `git status` in it says exactly what it said the moment
/// before it became a unit.
const EXCLUDE_LINES: &[&str] = &["/.nodal/", "/.envrc", "/WORKUNIT.md"];

/// The marker `.git/info/exclude` carries, so a reader can see which lines are Nodal's.
pub const EXCLUDE_MARKER: &str = "# nodal";

/// Write the activation files into `home`.
///
/// # Errors
/// [`Error::Io`] if a file or directory cannot be written, and
/// [`Error::ManifestEncode`] if the manifest cannot be rendered as TOML.
pub fn write(home: &Path, activation: &Activation, manifest: &Manifest) -> Result<()> {
    let directory = home.join(DIR);
    std::fs::create_dir_all(&directory).map_err(Error::io(&directory))?;
    write_owner_only(&home.join(ENV), &dotenv(activation))?;
    write_text(&home.join(ENVRC), ENVRC_CONTENTS)?;
    write_text(&home.join(MANIFEST), &render_manifest(manifest)?)?;
    Ok(())
}

/// Take everything Nodal writes into a home back out of it, leaving the home itself.
///
/// That is the three activation files and the compiled memory ([`WORKUNIT`]) — every
/// path [`hide`] tells Git to ignore, and nothing else. The list is the same one for a
/// reason: a file Nodal hid from `git status` is a file a person would not see left
/// behind, so the two must not drift apart.
///
/// Idempotent: a file that is not there is not an error, and `.nodal/` is removed only
/// when nothing else has been put in it.
///
/// # Errors
/// [`Error::Io`] if a file that is there cannot be removed.
pub fn remove(home: &Path) -> Result<()> {
    for relative in PATHS.iter().chain(std::iter::once(&WORKUNIT)) {
        let path = home.join(relative);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io(&path)(error)),
        }
    }
    let directory = home.join(DIR);
    if std::fs::read_dir(&directory).is_ok_and(|mut entries| entries.next().is_none()) {
        std::fs::remove_dir(&directory).map_err(Error::io(&directory))?;
    }
    Ok(())
}

/// Add the activation paths to `git_dir/info/exclude`, once.
///
/// Returns whether the lines were added, so that a caller can say what it changed.
/// A second call over a file that already carries them adds nothing.
///
/// `git_dir` is the directory Git reads `info/exclude` from, which is the repository's
/// **common** directory. Every worktree of a repository shares one exclude file: a
/// linked worktree's own `.git/worktrees/<name>/info/exclude` is not read at all. So
/// hiding the files of a unit adopted in a nested worktree writes one block into the
/// repository the whole project shares, and the three names it holds are Nodal's own
/// ([`EXCLUDE_LINES`]) rather than anything a project puts in a tree.
///
/// # Errors
/// [`Error::Io`] if the file cannot be read or written.
pub fn hide(git_dir: &Path) -> Result<bool> {
    Ok(!exclude(git_dir, EXCLUDE_LINES)?.is_empty())
}

/// Tell a repository to leave `lines` alone, and say which of them it did not already.
///
/// The file is read before it is written and only the missing lines are added, because
/// a home is told about more paths than the activation over its life: the compiled
/// memory arrives after the create that made the home, and a home written by an older
/// version of Nodal has to be able to learn about it. Adding the whole set again
/// whenever one line is new would make a person's own exclude file grow on every
/// command.
///
/// The marker line is written once, above the first line Nodal adds, so that a person
/// reading the file can see whose lines these are.
///
/// # Errors
/// [`Error::Io`] if the file cannot be read or written.
pub fn exclude<'a>(git_dir: &Path, lines: &[&'a str]) -> Result<Vec<&'a str>> {
    let info = git_dir.join("info");
    let path = info.join("exclude");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(Error::io(&path)(error)),
    };
    let present: Vec<String> = existing.lines().map(|line| line.trim_end().to_owned()).collect();
    let has = |wanted: &str| present.iter().any(|line| line == wanted);
    let adding: Vec<&'a str> = lines.iter().copied().filter(|line| !has(line)).collect();
    if adding.is_empty() {
        return Ok(adding);
    }
    std::fs::create_dir_all(&info).map_err(Error::io(&info))?;
    let marked = has(EXCLUDE_MARKER);
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !marked {
        text.push_str(EXCLUDE_MARKER);
        text.push('\n');
    }
    for line in &adding {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(Error::io(&path))?;
    Ok(adding)
}

/// Take the lines [`hide`] wrote out of `git_dir/info/exclude` again.
///
/// Returns whether there was a block to remove. Every other line of the file is kept as
/// it was written, in the order it was written: this drops the marker line and the
/// lines it introduced, and touches nothing else. A line somebody wrote themselves that
/// is character for character one of Nodal's goes with them, which leaves the file
/// saying what Nodal's own block was already making it say.
///
/// This is the undo of an adoption. A checkout Nodal did not create is a directory it
/// must leave as it found it, so an adoption that fails half-way through takes its
/// exclusions back out rather than leaving a person's own repository carrying rules
/// for a unit that does not exist.
///
/// # Errors
/// [`Error::Io`] if the file is there and cannot be read or written.
pub fn unhide(git_dir: &Path) -> Result<bool> {
    let path = git_dir.join("info").join("exclude");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(Error::io(&path)(error)),
    };
    let kept: Vec<&str> = existing
        .lines()
        .filter(|line| line.trim_end() != EXCLUDE_MARKER)
        .filter(|line| !EXCLUDE_LINES.contains(&line.trim_end()))
        .collect();
    if kept.len() == existing.lines().count() {
        return Ok(false);
    }
    let mut text = kept.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(Error::io(&path))?;
    Ok(true)
}

/// The dotenv rendering: what `.nodal/env` holds and what direnv reads.
///
/// Values are double-quoted with `\`, `"`, `$` and a backtick escaped, so a reader
/// neither expands a variable reference nor ends the value early.
#[must_use]
pub fn dotenv(activation: &Activation) -> String {
    let mut text = String::from(HEADER);
    for var in &activation.vars {
        text.push_str(var.name().as_str());
        text.push('=');
        text.push('"');
        text.push_str(&escape_dotenv(var.expose()));
        text.push_str("\"\n");
    }
    text
}

/// The shell rendering: what `nodal env --export` prints for a script to `eval`.
///
/// Single quotes, with the one escape a POSIX shell has for a single quote inside them,
/// so no character in a value is interpreted.
#[must_use]
pub fn export(activation: &Activation) -> String {
    render_export(activation.vars.iter().map(|var| (var.name().as_str(), var.expose())))
}

/// The same rendering, over the pairs read back out of a home's `.nodal/env`.
#[must_use]
pub fn export_lines(pairs: &[(EnvName, String)]) -> String {
    render_export(pairs.iter().map(|(name, value)| (name.as_str(), value.as_str())))
}

/// One `export NAME='value'` line per pair.
fn render_export<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut text = String::new();
    for (name, value) in pairs {
        text.push_str("export ");
        text.push_str(name);
        text.push_str("='");
        text.push_str(&value.replace('\'', "'\\''"));
        text.push_str("'\n");
    }
    text
}

/// Read a home's `.nodal/env` back as name and value pairs.
///
/// This is the reverse of [`dotenv`] and nothing else: it reads the file Nodal wrote,
/// so that `nodal env --export` renders exactly the set the home was activated with
/// without asking a registry or a secret source again.
///
/// # Errors
/// [`Error::Io`] if the file cannot be read.
pub fn read_dotenv(home: &Path) -> Result<Vec<(EnvName, String)>> {
    let path = home.join(ENV);
    let text = std::fs::read_to_string(&path).map_err(Error::io(&path))?;
    Ok(parse_dotenv(&text))
}

/// The assignments in the dotenv form [`dotenv`] writes.
fn parse_dotenv(text: &str) -> Vec<(EnvName, String)> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .filter_map(|(name, value)| Some((EnvName::parse(name.trim()).ok()?, unescape(value))))
        .collect()
}

/// Undo [`escape_dotenv`] on one double-quoted value.
fn unescape(value: &str) -> String {
    let inner = value.trim().trim_start_matches('"').trim_end_matches('"');
    let mut text = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            text.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => text.push('\n'),
            Some(escaped) => text.push(escaped),
            None => text.push('\\'),
        }
    }
    text
}

/// The home `start` is in: the nearest directory at or above it with a manifest.
///
/// # Errors
/// [`Error::NotAHome`] when no directory above `start` has one.
pub fn find_home(start: &Path) -> Result<PathBuf> {
    for directory in start.ancestors() {
        if directory.join(MANIFEST).is_file() {
            return Ok(directory.to_path_buf());
        }
    }
    Err(Error::NotAHome { path: PathBuf::from(start) })
}

/// Read a home's manifest.
///
/// # Errors
/// [`Error::Io`] if the file cannot be read, and [`Error::Recipe`] if it is not a
/// manifest.
pub fn read_manifest(home: &Path) -> Result<Manifest> {
    let path = home.join(MANIFEST);
    let text = std::fs::read_to_string(&path).map_err(Error::io(&path))?;
    toml::from_str(&text).map_err(|source| Error::Recipe { path, source: Box::new(source) })
}

/// What `.nodal/env` says about itself, so a person who opens it knows not to edit it.
const HEADER: &str = "# Written by nodal. Edits are lost the next time the unit is \
                      activated.\n# Per-machine values belong in ~/.nodal/secrets.env.\n";

/// Escape a value for the double-quoted form of a dotenv file.
fn escape_dotenv(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '"' | '$' | '`' => {
                text.push('\\');
                text.push(character);
            }
            '\n' => text.push_str("\\n"),
            other => text.push(other),
        }
    }
    text
}

/// The manifest as the TOML that goes in the file.
fn render_manifest(manifest: &Manifest) -> Result<String> {
    toml::to_string_pretty(manifest).map_err(|source| Error::ManifestEncode { source })
}

/// Write a file whose content is not secret.
fn write_text(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).map_err(Error::io(path))
}

/// Write a file at owner-only permissions, whether or not it is already there.
#[cfg(unix)]
fn write_owner_only(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(ENV_MODE)
        .open(path)
        .map_err(Error::io(path))?;
    file.write_all(text.as_bytes()).map_err(Error::io(path))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(ENV_MODE))
        .map_err(Error::io(path))
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, text: &str) -> Result<()> {
    write_text(path, text)
}

/// Writing a home's activation files as one step of an operation.
///
/// `apply` writes all three; `undo` removes them. Both are safe to repeat, which is
/// what the runner requires of every step (`docs/code-structure.md`).
pub struct WriteFiles {
    /// The home the files are written into.
    pub home: PathBuf,
    /// The variables and the report.
    pub activation: Activation,
    /// The manifest that goes beside them.
    pub manifest: Manifest,
}

impl Step for WriteFiles {
    fn key(&self) -> String {
        String::from("env.write-files")
    }

    fn apply(&self) -> Result<()> {
        write(&self.home, &self.activation, &self.manifest)
    }

    fn undo(&self) -> Result<()> {
        remove(&self.home)
    }
}

/// Telling a home's repository to leave Nodal's own files alone, as one step.
///
/// The Git directory is asked for inside `apply` rather than carried in the step,
/// because a plan holds only what the journal can write down and a Git directory is
/// something the repository answers. It is the common directory, for the reason
/// [`hide`] gives: it is the only `info/exclude` Git reads.
pub struct Hide {
    /// The home whose repository is told.
    pub home: PathBuf,
}

impl Step for Hide {
    fn key(&self) -> String {
        String::from("git.hide")
    }

    fn apply(&self) -> Result<()> {
        hide(&exclude_dir(&self.home)?).map(drop)
    }

    fn undo(&self) -> Result<()> {
        unhide(&exclude_dir(&self.home)?).map(drop)
    }
}

/// The directory whose `info/exclude` Git reads for the checkout at `home`.
///
/// # Errors
/// [`Error::NotARepository`] when the home is not a checkout, and whatever Git
/// reported.
pub fn exclude_dir(home: &Path) -> Result<PathBuf> {
    Ok(crate::git::Git::open(home)?.layout()?.common_dir)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{dotenv, escape_dotenv, export, hide, unhide};
    use crate::env::{Activation, EnvVar, secrets};
    use crate::model::{EnvName, Missing, Origin, Want};

    fn activation() -> Activation {
        let var = |name: &str, value: &str| {
            EnvVar::new(
                EnvName::parse(name).unwrap(),
                Origin::Generated,
                secrets::Value::new(value),
            )
        };
        Activation {
            vars: vec![var("PORT", "3011"), var("SESSION_SECRET", "a $b `c` \"d\" 'e'")],
            missing: vec![Missing {
                name: EnvName::parse("RESEND_API_KEY").unwrap(),
                want: Want::Secret,
            }],
        }
    }

    #[test]
    fn the_dotenv_form_quotes_what_a_reader_would_otherwise_expand() {
        let text = dotenv(&activation());
        assert!(text.contains("PORT=\"3011\"\n"), "{text}");
        assert!(text.contains(r#"SESSION_SECRET="a \$b \`c\` \"d\" 'e'""#), "{text}");
    }

    #[test]
    fn the_export_form_quotes_what_a_shell_would_otherwise_interpret() {
        let text = export(&activation());
        assert!(text.contains("export PORT='3011'\n"), "{text}");
        assert!(text.contains(r#"export SESSION_SECRET='a $b `c` "d" '\''e'\'''"#), "{text}");
    }

    #[test]
    fn a_newline_in_a_value_stays_on_one_line_of_the_dotenv_file() {
        assert_eq!(escape_dotenv("one\ntwo"), "one\\ntwo");
    }

    /// What an adoption owes a checkout it did not create: the file as it found it.
    #[test]
    fn hiding_and_unhiding_leave_what_a_person_wrote_untouched() {
        let git_dir = tempfile::tempdir().unwrap();
        let path = git_dir.path().join("info").join("exclude");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = "# their own rules\n/scratch\n";
        std::fs::write(&path, theirs).unwrap();

        assert!(hide(git_dir.path()).unwrap(), "the lines were added");
        assert!(!hide(git_dir.path()).unwrap(), "adding them twice adds nothing");
        let hidden = std::fs::read_to_string(&path).unwrap();
        assert!(hidden.contains("/.nodal/"), "{hidden}");

        assert!(unhide(git_dir.path()).unwrap(), "the lines were removed");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);
        assert!(!unhide(git_dir.path()).unwrap(), "removing them twice removes nothing");
    }

    #[test]
    fn unhiding_a_repository_that_was_never_hidden_changes_nothing() {
        let git_dir = tempfile::tempdir().unwrap();
        assert!(!unhide(git_dir.path()).unwrap());
    }
}
