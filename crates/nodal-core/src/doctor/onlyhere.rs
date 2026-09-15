//! The work of a unit home that exists nowhere else.
//!
//! Every other source here reports what a tool left behind: a worktree, a cache, a
//! stopped container. This one reports the opposite kind of thing — a unit that is
//! working exactly as it should and holds the only copy of a morning's commits.
//!
//! It is here because of what doctor said without it. A machine whose two homes held
//! three commits that exist in no other object store and on no remote was told "nothing
//! of this project is left behind". That sentence was true of doctor's own subject,
//! which was leftovers, and it was read as an answer to "is anything only on this
//! disk?" — the question a person asks before they clear a machine.
//!
//! **The reading is the reclaim's own** ([`assess`]). A commit of a home is only here
//! when the project's checkout does not reach it, no other object store on this machine
//! holds it, and a witnessed reading of the remote does not reach it either. A commit
//! nothing checked is reported as unchecked and never as safe. There is one evaluator,
//! so doctor, `nodal reclaim --check` and a refusing `nodal reclaim` cannot disagree
//! about one unit.
//!
//! **Nothing here writes, signals or reaches a network.** [`assess::Input::refusal`] is
//! the cheapest of the three readings: the refusal and nothing it does not act on, with
//! no dispositions, no state classification and no process scan.
//!
//! **A home that could not be read is a row.** It is not silence and it is not a clean
//! verdict: doctor's closing line for the section says nothing was left behind, and a
//! home nobody could read is not evidence for that sentence.
//!
//! Only this project's section carries these rows. Another project's unique work is that
//! project's to look at, and a person cleaning up one project must not be led into it.

use std::path::Path;

use rusqlite::Connection;

use crate::doctor::{Known, Scope, Section, size};
use crate::lifecycle::assess::{self, Copies};
use crate::model::{Unit, UnitStatus};
use crate::output::view::doctor::{Finding, Kind};
use crate::store::{environments, units};

/// Every unit home of this project that holds work no other copy has.
///
/// # Errors
/// [`crate::Error::Store`] when the registry could not be read.
pub fn find(conn: &Connection, scope: &Scope) -> crate::Result<Vec<(Section, Finding)>> {
    let mut rows = Vec::new();
    for known in &scope.projects {
        if scope.section(&known.root) == Section::Elsewhere {
            continue;
        }
        for unit in open_units(conn, known)? {
            for environment in environments::list_for_unit(conn, unit.id)? {
                if !environment.home.is_dir() {
                    continue;
                }
                if let Some(finding) = read(&environment.home, &known.root, &unit) {
                    rows.push((Section::Here, finding));
                }
            }
        }
    }
    Ok(rows)
}

/// The units of a project that still have a home to read: everything but the archived.
fn open_units(conn: &Connection, known: &Known) -> crate::Result<Vec<Unit>> {
    let mut open = units::list_by_status(conn, known.project.id, UnitStatus::Open)?;
    open.extend(units::list_by_status(conn, known.project.id, UnitStatus::Review)?);
    Ok(open)
}

/// One home, read as a reclaim reads it, and the row it earns.
///
/// `None` for a home whose every commit lives somewhere else, which is the home the
/// closing line is about.
fn read(home: &Path, checkout: &Path, unit: &Unit) -> Option<Finding> {
    let assessed = match assess::assess(&assess::Input::refusal(home, Some(checkout))) {
        Ok(assessed) => assessed,
        Err(why) => return Some(unreadable(unit, home, &why.to_string())),
    };
    let counted = |wanted: fn(&Copies) -> bool| -> usize {
        assessed.commits.iter().filter(|group| wanted(&group.copies)).map(|group| group.count).sum()
    };
    let only_here = counted(|copies| matches!(copies, Copies::OnlyHere { .. }));
    let unchecked = counted(|copies| matches!(copies, Copies::NotChecked { .. }));
    if only_here == 0 && unchecked == 0 && assessed.notes.is_empty() {
        return None;
    }
    let measured = size::measure(home);
    let mut finding = Finding::new(Kind::UniqueWork, unit.slug.to_string())
        .sized(measured.bytes, measured.complete);
    if only_here > 0 {
        finding = finding.says(commits(only_here, "exist only here"));
    }
    if unchecked > 0 {
        finding = finding.says(commits(unchecked, "nothing here has checked"));
    }
    for note in &assessed.notes {
        finding = finding.says(note.clone());
    }
    finding.intent = unit.objective.as_ref().map(ToString::to_string);
    Some(finding)
}

/// How many commits, and what is true of them.
fn commits(count: usize, said: &str) -> String {
    let word = if count == 1 { "commit" } else { "commits" };
    format!("{count} {word} {said}")
}

/// The row a home that could not be read earns, which is never no row at all.
fn unreadable(unit: &Unit, home: &Path, why: &str) -> Finding {
    Finding::new(Kind::UniqueWork, unit.slug.to_string())
        .says(format!("the home at {} could not be read: {why}", home.display()))
}
