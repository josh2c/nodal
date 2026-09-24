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
//! **The checkout is read once for the project, not once for each home, and not at all
//! until a home needs it.** What a reading asks of the checkout — its git directory, its
//! refs, its `origin`, and which of its tips it really holds — is the same question
//! whichever home is being read, and asking it per home was six `git` invocations per
//! home for one answer. [`Checkout::read`] takes it once, on the first home of the
//! project that is there to read, and every home after it is read against that one
//! reading. A project whose homes are all gone reads nothing, which is what it cost
//! before. It is the same evidence, so no row moves.
//!
//! **A home that could not be read is a row.** It is not silence and it is not a clean
//! verdict: doctor's closing line for the section says nothing was left behind, and a
//! home nobody could read is not evidence for that sentence.
//!
//! **The question is asked of the project's open homes as a set.** Two units can each be
//! safe because the other holds the copy: the reading of one names the other's home as a
//! second object store, and the reading of the other names the first. Each answer is true
//! and the pair is not, and a person clearing a machine removes both. So a home of this
//! project's own open units is not believed as a second store ([`kernel::joint`]), and a
//! commit that lives only in one of them is reported here.
//!
//! The project's checkout, and the clones beside it that are nobody's unit home, are
//! believed exactly as they were. They are not going anywhere when the units do.
//!
//! Only this project's section carries these rows. Another project's unique work is that
//! project's to look at, and a person cleaning up one project must not be led into it.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::doctor::{Known, Scope, Section, scan, size};
use crate::lifecycle::assess::{self, Copies};
use crate::lifecycle::kernel;
use crate::lifecycle::witness::Checkout;
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
        let mut checkout = None;
        // Discovered once for the project, like the checkout beside it and for the same
        // reason: which other repositories stand beside this checkout is a fact about
        // the project, not about any home read against them.
        let mut siblings = None;
        let open = open_units(conn, known)?;
        let homes = homes_of(conn, &open)?;
        for unit in &open {
            for environment in environments::list_for_unit(conn, unit.id)? {
                if !environment.home.is_dir() {
                    continue;
                }
                let read_once = checkout.get_or_insert_with(|| Checkout::read(&known.root));
                let beside = siblings.get_or_insert_with(|| scan::siblings(&known.root));
                if let Some(finding) = read(&environment.home, read_once, beside, &homes, unit) {
                    rows.push((Section::Here, finding));
                }
            }
        }
    }
    Ok(rows)
}

/// Every home of these units that is on the disk, which is the set the question is asked
/// of.
///
/// Read once for the project. A home that is not there holds nothing and is left out, so
/// a unit whose home was already removed never discounts a copy that is really there.
fn homes_of(conn: &Connection, open: &[Unit]) -> crate::Result<Vec<PathBuf>> {
    let mut homes = Vec::new();
    for unit in open {
        for environment in environments::list_for_unit(conn, unit.id)? {
            if environment.home.is_dir() {
                homes.push(environment.home);
            }
        }
    }
    Ok(homes)
}

/// The units of a project that still have a home to read: everything but the archived.
fn open_units(conn: &Connection, known: &Known) -> crate::Result<Vec<Unit>> {
    let mut open = units::list_by_status(conn, known.project.id, UnitStatus::Open)?;
    open.extend(units::list_by_status(conn, known.project.id, UnitStatus::Review)?);
    Ok(open)
}

/// One home, read as a reclaim reads it, and the row it earns.
///
/// `homes` is every open home of this project, this one included. A copy that lives only
/// in one of them is not a copy a person clearing the machine keeps, so the joint rule
/// discounts it ([`kernel::joint`]).
///
/// That rule is asked of the reading rather than built into it. The reading is exactly
/// [`assess::Input::refusal`], the same one a refusing `nodal reclaim` makes, and the set is
/// handed to the kernel, which applies it to the [`Copies::SecondLocalCopy`] groups the
/// reading came back with ([`kernel::joint`]) — the same route `nodal reclaim --check` takes
/// over a set of units. So the two joint answers cannot name different holders, because
/// there is one rule and one place it is asked.
///
/// The row itself is the kernel's verdict and not a count compared with zero. What the two
/// clauses below add is which of the two refusals it is, and how many commits each is
/// about, which a verdict does not carry and a person reading the section wants.
///
/// `None` for a home whose every commit lives somewhere else, which is the home the
/// closing line is about.
fn read(
    home: &Path,
    checkout: &Checkout,
    siblings: &[PathBuf],
    homes: &[PathBuf],
    unit: &Unit,
) -> Option<Finding> {
    let assessed = match assess::assess(&assess::Input::refusal(home, Some(checkout), siblings)) {
        Ok(assessed) => assessed,
        Err(why) => return Some(unreadable(unit, home, &why.to_string())),
    };
    let counted = |wanted: fn(&Copies) -> bool| -> usize {
        assessed.commits.iter().filter(|group| wanted(&group.copies)).map(|group| group.count).sum()
    };
    let only_here = counted(|copies| matches!(copies, Copies::OnlyHere { .. }));
    let unchecked = counted(|copies| matches!(copies, Copies::NotChecked { .. }));
    let shared = kernel::joint(&assessed.loss_set(), homes);
    if only_here == 0 && unchecked == 0 && shared.is_empty() && assessed.notes.is_empty() {
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
    for reason in &shared {
        finding = finding.says(reason.detail.clone());
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
