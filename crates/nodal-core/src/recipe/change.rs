//! Which lines of a file a rewrite changes.
//!
//! `nodal init --force` writes a recipe over one a person edited. What it keeps is
//! every key that file sets; what it does not keep is every comment, because the
//! contents are rendered from the merged recipe and the render writes the template's
//! own comments. A person who ran the command to answer one gap lost the note they
//! wrote above another one, and nothing said so.
//!
//! So the command says which lines change before it writes them. This is the reading
//! it says it from: the lines of the old text that the new text does not hold, and the
//! lines of the new text that the old text did not, each at the line number of the file
//! it belongs to.
//!
//! It is a reading of two texts and it writes nothing. The rule for which lines are
//! "the same" is the longest common subsequence of the two line lists, which is the
//! rule every diff uses and the only one that does not report a whole file as changed
//! when one line is inserted at the top.

use serde::{Deserialize, Serialize};

/// Whether a line leaves the file or arrives in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit {
    /// The old file held this line and the new one does not.
    Removed,
    /// The new file holds this line and the old one did not.
    Added,
}

impl Edit {
    /// The mark a report prints this edit with, which is the one `diff` prints.
    #[must_use]
    pub const fn mark(self) -> char {
        match self {
            Self::Removed => '-',
            Self::Added => '+',
        }
    }
}

/// One line that a rewrite changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// Whether the line leaves or arrives.
    pub edit: Edit,
    /// Its line number, counted from one, in the file it belongs to: the old file for a
    /// removal and the new file for an addition.
    pub at: usize,
    /// The line itself, without its terminator.
    pub text: String,
}

/// Every line `before` holds that `after` does not, and every line `after` holds that
/// `before` did not, in the order a reader reads them.
///
/// Two texts that hold the same lines answer an empty list, which is what says a
/// rewrite would change nothing.
#[must_use]
pub fn lines(before: &str, after: &str) -> Vec<Change> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let common = common_table(&old, &new);
    walk(&old, &new, &common)
}

/// The length of the longest common subsequence of every pair of suffixes.
///
/// `table[i][j]` is that length for `old[i..]` and `new[j..]`. Built from the end, so
/// that [`walk`] can read it forwards and emit the changes in the order they are read.
fn common_table(old: &[&str], new: &[&str]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    table
}

/// Read the table forwards, emitting a change for every line that is not in the common
/// subsequence.
///
/// A removal is emitted before an addition at the same place, so that a line a rewrite
/// replaces reads as the pair it is.
fn walk(old: &[&str], new: &[&str], common: &[Vec<usize>]) -> Vec<Change> {
    let (mut i, mut j, mut changes) = (0, 0, Vec::new());
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
        } else if j == new.len() || (i < old.len() && common[i + 1][j] >= common[i][j + 1]) {
            changes.push(Change { edit: Edit::Removed, at: i + 1, text: old[i].to_owned() });
            i += 1;
        } else {
            changes.push(Change { edit: Edit::Added, at: j + 1, text: new[j].to_owned() });
            j += 1;
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::{Edit, lines};

    #[test]
    fn two_texts_that_hold_the_same_lines_change_nothing() {
        assert!(lines("a\nb\nc\n", "a\nb\nc\n").is_empty());
        assert!(lines("", "").is_empty());
    }

    #[test]
    fn a_line_inserted_at_the_top_is_one_change_and_not_a_whole_file() {
        let changed = lines("a\nb\n", "new\na\nb\n");
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(changed[0].edit, Edit::Added);
        assert_eq!(changed[0].at, 1);
        assert_eq!(changed[0].text, "new");
    }

    /// The reading `nodal init --force` is there for: the keys stay and the comment a
    /// person wrote goes.
    #[test]
    fn a_comment_a_person_wrote_is_named_where_a_rewrite_drops_it() {
        let before = "# ours: the staging copy needs this\nbackend = \"native\"\n";
        let after = "# written by nodal init\nbackend = \"native\"\n";
        let changed = lines(before, after);
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert_eq!(changed[0].edit, Edit::Removed);
        assert_eq!(changed[0].text, "# ours: the staging copy needs this");
        assert_eq!(changed[1].edit, Edit::Added);
        assert_eq!(changed[1].text, "# written by nodal init");
    }

    #[test]
    fn a_line_number_counts_from_one_in_the_file_the_line_belongs_to() {
        let changed = lines("a\nb\nc\n", "a\nc\n");
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(changed[0].edit, Edit::Removed);
        assert_eq!(changed[0].at, 2, "b is the second line of the old file");
    }

    #[test]
    fn a_removal_is_read_before_the_addition_that_replaces_it() {
        let changed = lines("keep\nold\n", "keep\nnew\n");
        let marks: Vec<char> = changed.iter().map(|change| change.edit.mark()).collect();
        assert_eq!(marks, ['-', '+'], "{changed:?}");
    }

    #[test]
    fn a_file_written_where_there_was_none_is_every_line_added() {
        let changed = lines("", "a\nb\n");
        assert_eq!(changed.len(), 2);
        assert!(changed.iter().all(|change| change.edit == Edit::Added));
    }
}
