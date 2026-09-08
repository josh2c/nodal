//! `.claude/settings.json`: putting Nodal's hooks in it, and taking exactly them out.
//!
//! # Whose file this is
//!
//! The file belongs to the project, not to Nodal. It may already hold permissions, an
//! environment, and hooks somebody else installed, and every one of those has to
//! survive both halves of this module. It is also a file a person may commit, so
//! nothing Nodal writes into it names a path that is only true on one machine: every
//! command is the word `nodal` and a subcommand, and reads the same on every machine.
//!
//! # How Nodal's hooks are recognised again
//!
//! There are no comments in JSON, so there is no marked block to remove the way
//! [`crate::setup::rc`] removes one from a start-up file. Two things stand in for it.
//!
//! The first is the text of the commands. Every command Nodal writes carries
//! [`MARKER`], so an entry can always be told from somebody else's, whichever version
//! of Nodal wrote it.
//!
//! The second is what makes the round trip exact: **what is added is one contiguous
//! region of text that Nodal can write again.** [`add`] splices that region into the
//! file and changes no other byte; [`remove`] finds the same region and takes it out.
//! So a settings file that had Nodal's hooks added and then removed is byte for byte
//! the file it was, whatever its indentation, key order or content — the same rule
//! [`crate::setup::rc`] holds to, for the same reason.
//!
//! A file that has been reformatted since the install no longer holds that region. It
//! is not left with Nodal's hooks in it: removal falls back to reading the JSON,
//! dropping the entries that carry the marker, and writing the document back. That
//! answer is correct but it is not byte-identical, and it is the only path here that is
//! not.
//!
//! A file that holds nothing but what Nodal added comes back an empty document, and the
//! caller removes it rather than leaving it ([`is_empty`]).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::{Error, Result};

/// The directory the settings file is in, relative to a project root.
pub const DIR: &str = ".claude";

/// The settings file itself, relative to a project root.
pub const FILE: &str = ".claude/settings.json";

/// The token every command Nodal writes into the file carries.
///
/// It is what a fallback removal matches on, and it is the command a person reads.
pub const MARKER: &str = "nodal claude-code";

/// The key the hooks live under, at the top level and inside one group.
const HOOKS: &str = "hooks";

/// The key one hook entry's command lives under.
const COMMAND: &str = "command";

/// One indent level, as Claude Code writes the file.
const STEP: &str = "  ";

/// The indent one level in from a two-space top level, for a file that says nothing
/// about its own indentation because it is all on one line.
const ONE_LEVEL: &str = "  ";

/// The same, two levels in: where an event's member sits inside the `hooks` object.
const TWO_LEVELS: &str = "    ";

/// One hook Nodal installs: the event it answers, and what runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hook {
    /// The event name Claude Code fires, spelled as Claude Code spells it.
    pub event: &'static str,
    /// The shell command, carrying [`MARKER`].
    pub command: String,
    /// How long the command may take, in seconds, when it is worth saying.
    pub timeout: Option<u32>,
}

/// Where the settings file of the project at `root` is.
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

/// `text` with `hooks` installed, or `None` when it already reads exactly that way.
///
/// Installing twice is installing once. A command whose text has changed since the last
/// install replaces the one that is there rather than joining it, so a project never
/// carries two Nodal entries for one event.
///
/// # Errors
/// [`Error::InvalidValue`] when the file is not a JSON object.
pub fn add(text: &str, hooks: &[Hook]) -> Result<Option<String>> {
    let base = match remove(text, hooks) {
        Some(stripped) => stripped,
        None => text.to_owned(),
    };
    let document = parse(&base)?;
    let installed = splice(&base, &document, hooks);
    Ok((installed != text).then_some(installed))
}

