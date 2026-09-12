//! The verdict says what it read, and says how old the reading is.
//!
//! The table is the first thing a stranger sees, and every cell of it is a claim about a
//! directory they are deciding whether to delete. Two of those claims were reported wrong
//! on a real checkout in a way nothing in the table admitted, and each is one property
//! here.
//!
//! | property | what a break looks like |
//! |---|---|
//! | an intent reaches the cell unchanged | a prompt holding an apostrophe or a letter outside ASCII renders as something the person did not write |
//! | a stale reading says so | `-0 (origin/main)` is printed from three-week-old refs and the line says nothing about it |
//! | a fresh reading says nothing extra | a checkout fetched minutes ago carries words about an age nobody needed |
//!
//! **FOR is quoted text.** It is somebody's own words, taken out of a session record and
//! cut to the width of the column, and nothing between the record and the cell may change
//! a character of it. A recovered intent is marked as observed wherever it is shown for
//! this reason: Nodal is repeating what it read, so what it repeats has to be what is
//! there.
//!
//! **BEHIND is arithmetic over a ref only a fetch moves.** Nodal makes no network call of
//! its own, so it cannot make that ref current and does not try. What it can do is say how
//! old the ref is, which is what turns a right-looking `-0` into a reading a person can
//! judge. The age is in the closing line rather than in a column, because every row is
//! measured against the same revision and the staleness belongs to the checkout.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests fail by panicking")]

use nodal_safety::checkout::{self, NESTED};
use nodal_safety::{Machine, stdout};

/// A prompt holding the two things text is damaged by: an apostrophe, which a shell or a
/// quoting layer escapes, and letters outside ASCII, which a cut counted in bytes breaks.
/// It is under the width of the column, so the cell is the whole of it.
const INTENT: &str = "Fix the café's naïve join'contest résumé";

/// The same shape, over the width of the column, so the cell is a cut of it.
const LONG: &str = "Fix the café's naïve join'contest résumé, and then go on past the width \
                    of the column so that the cell is a cut and not the whole line";

/// How wide the FOR column is, which is `verdict::INTENT_WIDTH`. Written out rather than
/// imported: a test that took the width from the code under test would agree with it
/// however wrong it was.
const WIDTH: usize = 48;

/// What the verdict prints in a checkout holding one worktree whose intent was recorded.
fn rendered(prompt: &str) -> String {
    let machine = Machine::new();
    let plain = checkout::plain(&machine);
    checkout::record_intent(&machine, &plain.nested, prompt);
    stdout(&machine.nodal_in(&plain.checkout, &[]))
}

/// An intent a supported agent recorded reaches the cell as the person typed it.
///
/// Byte for byte. The assertion is on the rendered table and not on the reading behind
/// it, because every step between the record and the terminal is in scope: the parse of
/// the record, the reading of the task out of the prompt, the cut to the column, and the
/// laying out of the row.
#[test]
fn a_recovered_intent_reaches_the_cell_unchanged() {
    let report = rendered(INTENT);
    assert!(
        report.contains(INTENT),
        "the intent was damaged between the record and the cell.\nrecorded: {INTENT}\n{report}"
    );
}

/// The same intent, cut by the column.
///
/// The cut is a prefix of what was recorded and nothing else: the same characters, in the
/// same order, with one character standing for the rest. A cut counted in bytes would
/// break a letter outside ASCII in half, and a quoting layer would show its escape.
#[test]
fn an_intent_wider_than_the_column_is_cut_and_never_rewritten() {
    let report = rendered(LONG);
    let kept: String = LONG.chars().take(WIDTH - 1).collect();
    let cell = format!("{}…", kept.trim_end());
    assert!(
        report.contains(&cell),
        "the cut cell is not a prefix of what was recorded.\nexpected: {cell}\n{report}"
    );
}

/// A reading taken against a ref nobody has fetched for three weeks says so.
///
/// This is the shape that was reported: several worktrees, `-0 (origin/main)`, and an
/// `origin/main` last moved three weeks earlier. The arithmetic is right and the data is
/// three weeks old, and until this line nothing in the answer said which.
#[test]
fn a_stale_behind_reading_says_how_old_it_is() {
    let machine = Machine::new();
    let plain = checkout::plain(&machine);
    checkout::tracking_origin(&plain.checkout);
    checkout::last_moved_days_ago(&plain.checkout, "refs/remotes/origin/main", 23);

    let report = stdout(&machine.nodal_in(&plain.checkout, &[]));
    assert!(report.contains("origin/main"), "nothing was measured against origin/main:\n{report}");
    assert!(
        report.contains("last moved on 20"),
        "a reading against a ref three weeks old did not say how old it is:\n{report}"
    );
    assert!(report.contains("nodal removed nothing"), "the promise is missing:\n{report}");
}

/// A reading taken minutes after a fetch says nothing about an age.
///
/// The table is already wide and the closing line is already two sentences. An age on
/// every reading would teach a person to skip the line that the stale case needs them to
/// read.
#[test]
fn a_fresh_behind_reading_is_not_cluttered_with_an_age() {
    let machine = Machine::new();
    let plain = checkout::plain(&machine);
    checkout::tracking_origin(&plain.checkout);

    let report = stdout(&machine.nodal_in(&plain.checkout, &[]));
    assert!(report.contains(NESTED), "the verdict reported nothing:\n{report}");
    assert!(
        !report.contains("last moved"),
        "a checkout whose refs moved moments ago was reported as stale:\n{report}"
    );
}
