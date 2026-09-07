//! Why a nested worktree was made, recovered from the records Claude Code keeps.
//!
//! A worktree another tool made carries no statement of what it is for. Its branch name
//! is a slug and its directory is a slug, and a person looking at a list of four of them
//! three weeks later cannot tell which one holds the work they remember. That is the
//! thing that makes a machine hard to clean up: not the disk, but not knowing what a
//! directory was.
//!
//! Claude Code writes a record of every session, one file of JSON lines under
//! `~/.claude/projects/<directory>/<session>.jsonl`, and the first user record of a
//! session holds the prompt the session started from. That prompt is the intent. This
//! module reads it, and reads nothing else out of those files.
//!
//! The directory a session belongs to is named after the working directory it ran in,
//! with every character that is not a letter, a digit or a dash replaced by a dash. The
//! rule loses information — `/a/b-c` and `/a/b/c` encode to one name — so the `cwd` a
//! record carries is read as well, and a record whose `cwd` is somewhere else entirely
//! is not this worktree's intent.
//!
//! A record whose `cwd` is a directory **above** the worktree is used. That is the
//! ordinary case and not an exception: a person opens the session in the checkout, the
//! session makes the worktree and moves into it, and the opening prompt was written
//! before the move. The prompt is still what the worktree was made for. A record with
//! no `cwd` is used as it is, because an older version of the tool wrote it.
//!
//! Nothing here is required. A machine with no Claude Code, a worktree another tool
//! made, and a session file that has been cleaned up all give the same answer, which is
//! no intent, and a report prints a row without one.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The variable Claude Code reads for the directory it keeps its records in.
pub const CONFIG_VAR: &str = "CLAUDE_CONFIG_DIR";

/// That directory's name under the user's own directory, when the variable is not set.
const CONFIG_NAME: &str = ".claude";

/// The directory the per-project session directories sit under.
const PROJECTS: &str = "projects";

/// The extension of one session's file.
const EXTENSION: &str = "jsonl";

/// How many lines of a session file are read before the search for the first prompt
/// gives up. The first user record is within the first few lines of every file this
/// reads; the limit is what stops a damaged file from being read to its end.
const HEAD_LINES: usize = 64;

/// Where Claude Code keeps its records on this machine, or `None` when nothing says.
#[must_use]
pub fn config_directory() -> Option<PathBuf> {
    if let Some(moved) = std::env::var_os(CONFIG_VAR).filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(moved));
    }
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
    Some(PathBuf::from(home).join(CONFIG_NAME))
}

/// The first prompt of the earliest session that ran in `worktree`, when there is one.
///
/// `config` is the directory [`config_directory`] answers with. Reading it is tolerant
/// throughout: a missing directory, an unreadable file and a line that is not the
/// document this expects each mean no intent rather than an error.
#[must_use]
pub fn recover(config: &Path, worktree: &Path) -> Option<String> {
    let directory = config.join(PROJECTS).join(encode(worktree));
    let mut found: Vec<Started> = Vec::new();
    for entry in std::fs::read_dir(&directory).ok()?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == EXTENSION)
            && let Some(started) = first_prompt(&path, worktree)
        {
            found.push(started);
        }
    }
    found.sort_by(|left, right| left.at.cmp(&right.at).then_with(|| left.file.cmp(&right.file)));
    found.into_iter().next().map(|started| started.prompt)
}

/// A session's opening prompt, and what orders it against the other sessions.
struct Started {
    /// When the prompt was sent, as the record writes it. RFC 3339 text sorts by
    /// instant, so this is compared as it was read.
    at: String,
    /// The file it came from, which breaks a tie so that two runs answer the same.
    file: PathBuf,
    /// The prompt itself.
    prompt: String,
}