/// `text` with everything Nodal added taken out, or `None` when it added nothing.
///
/// The inverse of [`add`] byte for byte where the region is still there, and a
/// re-rendering of the document where it is not.
#[must_use]
pub fn remove(text: &str, hooks: &[Hook]) -> Option<String> {
    if let Some(exact) = cut(text, hooks) {
        return Some(exact);
    }
    if !holds_hooks(text) {
        return None;
    }
    let mut document = parse(text).ok()?;
    strip(&mut document);
    Some(render(&document))
}

/// Whether a document holds nothing at all.
///
/// A file that reads this way after a [`remove`] held nothing but what Nodal added, and
/// is removed rather than left behind.
#[must_use]
pub fn is_empty(text: &str) -> bool {
    parse(text).is_ok_and(|document| document.is_empty())
}

/// Whether `text` holds a hook Nodal wrote.
///
/// A file that is not a JSON object holds none: Nodal did not write it, and an
/// uninstall says nothing about a file it cannot read.
#[must_use]
pub fn holds_hooks(text: &str) -> bool {
    parse(text).is_ok_and(|document| holds_marker(&document))
}

/// Every event of `text` that Claude Code would fire at something other than Nodal.
///
/// This is what an uninstall shows a person before it edits the file: what stays.
#[must_use]
pub fn other_events(text: &str) -> Vec<String> {
    let Ok(document) = parse(text) else { return Vec::new() };
    let Some(events) = document.get(HOOKS).and_then(Value::as_object) else { return Vec::new() };
    events.iter().filter(|(_, groups)| !all_nodal(groups)).map(|(event, _)| event.clone()).collect()
}

// The region: writing it, and finding it again.

/// How the region ends, which is the one thing about it that the file decides.
///
/// A region that goes in front of members the file already had ends with the comma that
/// separates it from them. A region that is the whole of its object ends with the
/// newline that puts the closing brace on a line of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ends {
    /// The object already had members.
    Comma,
    /// The object was empty.
    Newline,
}

impl Ends {
    /// What the region ends with.
    const fn text(self) -> &'static str {
        match self {
            Self::Comma => ",",
            Self::Newline => "\n",
        }
    }

    /// Which one an object of `count` members calls for.
    const fn of(count: usize) -> Self {
        if count == 0 { Self::Newline } else { Self::Comma }
    }
}

/// `text` with the region for `hooks` spliced in.
///
/// Into the `hooks` object the file already has, when it has one; into the top-level
/// object otherwise; and into `{}` when there is no file yet.
fn splice(text: &str, document: &Map<String, Value>, hooks: &[Hook]) -> String {
    let (before, after) = if text.trim().is_empty() { ("{", "}\n") } else { (text, "") };
    let (at, region) = if let Some(at) = opening(before, HOOKS) {
        let count = document.get(HOOKS).and_then(Value::as_object).map_or(0, Map::len);
        (at, events(hooks, &indent_at(before, at, TWO_LEVELS), Ends::of(count)))
    } else {
        let at = opening_brace(before).unwrap_or(before.len());
        let indent = indent_at(before, at, ONE_LEVEL);
        (at, object(hooks, &indent, Ends::of(document.len())))
    };
    format!("{}{region}{}{after}", &before[..at], &before[at..])
}

/// `text` with a region Nodal wrote taken out, or `None` when it holds none.
///
/// Every region this module could have written is generated again and looked for.
/// There are four: two places, and two endings.
fn cut(text: &str, hooks: &[Hook]) -> Option<String> {
    let region =
        candidates(text, hooks).into_iter().find(|region| text.contains(region.as_str()))?;
    Some(text.replacen(&region, "", 1))
}

/// Every region this module could have written into `text`, widest first.
///
/// The whole `hooks` member is looked for before the members inside it, because a file
/// that got the whole member holds the members too and taking only those out would
/// leave an empty `hooks` object behind.
fn candidates(text: &str, hooks: &[Hook]) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(at) = opening_brace(text) {
        let indent = indent_at(text, at, ONE_LEVEL);
        found.push(object(hooks, &indent, Ends::Comma));
        found.push(object(hooks, &indent, Ends::Newline));
    }
    if let Some(at) = opening(text, HOOKS) {
        let indent = indent_at(text, at, TWO_LEVELS);
        found.push(events(hooks, &indent, Ends::Comma));
        found.push(events(hooks, &indent, Ends::Newline));
    }
    found
}

