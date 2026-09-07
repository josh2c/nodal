//! The human renderer: blocks of text, and the one place that lays them out.
//!
//! A read type does not print itself. It describes itself as a [`Doc`] of blocks, and
//! this file decides indentation, label widths, column widths and separators. Layout is
//! therefore one implementation with one set of tests, and a read type added later
//! cannot invent an alignment of its own.

use std::fmt;

use crate::model::Timestamp;

/// What a cell or a value with nothing in it carries, so a column never looks empty by
/// accident.
pub const NONE: &str = "—";

/// The separator between items that share one cell or one field value.
pub const JOIN: &str = " · ";

/// One indent level, in spaces.
const STEP: usize = 2;

/// The blank kept between a label or a column and whatever follows it.
const GAP: usize = 2;

/// One `label   value` pair. The value may hold newlines; continuation lines are
/// aligned under the first one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The label, lowercase, as the scenarios write it.
    pub label: String,
    /// What the label introduces.
    pub value: String,
}

impl Field {
    /// A field from anything that can become a string.
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self { label: label.into(), value: value.into() }
    }
}

/// A column-aligned table. Headers are written lowercase by callers and upper-cased
/// here, so the case is a property of the renderer rather than of every call site.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    /// Column headings, in order.
    pub headers: Vec<String>,
    /// One vector of cells per row, in the order of `headers`.
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// An empty table with these columns.
    #[must_use]
    pub fn new(headers: &[&str]) -> Self {
        Self { headers: headers.iter().map(|head| (*head).to_owned()).collect(), rows: Vec::new() }
    }

    /// Append one row. Missing cells render blank; extra cells widen the table.
    pub fn push(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// Whether the table has no rows, which is what a caller checks before showing it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// One piece of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A run of `label   value` pairs sharing one label column.
    Fields {
        /// Indent level, in units of [`STEP`] spaces.
        indent: usize,
        /// The pairs, in order.
        fields: Vec<Field>,
    },
    /// A column-aligned table with a heading row.
    Table {
        /// Indent level, in units of [`STEP`] spaces.
        indent: usize,
        /// The table itself.
        table: Table,
    },
    /// A single line of prose.
    Line {
        /// Indent level, in units of [`STEP`] spaces.
        indent: usize,
        /// The text.
        text: String,
    },
    /// A separating empty line.
    Blank,
}

impl Block {
    /// A run of fields at the default indent.
    #[must_use]
    pub fn fields(fields: Vec<Field>) -> Self {
        Self::Fields { indent: 1, fields }
    }

    /// A table at the default indent.
    #[must_use]
    pub fn table(table: Table) -> Self {
        Self::Table { indent: 1, table }
    }

    /// A line of prose at the default indent.
    #[must_use]
    pub fn line(text: impl Into<String>) -> Self {
        Self::Line { indent: 1, text: text.into() }
    }

    /// An empty line.
    #[must_use]
    pub const fn blank() -> Self {
        Self::Blank
    }

    /// The same block, moved to another indent level.
    #[must_use]
    pub fn at(self, level: usize) -> Self {
        match self {
            Self::Fields { fields, .. } => Self::Fields { indent: level, fields },
            Self::Table { table, .. } => Self::Table { indent: level, table },
            Self::Line { text, .. } => Self::Line { indent: level, text },
            Self::Blank => Self::Blank,
        }
    }

    /// The lines this block occupies, without a trailing newline on the last one.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        match self {
            Self::Fields { indent, fields } => field_lines(*indent, fields),
            Self::Table { indent, table } => table_lines(*indent, table),
            Self::Line { indent, text } => vec![format!("{}{text}", pad(*indent * STEP))],
            Self::Blank => vec![String::new()],
        }
    }
}

/// The human form of one read type: an ordered list of blocks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Doc {
    blocks: Vec<Block>,
}

impl Doc {
    /// An empty document.
    #[must_use]
    pub const fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Append a block.
    pub fn push(&mut self, block: Block) {
        self.blocks.push(block);
    }

    /// The blocks, for a caller that composes one document into another.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Every line of the document, without trailing newlines.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.blocks.iter().flat_map(Block::lines).collect()
    }
}

impl FromIterator<Block> for Doc {
    fn from_iter<I: IntoIterator<Item = Block>>(blocks: I) -> Self {
        Self { blocks: blocks.into_iter().collect() }
    }
}

impl fmt::Display for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for line in self.lines() {
            writeln!(f, "{}", line.trim_end())?;
        }
        Ok(())
    }
}

/// `count` spaces.
fn pad(count: usize) -> String {
    " ".repeat(count)
}

/// How wide a run of cells is, counted in characters rather than bytes so that the
/// separators and placeholders this module uses do not skew a column.
fn width(text: &str) -> usize {
    text.chars().count()
}

/// Lay out `label   value` pairs, aligning continuation lines under the value.
fn field_lines(indent: usize, fields: &[Field]) -> Vec<String> {
    let margin = pad(indent * STEP);
    let label_width = fields.iter().map(|field| width(&field.label)).max().unwrap_or(0) + GAP;
    let mut lines = Vec::new();
    for field in fields {
        let mut values = field.value.lines();
        let first = values.next().unwrap_or("");
        let label = format!("{}{}", field.label, pad(label_width - width(&field.label)));
        lines.push(format!("{margin}{label}{first}"));
        for value in values {
            lines.push(format!("{margin}{}{value}", pad(label_width)));
        }
    }
    lines
}