/// The first user prompt in one session file.
fn first_prompt(path: &Path, worktree: &Path) -> Option<Started> {
    let file = std::fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(HEAD_LINES) {
        let Ok(line) = line else { return None };
        let Ok(record) = serde_json::from_str::<Record>(&line) else { continue };
        if !record.is_opening_prompt(worktree) {
            continue;
        }
        let Some(prompt) = record.message.and_then(|message| message.content.text()) else {
            continue;
        };
        let prompt = prompt.trim();
        if prompt.is_empty() {
            continue;
        }
        return Some(Started {
            at: record.timestamp.unwrap_or_default(),
            file: path.to_owned(),
            prompt: prompt.to_owned(),
        });
    }
    None
}

/// The part of one session record this module reads.
#[derive(Debug, Deserialize)]
struct Record {
    /// What kind of record it is. A prompt is `user`.
    #[serde(rename = "type", default)]
    kind: String,
    /// The directory the session ran in, which newer records carry.
    #[serde(default)]
    cwd: Option<PathBuf>,
    /// Whether the record belongs to a sub-agent rather than to the session itself.
    #[serde(rename = "isSidechain", default)]
    sidechain: bool,
    /// Whether the tool wrote the record itself rather than a person.
    #[serde(rename = "isMeta", default)]
    meta: bool,
    /// When it was written.
    #[serde(default)]
    timestamp: Option<String>,
    /// Where the prompt is.
    #[serde(default)]
    message: Option<Message>,
}

impl Record {
    /// Whether this record is the prompt a person opened a session for `worktree` with.
    fn is_opening_prompt(&self, worktree: &Path) -> bool {
        self.kind == "user" && !self.sidechain && !self.meta && self.is_about(worktree)
    }

    /// Whether the directory the record names can be the one this worktree was made in.
    ///
    /// The worktree itself, or anything above it. Above it is the ordinary case: the
    /// session was opened in the checkout and made the worktree from there.
    fn is_about(&self, worktree: &Path) -> bool {
        self.cwd.as_deref().is_none_or(|cwd| worktree.starts_with(cwd))
    }
}

/// The message of a user record.
#[derive(Debug, Deserialize)]
struct Message {
    /// Its content, which the tool writes either as text or as a list of blocks.
    #[serde(default)]
    content: Content,
}

/// What a user record holds: one string, or a list of blocks of which the first text
/// block is the prompt.
#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum Content {
    /// The prompt as one string.
    Text(String),
    /// The prompt as blocks.
    Blocks(Vec<Block>),
    /// Anything else this module does not read.
    #[default]
    Other,
}

impl Content {
    /// The prompt, when the content holds one a person wrote.
    ///
    /// Text that opens with `<` is left out. The tool writes its own notes to the same
    /// field in that shape — a command it expanded, a reminder it added — and none of
    /// them is what the session was started to do.
    fn text(self) -> Option<String> {
        let text = match self {
            Self::Text(text) => text,
            Self::Blocks(blocks) => blocks.into_iter().find_map(|block| block.text)?,
            Self::Other => return None,
        };
        (!text.trim_start().starts_with('<')).then_some(text)
    }
}

/// One block of a message written as blocks.
#[derive(Debug, Deserialize)]
struct Block {
    /// Its text, which only a text block has.
    #[serde(default)]
    text: Option<String>,
}

