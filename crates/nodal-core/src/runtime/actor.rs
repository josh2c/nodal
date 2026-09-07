//! Who is running this: a person at a terminal, or an agent.
//!
//! The answer comes from the environment a process carries, because that is the one
//! signal every tool leaves and the only one available for a process Nodal did not
//! start. `NODAL_ACTOR` is the explicit answer and beats every guess; after it comes a
//! table of the variables agents set for themselves, and after that the person the
//! process runs as.
//!
//! The table is data so that supporting one more agent is a row. A signal is a variable
//! *name* only: the value is never read, never recorded and never printed.

use std::collections::BTreeMap;

use crate::Result;
use crate::model::{Actor, ActorKind, ActorName};

/// The variable that names the actor outright: an agent's own name.
pub const OVERRIDE: &str = "NODAL_ACTOR";

/// The variable a shell leaves that names the person.
pub const USER: &str = "USER";

/// What an actor is called when nothing at all says.
pub const UNKNOWN: &str = "unknown";

/// A variable an agent sets, and the name that agent is known by.
struct Signal {
    /// The variable name to look for. Its value is never read.
    var: &'static str,
    /// What the agent is called in a report and in an event.
    name: &'static str,
}

/// Every agent Nodal recognises from the environment alone.
const SIGNALS: &[Signal] = &[
    Signal { var: "CLAUDECODE", name: "claude-code" },
    Signal { var: "CLAUDE_CODE_ENTRYPOINT", name: "claude-code" },
    Signal { var: "CODEX_SANDBOX", name: "codex" },
    Signal { var: "CURSOR_AGENT", name: "cursor" },
    Signal { var: "AIDER_MODEL", name: "aider" },
];

/// Every variable name this module reads, so that a process scan can keep those and
/// discard the rest of an environment it had to look at.
#[must_use]
pub fn signal_names() -> Vec<&'static str> {
    let mut names = vec![OVERRIDE, USER];
    names.extend(SIGNALS.iter().map(|signal| signal.var));
    names
}

/// The actor a set of variables describes.
///
/// # Errors
/// [`crate::Error::InvalidValue`], which cannot happen while [`UNKNOWN`] is a name the
/// model accepts: an unprintable name falls back to it rather than failing.
pub fn from_vars(vars: &BTreeMap<String, String>) -> Result<Actor> {
    let lookup = |name: &str| vars.get(name).map(String::as_str).filter(|text| !text.is_empty());
    if let Some(named) = lookup(OVERRIDE).and_then(|text| ActorName::parse(text).ok()) {
        return Ok(Actor { kind: ActorKind::Agent, name: named });
    }
    if let Some(agent) = SIGNALS.iter().find(|signal| lookup(signal.var).is_some()) {
        return Ok(Actor { kind: ActorKind::Agent, name: name_or_unknown(agent.name)? });
    }
    let person = lookup(USER).unwrap_or(UNKNOWN);
    Ok(Actor { kind: ActorKind::Human, name: name_or_unknown(person)? })
}

/// The actor of the process that is running now.
///
/// # Errors
/// As [`from_vars`].
pub fn current() -> Result<Actor> {
    let vars = signal_names()
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?)))
        .collect();
    from_vars(&vars)
}

/// A name the model accepts, falling back rather than failing: an actor whose name is
/// unprintable is still an actor, and refusing to record the event would lose more.
fn name_or_unknown(text: &str) -> Result<ActorName> {
    ActorName::parse(text).or_else(|_| ActorName::parse(UNKNOWN))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeMap;

    use super::{OVERRIDE, USER, from_vars};
    use crate::model::ActorKind;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
    }

    #[test]
    fn the_explicit_answer_wins() {
        let actor = from_vars(&vars(&[(OVERRIDE, "claude-code"), (USER, "josh")])).unwrap();
        assert_eq!(actor.kind, ActorKind::Agent);
        assert_eq!(actor.name.as_str(), "claude-code");
    }

    #[test]
    fn an_agents_own_variable_names_it() {
        let actor = from_vars(&vars(&[("CLAUDECODE", "1"), (USER, "josh")])).unwrap();
        assert_eq!(actor.kind, ActorKind::Agent);
        assert_eq!(actor.name.as_str(), "claude-code");
    }

    #[test]
    fn a_shell_with_no_agent_in_it_is_a_person() {
        let actor = from_vars(&vars(&[(USER, "josh")])).unwrap();
        assert_eq!(actor.kind, ActorKind::Human);
        assert_eq!(actor.name.as_str(), "josh");
    }
}