/// The region as one member per event, for an object that already exists.
fn events(hooks: &[Hook], indent: &str, ends: Ends) -> String {
    let members: Vec<String> =
        hooks.iter().map(|hook| format!("{indent}{}", member(hook, indent))).collect();
    format!("\n{}{}", members.join(",\n"), ends.text())
}

/// The region as the whole `hooks` member, for a file that has none.
fn object(hooks: &[Hook], indent: &str, ends: Ends) -> String {
    let inner = format!("{indent}{STEP}");
    let members: Vec<String> =
        hooks.iter().map(|hook| format!("{inner}{}", member(hook, &inner))).collect();
    format!("\n{indent}\"{HOOKS}\": {{\n{}\n{indent}}}{}", members.join(",\n"), ends.text())
}

/// One event's member: its name, and the one group Nodal installs for it.
fn member(hook: &Hook, indent: &str) -> String {
    let groups = Value::Array(vec![group(hook)]);
    format!("\"{}\": {}", hook.event, reindent(&pretty(&groups), indent))
}

/// One group holding one command, in the shape Claude Code reads.
fn group(hook: &Hook) -> Value {
    let mut entry = Map::new();
    entry.insert(String::from("type"), Value::String(String::from(COMMAND)));
    entry.insert(String::from(COMMAND), Value::String(hook.command.clone()));
    if let Some(seconds) = hook.timeout {
        entry.insert(String::from("timeout"), Value::from(seconds));
    }
    let mut group = Map::new();
    group.insert(String::from(HOOKS), Value::Array(vec![Value::Object(entry)]));
    Value::Object(group)
}

/// A value as two-space JSON, which is what Claude Code writes.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| String::from("[]"))
}

/// Every line after the first, moved right by `indent`.
fn reindent(text: &str, indent: &str) -> String {
    text.replace('\n', &format!("\n{indent}"))
}

/// The document as the bytes this module writes when it has to write the whole of it.
fn render(document: &Map<String, Value>) -> String {
    serde_json::to_string_pretty(document)
        .map_or_else(|_| String::from("{}"), |text| format!("{text}\n"))
}

// Reading the file: what it holds, and where its objects open.

/// The file as an object. A file that is not there, or is only whitespace, is `{}`.
fn parse(text: &str) -> Result<Map<String, Value>> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(not_an_object(String::from("it holds something other than an object"))),
        Err(error) => Err(not_an_object(error.to_string())),
    }
}

/// The one error this module reports about a file it will not touch.
fn not_an_object(why: String) -> Error {
    Error::InvalidValue { kind: "claude code settings", value: why }
}

/// The offset just after the brace that opens the top-level object.
fn opening_brace(text: &str) -> Option<usize> {
    text.find('{').map(|at| at + 1)
}

/// The offset just after the brace that opens the top-level `key` object.
///
/// The scan reads JSON only as far as it has to: it tracks whether it is inside a
/// string, so that a brace or a key name in somebody's command text is not mistaken for
/// structure, and it counts depth, so that a `hooks` key inside a group is not mistaken
/// for the top-level one.
fn opening(text: &str, key: &str) -> Option<usize> {
    let quoted = format!("\"{key}\"");
    let mut scan = Scan::default();
    let mut wanted = false;
    for (at, character) in text.char_indices() {
        if scan.in_string(character) {
            continue;
        }
        match character {
            '{' if wanted && scan.depth == 1 => return Some(at + 1),
            '{' | '[' => scan.depth += 1,
            '}' | ']' => scan.depth -= 1,
            ',' => wanted = false,
            ':' if scan.depth == 1 => wanted = text[..at].trim_end().ends_with(&quoted),
            _ => {}
        }
    }
    None
}