/// The name Claude Code gives the session directory of a working directory.
///
/// Every character that is not a letter, a digit or a dash becomes a dash. The rule is
/// the tool's, not Nodal's, and it is applied here so that Nodal looks in one directory
/// instead of reading every session file on the machine.
#[must_use]
pub fn encode(directory: &Path) -> String {
    directory
        .to_string_lossy()
        .chars()
        .map(
            |character| {
                if character.is_ascii_alphanumeric() || character == '-' { character } else { '-' }
            },
        )
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail by panicking")]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{encode, recover};

    /// A config directory holding one session file for `worktree`.
    fn sessions(worktree: &Path, files: &[(&str, &str)]) -> tempfile::TempDir {
        let config = tempfile::tempdir().unwrap();
        let directory = config.path().join("projects").join(encode(worktree));
        std::fs::create_dir_all(&directory).unwrap();
        for (name, body) in files {
            std::fs::write(directory.join(name), body).unwrap();
        }
        config
    }

    fn prompt_line(cwd: &Path, at: &str, text: &str) -> String {
        format!(
            r#"{{"type":"user","isSidechain":false,"cwd":{cwd},"timestamp":"{at}","message":{{"role":"user","content":{text}}}}}"#,
            cwd = serde_json::to_string(cwd).unwrap(),
            text = serde_json::to_string(text).unwrap(),
        )
    }

    #[test]
    fn the_first_prompt_of_the_earliest_session_is_the_intent() {
        let worktree = PathBuf::from("/home/j/code/app/.claude/worktrees/auth");
        let later = prompt_line(&worktree, "2026-09-02T10:00:00Z", "later work");
        let earlier = prompt_line(&worktree, "2026-08-19T23:37:11Z", "Fix the token refresh");
        let config = sessions(&worktree, &[("b.jsonl", &later), ("a.jsonl", &earlier)]);
        assert_eq!(recover(config.path(), &worktree).as_deref(), Some("Fix the token refresh"));
    }

    #[test]
    fn a_record_that_names_another_directory_is_not_this_worktrees_intent() {
        let worktree = PathBuf::from("/home/j/code/app/.claude/worktrees/auth");
        let other =
            prompt_line(Path::new("/home/j/elsewhere"), "2026-08-19T00:00:00Z", "other work");
        let config = sessions(&worktree, &[("a.jsonl", &other)]);
        assert_eq!(recover(config.path(), &worktree), None);
    }

    /// The real shape: the session was opened in the checkout, and it made the worktree.
    #[test]
    fn a_session_opened_above_the_worktree_is_still_its_intent() {
        let worktree = PathBuf::from("/home/j/code/app/.claude/worktrees/auth");
        let above =
            prompt_line(Path::new("/home/j/code/app"), "2026-08-19T00:00:00Z", "Audit the keys");
        let config = sessions(&worktree, &[("a.jsonl", &above)]);
        assert_eq!(recover(config.path(), &worktree).as_deref(), Some("Audit the keys"));
    }

    #[test]
    fn the_tools_own_notes_are_not_the_intent() {
        let worktree = PathBuf::from("/w");
        let meta =
            prompt_line(&worktree, "2026-08-01T00:00:00Z", "<command-name>/clear</command-name>");
        let real = prompt_line(&worktree, "2026-08-02T00:00:00Z", "Add a retry to the importer");
        let config = sessions(&worktree, &[("a.jsonl", &format!("{meta}\n{real}\n"))]);
        assert_eq!(
            recover(config.path(), &worktree).as_deref(),
            Some("Add a retry to the importer")
        );
    }

    #[test]
    fn a_prompt_written_as_blocks_is_read_as_well() {
        let worktree = PathBuf::from("/w");
        let line = r#"{"type":"user","cwd":"/w","timestamp":"2026-08-01T00:00:00Z","message":{"content":[{"type":"text","text":"Ship the exporter"}]}}"#;
        let config = sessions(&worktree, &[("a.jsonl", line)]);
        assert_eq!(recover(config.path(), &worktree).as_deref(), Some("Ship the exporter"));
    }

    #[test]
    fn a_machine_with_no_records_answers_with_no_intent() {
        let config = tempfile::tempdir().unwrap();
        assert_eq!(recover(config.path(), Path::new("/w")), None);
    }

    #[test]
    fn a_line_that_is_not_a_record_is_stepped_over() {
        let worktree = PathBuf::from("/w");
        let good = prompt_line(&worktree, "2026-08-01T00:00:00Z", "Do the thing");
        let config = sessions(&worktree, &[("a.jsonl", &format!("not json\n{{}}\n{good}\n"))]);
        assert_eq!(recover(config.path(), &worktree).as_deref(), Some("Do the thing"));
    }

    #[test]
    fn the_directory_name_is_the_tools_own_encoding() {
        assert_eq!(
            encode(Path::new("/home/j/code/app/.claude/worktrees/audit-F12-1")),
            "-home-j-code-app--claude-worktrees-audit-F12-1"
        );
    }
}