/// The width of every column: the widest of its heading and its cells.
fn column_widths(table: &Table) -> Vec<usize> {
    let mut widths: Vec<usize> = table.headers.iter().map(|head| width(head)).collect();
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            let cell = width(cell);
            match widths.get_mut(index) {
                Some(current) => *current = (*current).max(cell),
                None => widths.push(cell),
            }
        }
    }
    widths
}

/// Lay out one row against known column widths. The last cell is never padded, so no
/// line carries trailing blanks.
fn row_line(margin: &str, widths: &[usize], cells: &[String]) -> String {
    let mut line = String::from(margin);
    for (index, cell) in cells.iter().enumerate() {
        line.push_str(cell);
        let last = index + 1 == cells.len();
        if !last {
            let column = widths.get(index).copied().unwrap_or_else(|| width(cell));
            line.push_str(&pad(column.saturating_sub(width(cell)) + GAP));
        }
    }
    line
}

/// Lay out a heading row and its rows.
fn table_lines(indent: usize, table: &Table) -> Vec<String> {
    let margin = pad(indent * STEP);
    let widths = column_widths(table);
    let headers: Vec<String> = table.headers.iter().map(|head| head.to_uppercase()).collect();
    let mut lines = vec![row_line(&margin, &widths, &headers)];
    for row in &table.rows {
        lines.push(row_line(&margin, &widths, row));
    }
    lines
}

/// A byte count in the units a person reads, decimal rather than binary because that is
/// what a disk reports. Kept in integers so the output does not depend on rounding.
#[must_use]
pub fn bytes(count: u64) -> String {
    const SCALE: [(u64, &str); 4] =
        [(1_000_000_000_000, "TB"), (1_000_000_000, "GB"), (1_000_000, "MB"), (1_000, "kB")];
    for (scale, unit) in SCALE {
        if count >= scale {
            let tenths =
                u64::try_from(u128::from(count) * 10 / u128::from(scale)).unwrap_or(u64::MAX);
            return if tenths >= 1_000 {
                format!("{} {unit}", tenths / 10)
            } else {
                format!("{}.{} {unit}", tenths / 10, tenths % 10)
            };
        }
    }
    format!("{count} B")
}

/// How long ago `then` was, from `now`. `now` is a parameter rather than the clock, so
/// a rendering is a function of its inputs and a snapshot of it is stable.
#[must_use]
pub fn since(now: Timestamp, then: Timestamp) -> String {
    const SCALE: [(i64, i64, &str); 3] =
        [(3_600, 60, "min"), (86_400, 3_600, "h"), (0, 86_400, "d")];
    let seconds = now.unix_seconds() - then.unix_seconds();
    if seconds < 60 {
        return String::from("now");
    }
    for (limit, scale, unit) in SCALE {
        if limit == 0 || seconds < limit {
            return format!("{} {unit} ago", seconds / scale);
        }
    }
    String::from("now")
}

/// The text, or the placeholder when there is none.
#[must_use]
pub fn or_none(text: String) -> String {
    if text.is_empty() { String::from(NONE) } else { text }
}

/// Join items with [`JOIN`], or the placeholder when there are none.
#[must_use]
pub fn join(items: &[String]) -> String {
    or_none(items.join(JOIN))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Block, Doc, Field, Table, bytes, since};
    use crate::model::Timestamp;

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).expect("a fixed instant")
    }

    #[test]
    fn fields_share_one_label_column() {
        let doc = Doc::from_iter([Block::fields(vec![
            Field::new("unit", "worker-import"),
            Field::new("isolation", "db, rest, auth per unit"),
        ])]);
        assert_eq!(
            doc.to_string(),
            "  unit       worker-import\n  isolation  db, rest, auth per unit\n"
        );
    }

    #[test]
    fn a_multi_line_value_stays_under_its_first_line() {
        let doc =
            Doc::from_iter([Block::fields(vec![Field::new("needs you", "toolchain: ?\ndb: ?")])]);
        assert_eq!(doc.to_string(), "  needs you  toolchain: ?\n             db: ?\n");
    }

    #[test]
    fn columns_are_as_wide_as_their_widest_cell_and_never_trail() {
        let mut table = Table::new(&["unit", "state"]);
        table.push(vec![String::from("payroll-export"), String::from("open")]);
        table.push(vec![String::from("a"), String::from("open")]);
        let doc = Doc::from_iter([Block::table(table)]);
        assert_eq!(
            doc.to_string(),
            "  UNIT            STATE\n  payroll-export  open\n  a               open\n"
        );
    }

    #[test]
    fn indent_is_a_property_of_the_block() {
        let doc = Doc::from_iter([Block::line("managed").at(1), Block::line("2 units").at(2)]);
        assert_eq!(doc.to_string(), "  managed\n    2 units\n");
    }

    #[test]
    fn byte_counts_read_the_way_a_disk_reports_them() {
        assert_eq!(bytes(287_000_000), "287 MB");
        assert_eq!(bytes(2_400_000_000), "2.4 GB");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(0), "0 B");
    }

    #[test]
    fn ages_are_relative_to_the_instant_given() {
        let now = at("2026-09-06T12:00:00Z");
        assert_eq!(since(now, at("2026-09-06T11:59:30Z")), "now");
        assert_eq!(since(now, at("2026-09-06T11:48:00Z")), "12 min ago");
        assert_eq!(since(now, at("2026-09-06T09:00:00Z")), "3 h ago");
        assert_eq!(since(now, at("2026-09-04T12:00:00Z")), "2 d ago");
    }
}