/// Where a scan of the text is: how deep, and whether inside a string.
#[derive(Debug, Default)]
struct Scan {
    /// How many objects and arrays are open.
    depth: i32,
    /// Whether the scan is inside a string literal.
    string: bool,
    /// Whether the last character inside a string was a backslash.
    escaped: bool,
}

impl Scan {
    /// Take one character, and say whether it is inside a string and so not structure.
    fn in_string(&mut self, character: char) -> bool {
        if self.string {
            self.escaped = !self.escaped && character == '\\';
            self.string = self.escaped || character != '"';
            return true;
        }
        if character == '"' {
            self.string = true;
            self.escaped = false;
            return true;
        }
        false
    }
}

/// The indentation of the line after `at`, which is what the file uses one level in.
///
/// A file that says nothing — an object opened and closed on one line — gets
/// `fallback`, which is what a two-space file would have used there.
fn indent_at(text: &str, at: usize, fallback: &str) -> String {
    let rest = text.get(at..).unwrap_or_default();
    let Some(line) = rest.split('\n').nth(1) else { return String::from(fallback) };
    let indent: String = line.chars().take_while(|character| *character == ' ').collect();
    if indent.is_empty() { String::from(fallback) } else { indent }
}

// The fallback: reading the document and dropping what carries the marker.

/// Take out every hook entry whose command carries [`MARKER`], and every container the
/// removal leaves empty.
fn strip(document: &mut Map<String, Value>) {
    let Some(Value::Object(events)) = document.get_mut(HOOKS) else { return };
    for groups in events.values_mut() {
        let Some(list) = groups.as_array_mut() else { continue };
        for group in list.iter_mut() {
            if let Some(inner) = group.get_mut(HOOKS).and_then(Value::as_array_mut) {
                inner.retain(|hook| !is_nodal(hook));
            }
        }
        list.retain(|group| !is_empty_group(group));
    }
    events.retain(|_, groups| !groups.as_array().is_some_and(Vec::is_empty));
    if events.is_empty() {
        document.remove(HOOKS);
    }
}

/// Whether a group has no hooks left in it.
fn is_empty_group(group: &Value) -> bool {
    group.get(HOOKS).and_then(Value::as_array).is_some_and(Vec::is_empty)
}

/// Whether one hook entry is one Nodal wrote.
fn is_nodal(hook: &Value) -> bool {
    hook.get(COMMAND).and_then(Value::as_str).is_some_and(|command| command.contains(MARKER))
}

/// Whether every hook of an event is one Nodal wrote.
fn all_nodal(groups: &Value) -> bool {
    inner_hooks(groups).all(is_nodal)
}

/// Whether any hook of the document is one Nodal wrote.
fn holds_marker(document: &Map<String, Value>) -> bool {
    document
        .get(HOOKS)
        .and_then(Value::as_object)
        .is_some_and(|events| events.values().any(|groups| inner_hooks(groups).any(is_nodal)))
}

/// Every hook entry under one event, whatever groups it is spread over.
fn inner_hooks(groups: &Value) -> impl Iterator<Item = &Value> {
    groups
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| group.get(HOOKS).and_then(Value::as_array))
        .flatten()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

    use super::{Hook, MARKER, add, holds_hooks, is_empty, other_events, remove};

    /// Two hooks, of the two shapes: one that states a timeout and one that does not.
    fn hooks() -> Vec<Hook> {
        vec![
            Hook {
                event: "WorktreeCreate",
                command: String::from("exec nodal claude-code worktree-create"),
                timeout: Some(900),
            },
            Hook {
                event: "SessionStart",
                command: String::from("exec nodal claude-code session-start"),
                timeout: None,
            },
        ]
    }

    /// Install and uninstall, as the two functions do it.
    fn round_trip(before: &str) -> String {
        let installed = add(before, &hooks()).unwrap().expect("the install did nothing");
        assert!(holds_hooks(&installed), "{installed}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&installed).is_ok(),
            "the install wrote something that is not JSON: {installed}"
        );
        remove(&installed, &hooks()).expect("the removal found nothing")
    }

    #[test]
    fn a_settings_file_is_byte_identical_after_an_install_and_an_uninstall() {
        for before in [
            "{}",
            "{}\n",
            "{\n  \"permissions\": {\n    \"allow\": []\n  }\n}\n",
            "{\n  \"hooks\": {}\n}\n",
            "{\"permissions\":{\"allow\":[]}}",
            "{\n    \"env\": {\n        \"TZ\": \"UTC\"\n    }\n}\n",
        ] {
            assert_eq!(round_trip(before), before, "{before:?} did not survive the round trip");
        }
    }

    #[test]
    fn a_project_with_no_settings_file_is_left_with_no_settings_file() {
        let installed = add("", &hooks()).unwrap().unwrap();
        assert!(installed.starts_with("{\n"), "{installed}");
        let removed = remove(&installed, &hooks()).unwrap();
        assert!(is_empty(&removed), "the file was left holding something: {removed:?}");
    }

    #[test]
    fn a_hook_somebody_else_installed_survives_both_halves() {
        let theirs = "{\n  \"hooks\": {\n    \"SessionStart\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"./scripts/greet.sh\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
        let installed = add(theirs, &hooks()).unwrap().unwrap();
        assert!(installed.contains("./scripts/greet.sh"), "{installed}");
        assert!(installed.contains(MARKER), "{installed}");
        assert_eq!(round_trip(theirs), theirs, "somebody else's hook was not put back");
    }

    #[test]
    fn installing_twice_is_installing_once() {
        let installed = add("{}\n", &hooks()).unwrap().unwrap();
        assert!(add(&installed, &hooks()).unwrap().is_none(), "a second install wrote again");
    }

    #[test]
    fn a_command_that_has_changed_replaces_the_one_that_is_there() {
        let installed = add("{}\n", &hooks()).unwrap().unwrap();
        let mut later = hooks();
        later[0].command = String::from("exec nodal claude-code worktree-create --new-flag");
        let again = add(&installed, &later).unwrap().expect("the changed command was not written");
        assert!(again.contains("--new-flag"), "{again}");
        assert_eq!(
            again.matches("worktree-create").count(),
            1,
            "the changed command joined the one it should have replaced: {again}"
        );
    }

    #[test]
    fn a_file_that_holds_no_hook_of_ours_is_left_alone() {
        assert!(remove("{\n  \"permissions\": {}\n}\n", &hooks()).is_none());
        assert!(!holds_hooks("{\n  \"permissions\": {}\n}\n"));
    }

    #[test]
    fn a_file_that_was_reformatted_since_the_install_still_loses_the_hooks() {
        let installed = add("{}\n", &hooks()).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_str(&installed).unwrap();
        let reformatted = serde_json::to_string(&value).unwrap();
        let removed = remove(&reformatted, &hooks()).expect("the fallback found nothing");
        assert!(!holds_hooks(&removed), "{removed}");
        assert!(is_empty(&removed), "{removed}");
    }

    #[test]
    fn what_stays_in_the_file_is_named_before_it_is_edited() {
        let theirs = "{\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"./scripts/audit.sh\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
        let installed = add(theirs, &hooks()).unwrap().unwrap();
        assert_eq!(other_events(&installed), vec![String::from("PreToolUse")]);
    }

    #[test]
    fn a_file_that_is_not_an_object_is_refused_rather_than_rewritten() {
        assert!(add("[1, 2]", &hooks()).is_err());
        assert!(add("not json at all", &hooks()).is_err());
    }

    #[test]
    fn a_brace_inside_somebody_command_text_is_not_read_as_structure() {
        let theirs = "{\n  \"env\": {\n    \"NOTE\": \"a \\\"hooks\\\": { in a string\"\n  }\n}\n";
        assert_eq!(round_trip(theirs), theirs);
    }
}
