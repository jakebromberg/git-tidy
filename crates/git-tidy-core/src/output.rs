//! Shared output helpers used by all git-tidy binary crates.

use std::borrow::Cow;
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use crate::counts::Counts;
use crate::types::{Classification, ClassificationLabel, WorktreeInfo};

/// Terminal display width in cells (codepoint width per Unicode UAX-11).
/// Used by `format_table` so multi-byte / wide-character cells stay aligned.
fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Ordered `(display word, count key)` pairs for the landed-classification summary
/// shared by worktree-tidy and branch-tidy. The display word is what the human sees
/// (`content`); the key is the classification label the count is stored under
/// (`landed-content`).
pub const LANDED_SUMMARY: &[(&str, &str)] = &[
    ("landed", "landed"),
    ("stale", "landed-stale"),
    ("content", "landed-content"),
    ("partial", "partial"),
    ("active", "active"),
    ("local", "local"),
];

/// Format a summary breakdown — `"{n0} {disp0}, {n1} {disp1}, …"` — from a `Counts`
/// and an ordered `(display, key)` spec. Each entry reads `counts.get(key)` (0 when
/// absent, so explicit zeros still print) and pairs it with the human display word.
///
/// Centralizes the breakdown so the scan tools don't each hand-roll the same
/// `writeln!`. Tools whose summary is a single standard line call
/// [`write_summary_line`]; repo-tidy, which appends a dirty note and a second line,
/// calls this directly.
pub fn format_summary_buckets(counts: &Counts, spec: &[(&str, &str)]) -> String {
    spec.iter()
        .map(|(display, key)| format!("{} {display}", counts.get(key)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Write a standard summary line: `"\n{total} {item_noun} scanned: {breakdown}"`,
/// where the breakdown is built from `spec` (see [`format_summary_buckets`]).
pub fn write_summary_line(
    out: &mut dyn Write,
    total: usize,
    counts: &Counts,
    item_noun: &str,
    spec: &[(&str, &str)],
) -> std::io::Result<()> {
    writeln!(
        out,
        "\n{total} {item_noun} scanned: {}",
        format_summary_buckets(counts, spec)
    )
}

/// Write warnings with the "warning: " prefix.
pub fn write_warnings(out: &mut dyn Write, warnings: &[String]) -> std::io::Result<()> {
    for warning in warnings {
        writeln!(out, "warning: {warning}")?;
    }
    Ok(())
}

/// Format ahead/behind as "+N/-M". Returns empty string when both are 0.
pub fn format_ahead_behind(ahead: usize, behind: usize) -> String {
    if ahead > 0 || behind > 0 {
        format!("+{}/-{}", ahead, behind)
    } else {
        String::new()
    }
}

/// Format a comma-separated annotation list from string slices.
/// Returns empty string when the list is empty.
pub fn format_annotations(annotations: &[&str]) -> String {
    annotations.join(", ")
}

/// Extract a display name from a repo path (last path component, or full path as fallback).
pub fn repo_display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Trait for scan results that can be flattened into JSON items.
pub trait FlatJsonItems {
    /// The JSON-serializable item type.
    type JsonItem: Serialize;

    /// Flatten repo groups into a vec of JSON items.
    fn to_json_items(&self) -> Vec<Self::JsonItem>;
}

/// Flatten a scan result into JSON items and write as pretty-printed JSON.
pub fn write_json_flat<T: FlatJsonItems>(out: &mut dyn Write, result: &T) -> std::io::Result<()> {
    let items = result.to_json_items();
    write_json_pretty(out, &items)
}

/// Serialize a value as pretty-printed JSON and write to output.
pub fn write_json_pretty(out: &mut dyn Write, value: &impl Serialize) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    writeln!(out, "{json}")
}

/// Write a hint pointing users to `git tidy explain` for terminology.
pub fn write_explain_hint(out: &mut dyn Write) -> std::io::Result<()> {
    writeln!(out, "hint: run 'git tidy explain' for a glossary of terms")
}

/// Format the landed ratio for display. Returns empty string for non-landed classifications.
pub fn format_landed_ratio(classification: &Classification) -> String {
    match classification {
        Classification::LandedByContent { matched, total } => format!("{matched}/{total}"),
        Classification::LandedPartial { matched, total, .. } => format!("{matched}/{total}"),
        _ => String::new(),
    }
}

/// Horizontal alignment for a `TidyItem` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// Schema for a `TidyItem` column: header text + alignment.
#[derive(Debug, Clone, Copy)]
pub struct ColumnSpec {
    pub header: &'static str,
    pub align: Align,
}

impl ColumnSpec {
    /// Left-aligned column. Most table columns use this — short helper to keep
    /// `const COLUMNS` declarations compact.
    pub const fn left(header: &'static str) -> Self {
        Self {
            header,
            align: Align::Left,
        }
    }

    /// Right-aligned column. Used for numeric cells where the digit columns
    /// should line up.
    pub const fn right(header: &'static str) -> Self {
        Self {
            header,
            align: Align::Right,
        }
    }
}

/// A single cell value.
///
/// `Cell = Cow<'static, str>`. The `'static` lifetime only constrains the
/// `Borrowed` variant — `Cow::Owned(String)` (e.g. produced by `format!(...).into()`)
/// is valid regardless of how short-lived the source data is.
pub type Cell = Cow<'static, str>;

/// A tool-row type that can be rendered as a row in a human table or a porcelain line.
///
/// Implemented by each scan-shaped tool on its row struct (e.g. `WorktreeInfo`,
/// `BranchInfo`). By convention `TidyItem` impls live in the tool's `output.rs`,
/// not in `types.rs`, to keep formatting concerns out of the data layer.
///
/// `format_table` consumes `row()`, `row_extras()`, and `annotations()` to render a
/// padded human table. `format_porcelain` consumes only `porcelain_fields()`.
pub trait TidyItem {
    /// Static column schema. All items of a given type share this schema.
    const COLUMNS: &'static [ColumnSpec];

    /// One cell per column, in column order. `None` means "no value for this row at
    /// this column"; `format_table` hides a column iff every row returns `None` for
    /// that index. Tools that want a column to always be visible (even with no
    /// values) should return `Some(Cow::Borrowed(""))` to opt out of the
    /// auto-hide rule.
    fn row(&self) -> Vec<Option<Cell>>;

    /// Extra lines printed underneath the row, indented with four spaces.
    /// Default: empty. Used by worktree-tidy for `LandedPartial` unmatched-commit
    /// listings.
    fn row_extras(&self) -> Vec<Cow<'static, str>> {
        Vec::new()
    }

    /// Annotation tokens for **human output only**. `format_table` joins them with
    /// `", "` and appends them as `"  {joined}"` to the row. `format_porcelain`
    /// does NOT call this — porcelain encodes annotations through
    /// `porcelain_fields()` directly so the human format and the machine format
    /// are free to diverge (e.g. human `"remote deleted"` vs porcelain token
    /// `"remote_deleted"`). Default: empty.
    fn annotations(&self) -> Vec<Cow<'static, str>> {
        Vec::new()
    }

    /// Ordered porcelain fields. `format_porcelain` joins them with `\t`.
    ///
    /// **Contract**: field `[0]` is the **primary path identifier** for the row.
    /// For most tools this is the repo path. The lone exception is
    /// `WorktreeInfo`: its `[0]` is the worktree directory and the parent repo
    /// is at `[1]`, preserving the long-standing `worktree-tidy --porcelain`
    /// shape. New tools should put `repo_path.display().to_string()` at `[0]`
    /// unless they have a comparable historical reason to differ.
    ///
    /// The field count and the order of fields are part of each tool's public
    /// porcelain interface and must remain stable across releases.
    fn porcelain_fields(&self) -> Vec<Cow<'static, str>>;
}

/// Render a slice of items as a human-readable padded table.
///
/// # Contract
/// - Empty input writes nothing (no header, no blank line).
/// - Empty `T::COLUMNS` writes nothing.
/// - Per-column widths are computed from `T::COLUMNS` headers + visible cells,
///   measured by Unicode display width (UAX-11), not byte length.
/// - A column at index `i` is omitted (both header and every row's cell) iff every
///   row supplies `None` at that index. If every column ends up hidden, nothing
///   is written.
/// - The header row and every row are prefixed with a two-space gutter (`"  "`).
/// - Cells are padded to the column display width per `Align::Left` (trailing
///   spaces) or `Align::Right` (leading spaces).
/// - Visible columns are separated by a single space.
/// - When `annotations()` is non-empty after filtering empty tokens, the row
///   gains a suffix `"  " + annotations.join(", ")`. Empty tokens are skipped
///   so they cannot produce stray `","` artifacts.
/// - Each entry in `row_extras()` is written as `"    {extra}\n"` (four-space
///   indent) underneath the row, in order. Extras are trimmed of trailing
///   whitespace so they obey the same rule as table rows.
/// - Trailing whitespace is trimmed from every rendered line before writing.
///
/// # Panics
/// In debug builds, panics if any `row()` returns a `Vec` whose length differs
/// from `T::COLUMNS.len()`. In release builds, missing cells are treated as
/// `None` and trailing extras are silently dropped.
pub fn format_table<T: TidyItem>(out: &mut dyn Write, items: &[T]) -> std::io::Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    let columns = T::COLUMNS;
    if columns.is_empty() {
        return Ok(());
    }

    let rows: Vec<Vec<Option<Cell>>> = items.iter().map(|i| i.row()).collect();

    if cfg!(debug_assertions) {
        for (row_idx, row) in rows.iter().enumerate() {
            debug_assert_eq!(
                row.len(),
                columns.len(),
                "TidyItem::row() length {} != COLUMNS.len() {} at item {}",
                row.len(),
                columns.len(),
                row_idx,
            );
        }
    }

    // A column is visible iff at least one row supplies Some(_) for that index.
    let visible: Vec<bool> = (0..columns.len())
        .map(|i| {
            rows.iter()
                .any(|r| r.get(i).and_then(|c| c.as_ref()).is_some())
        })
        .collect();

    if !visible.iter().any(|v| *v) {
        return Ok(());
    }

    // Width = max(display_width(header), longest visible-cell display width).
    let widths: Vec<usize> = (0..columns.len())
        .map(|i| {
            if !visible[i] {
                return 0;
            }
            let mut w = display_width(columns[i].header);
            for r in &rows {
                if let Some(Some(cell)) = r.get(i) {
                    w = w.max(display_width(cell));
                }
            }
            w
        })
        .collect();

    let mut header = String::from("  ");
    let mut first = true;
    for (i, col) in columns.iter().enumerate() {
        if !visible[i] {
            continue;
        }
        if !first {
            header.push(' ');
        }
        first = false;
        append_padded(&mut header, col.header, widths[i], col.align);
    }
    writeln!(out, "{}", header.trim_end())?;

    for (idx, item) in items.iter().enumerate() {
        let row = &rows[idx];
        let mut line = String::from("  ");
        let mut first = true;
        for (i, col) in columns.iter().enumerate() {
            if !visible[i] {
                continue;
            }
            if !first {
                line.push(' ');
            }
            first = false;
            let cell = row.get(i).and_then(|c| c.as_deref()).unwrap_or("");
            append_padded(&mut line, cell, widths[i], col.align);
        }
        let anns = item.annotations();
        let joined: Vec<&str> = anns
            .iter()
            .map(|a| a.as_ref())
            .filter(|s| !s.is_empty())
            .collect();
        if !joined.is_empty() {
            line.push_str("  ");
            line.push_str(&joined.join(", "));
        }
        writeln!(out, "{}", line.trim_end())?;

        for extra in item.row_extras() {
            writeln!(out, "    {}", extra.trim_end())?;
        }
    }

    Ok(())
}

/// Append `cell` to `buf` padded to `width` display columns per `align`.
/// Padding is added as ASCII spaces; both width inputs are measured in
/// Unicode UAX-11 cell width so wide characters (CJK, emoji) align correctly.
fn append_padded(buf: &mut String, cell: &str, width: usize, align: Align) {
    let cw = display_width(cell);
    let pad = width.saturating_sub(cw);
    match align {
        Align::Left => {
            buf.push_str(cell);
            for _ in 0..pad {
                buf.push(' ');
            }
        }
        Align::Right => {
            for _ in 0..pad {
                buf.push(' ');
            }
            buf.push_str(cell);
        }
    }
}

// `WorktreeInfo` lives in this crate (`crates/git-tidy-core/src/types.rs`), so its
// `TidyItem` impl must also live here to satisfy the orphan rule. Tools whose row
// type lives in the tool crate (e.g. `BranchInfo` in `git-branch-tidy/src/types.rs`)
// place their `TidyItem` impl in the tool's own `output.rs`.
impl TidyItem for WorktreeInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("DIRECTORY"),
        ColumnSpec::left("BRANCH"),
        ColumnSpec::left("RATIO"),
        ColumnSpec::left("AHEAD/BEHIND"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        let status: Cell = Cow::Borrowed(self.classification.label());
        let dir_name: Cell = self
            .path
            .file_name()
            .map(|n| Cow::Owned(n.to_string_lossy().into_owned()))
            .unwrap_or(Cow::Borrowed(""));
        let branch: Cell = match &self.branch {
            Some(b) => Cow::Owned(b.clone()),
            None => Cow::Borrowed("(detached)"),
        };

        let ratio = format_landed_ratio(&self.classification);
        let ratio_cell: Option<Cell> = if ratio.is_empty() {
            None
        } else {
            Some(Cow::Owned(ratio))
        };

        let ab = format_ahead_behind(self.ahead, self.behind);
        let ab_cell: Option<Cell> = if ab.is_empty() {
            None
        } else {
            Some(Cow::Owned(ab))
        };

        vec![
            Some(status),
            Some(dir_name),
            Some(branch),
            ratio_cell,
            ab_cell,
        ]
    }

    fn row_extras(&self) -> Vec<Cow<'static, str>> {
        match &self.classification {
            Classification::LandedPartial { unmatched, .. } => unmatched
                .iter()
                .map(|c| Cow::Owned(format!("unmatched: {} {}", c.short_hash, c.subject)))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn annotations(&self) -> Vec<Cow<'static, str>> {
        let mut anns: Vec<Cow<'static, str>> = Vec::new();
        if self.annotations.dirty {
            let n = self.annotations.dirty_file_count;
            let noun = if n == 1 { "file" } else { "files" };
            anns.push(Cow::Owned(format!("dirty ({n} {noun})")));
        }
        if self.annotations.diverged {
            anns.push(Cow::Borrowed("diverged"));
        }
        if self.annotations.remote_deleted {
            anns.push(Cow::Borrowed("remote deleted"));
        }
        anns
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        let mut porcelain_anns: Vec<&str> = Vec::new();
        if self.annotations.remote_deleted {
            porcelain_anns.push("remote_deleted");
        }
        if self.annotations.diverged {
            porcelain_anns.push("diverged");
        }
        if self.annotations.dirty {
            porcelain_anns.push("dirty");
        }

        vec![
            Cow::Owned(self.path.display().to_string()),
            Cow::Owned(self.parent_repo.display().to_string()),
            Cow::Owned(self.branch.clone().unwrap_or_default()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(format_landed_ratio(&self.classification)),
            Cow::Owned(self.ahead.to_string()),
            Cow::Owned(self.behind.to_string()),
            Cow::Owned(self.annotations.dirty_file_count.to_string()),
            Cow::Owned(porcelain_anns.join(",")),
        ]
    }
}

/// Render a slice of items as tab-delimited porcelain output.
///
/// # Contract
/// - Empty input writes nothing.
/// - Each item produces one line: `porcelain_fields().join("\t")` followed by `\n`.
/// - No header, no column hiding, no padding. Empty fields are preserved as
///   adjacent `\t`s.
/// - Per `TidyItem::porcelain_fields()`, field `[0]` is the row's primary path
///   identifier — `repo_path` for most tools, the worktree directory for
///   `WorktreeInfo` (see the trait doc for the full convention).
pub fn format_porcelain<T: TidyItem>(out: &mut dyn Write, items: &[T]) -> std::io::Result<()> {
    for item in items {
        let fields = item.porcelain_fields();
        let joined: Vec<&str> = fields.iter().map(|f| f.as_ref()).collect();
        writeln!(out, "{}", joined.join("\t"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_line_format() {
        let counts = Counts::from_pairs(&[
            ("landed", 3),
            ("landed-content", 1),
            ("active", 2),
            ("local", 1),
        ]);
        let mut buf = Vec::new();
        write_summary_line(&mut buf, 7, &counts, "branches", LANDED_SUMMARY).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output,
            "\n7 branches scanned: 3 landed, 0 stale, 1 content, 0 partial, 2 active, 1 local\n"
        );
    }

    #[test]
    fn summary_line_worktrees() {
        let counts = Counts::from_pairs(&[("landed", 1)]);
        let mut buf = Vec::new();
        write_summary_line(&mut buf, 1, &counts, "worktrees", LANDED_SUMMARY).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("1 worktrees scanned"));
    }

    #[test]
    fn warnings_output() {
        let mut buf = Vec::new();
        write_warnings(&mut buf, &["fetch failed for /repo".to_string()]).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "warning: fetch failed for /repo\n");
    }

    #[test]
    fn warnings_empty() {
        let mut buf = Vec::new();
        write_warnings(&mut buf, &[]).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn ahead_behind_nonzero() {
        assert_eq!(format_ahead_behind(3, 5), "+3/-5");
    }

    #[test]
    fn ahead_behind_zero() {
        assert_eq!(format_ahead_behind(0, 0), "");
    }

    #[test]
    fn annotations_basic() {
        assert_eq!(
            format_annotations(&["diverged", "remote deleted"]),
            "diverged, remote deleted"
        );
    }

    #[test]
    fn annotations_empty() {
        assert_eq!(format_annotations(&[]), "");
    }

    #[test]
    fn landed_ratio_by_content() {
        assert_eq!(
            format_landed_ratio(&Classification::LandedByContent {
                matched: 3,
                total: 3
            }),
            "3/3"
        );
    }

    #[test]
    fn landed_ratio_partial() {
        assert_eq!(
            format_landed_ratio(&Classification::LandedPartial {
                matched: 2,
                total: 5,
                unmatched: vec![],
            }),
            "2/5"
        );
    }

    #[test]
    fn landed_ratio_other() {
        assert_eq!(format_landed_ratio(&Classification::Active), "");
    }

    #[test]
    fn repo_display_name_normal() {
        use std::path::PathBuf;
        assert_eq!(
            repo_display_name(&PathBuf::from("/repos/my-project")),
            "my-project"
        );
    }

    #[test]
    fn write_json_pretty_basic() {
        let data = vec!["hello", "world"];
        let mut buf = Vec::new();
        write_json_pretty(&mut buf, &data).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed, serde_json::json!(["hello", "world"]));
    }

    #[test]
    fn repo_display_name_root_path() {
        use std::path::PathBuf;
        let path = PathBuf::from("/");
        // Root path has no file_name, should fall back to display
        assert_eq!(repo_display_name(&path), "/");
    }

    #[test]
    fn explain_hint_output() {
        let mut buf = Vec::new();
        write_explain_hint(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output,
            "hint: run 'git tidy explain' for a glossary of terms\n"
        );
    }

    // --- TidyItem / format_table / format_porcelain tests ---

    /// Test fixture: 3-column item with mixed alignment.
    struct Demo {
        a: Option<&'static str>,
        b: Option<&'static str>,
        c: Option<&'static str>,
        anns: Vec<&'static str>,
        extras: Vec<&'static str>,
    }

    impl Demo {
        fn new(a: Option<&'static str>, b: Option<&'static str>, c: Option<&'static str>) -> Self {
            Demo {
                a,
                b,
                c,
                anns: Vec::new(),
                extras: Vec::new(),
            }
        }

        fn with_anns(mut self, anns: Vec<&'static str>) -> Self {
            self.anns = anns;
            self
        }

        fn with_extras(mut self, extras: Vec<&'static str>) -> Self {
            self.extras = extras;
            self
        }
    }

    impl TidyItem for Demo {
        const COLUMNS: &'static [ColumnSpec] = &[
            ColumnSpec::left("AAA"),
            ColumnSpec::right("BBB"),
            ColumnSpec::left("CCC"),
        ];

        fn row(&self) -> Vec<Option<Cell>> {
            vec![
                self.a.map(Cow::Borrowed),
                self.b.map(Cow::Borrowed),
                self.c.map(Cow::Borrowed),
            ]
        }

        fn row_extras(&self) -> Vec<Cow<'static, str>> {
            self.extras.iter().copied().map(Cow::Borrowed).collect()
        }

        fn annotations(&self) -> Vec<Cow<'static, str>> {
            self.anns.iter().copied().map(Cow::Borrowed).collect()
        }

        fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
            vec![
                Cow::Borrowed("/repo"),
                Cow::Borrowed(self.a.unwrap_or("")),
                Cow::Borrowed(self.b.unwrap_or("")),
                Cow::Borrowed(self.c.unwrap_or("")),
            ]
        }
    }

    fn render_table(items: &[Demo]) -> String {
        let mut buf = Vec::new();
        format_table(&mut buf, items).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn render_porcelain(items: &[Demo]) -> String {
        let mut buf = Vec::new();
        format_porcelain(&mut buf, items).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn align_derives() {
        assert_eq!(Align::Left, Align::Left);
        assert_ne!(Align::Left, Align::Right);
        let _copy = Align::Right; // Copy
    }

    #[test]
    fn column_spec_is_copy() {
        let c = ColumnSpec {
            header: "X",
            align: Align::Right,
        };
        let c2 = c;
        assert_eq!(c2.header, "X");
        assert_eq!(c.header, "X");
    }

    #[test]
    fn column_spec_left_constructor() {
        let c = ColumnSpec::left("NAME");
        assert_eq!(c.header, "NAME");
        assert_eq!(c.align, Align::Left);
    }

    #[test]
    fn column_spec_right_constructor() {
        let c = ColumnSpec::right("SIZE");
        assert_eq!(c.header, "SIZE");
        assert_eq!(c.align, Align::Right);
    }

    #[test]
    fn column_spec_constructors_are_const() {
        // Both constructors must be usable in a `const COLUMNS` initializer.
        const COLS: &[ColumnSpec] = &[ColumnSpec::left("A"), ColumnSpec::right("B")];
        assert_eq!(COLS[0].align, Align::Left);
        assert_eq!(COLS[1].align, Align::Right);
    }

    #[test]
    fn format_table_renders_cow_owned_cells_end_to_end() {
        // Validates that a Cow::Owned String built at runtime flows correctly
        // through format_table — width, padding, and the trim_end step all
        // need to handle owned cells the same as borrowed ones.
        struct Owned(String);
        impl TidyItem for Owned {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("MSG")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Owned(self.0.clone()))]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo"), Cow::Owned(self.0.clone())]
            }
        }
        let n = 5usize;
        let items = vec![Owned(format!("dirty ({n} files)"))];
        let mut buf = Vec::new();
        format_table(&mut buf, &items).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        // trim_end strips the column-padding spaces from the header.
        assert_eq!(lines[0], "  MSG");
        assert_eq!(lines[1], "  dirty (5 files)");
    }

    #[test]
    fn format_table_empty_input_writes_nothing() {
        let items: Vec<Demo> = vec![];
        let mut buf = Vec::new();
        format_table(&mut buf, &items).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn format_table_single_row_renders_header_and_row() {
        let out = render_table(&[Demo::new(Some("foo"), Some("bar"), Some("baz"))]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "  AAA BBB CCC");
        assert_eq!(lines[1], "  foo bar baz");
    }

    #[test]
    fn format_table_every_line_starts_with_two_space_gutter() {
        let out = render_table(&[
            Demo::new(Some("x"), Some("y"), Some("z")),
            Demo::new(Some("xx"), Some("yy"), Some("zz")),
        ]);
        for line in out.lines() {
            assert!(line.starts_with("  "), "missing 2-space gutter: {line:?}");
        }
    }

    #[test]
    fn format_table_alignment_left_pads_trailing() {
        // AAA is left-aligned; "x" (1 char) in a width-3 column has trailing pad "xx ".
        // Because the next visible cell follows, we should see "x   " (1 + 2 pad + 1 separator).
        let out = render_table(&[Demo::new(Some("x"), Some("yy"), Some("zz"))]);
        let lines: Vec<&str> = out.lines().collect();
        // AAA col width: max(3, 1) = 3 -> "x  "
        // BBB col width: max(3, 2) = 3 -> right-pad " yy"
        // CCC col width: max(3, 2) = 3 -> "zz " (trimmed at end)
        assert_eq!(lines[1], "  x    yy zz");
    }

    #[test]
    fn format_table_alignment_right_pads_leading() {
        // BBB is right-aligned. With width 3 and cell "x", we get "  x".
        let out = render_table(&[Demo::new(Some("aa"), Some("x"), Some("c"))]);
        let lines: Vec<&str> = out.lines().collect();
        // AAA width 3: "aa "; BBB width 3 (right): "  x"; CCC width 3: "c  " -> trimmed
        assert_eq!(lines[1], "  aa    x c");
    }

    #[test]
    fn format_table_hides_column_when_every_row_is_none() {
        let out = render_table(&[
            Demo::new(Some("foo"), None, Some("baz")),
            Demo::new(Some("xx"), None, Some("yy")),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            !lines[0].contains("BBB"),
            "BBB header leaked: {:?}",
            lines[0]
        );
        // Hidden BBB means header is "  AAA CCC".
        assert_eq!(lines[0], "  AAA CCC");
        assert_eq!(lines[1], "  foo baz");
        assert_eq!(lines[2], "  xx  yy");
    }

    #[test]
    fn format_table_keeps_column_when_at_least_one_some() {
        let out = render_table(&[
            Demo::new(Some("aa"), Some("yes"), Some("cc")),
            Demo::new(Some("xx"), None, Some("yy")),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].contains("BBB"));
        // Row 2 has None for BBB — it should render as padded empty between AAA and CCC.
        // BBB width = max(3, 3) = 3 -> right-aligned empty "   ".
        assert_eq!(lines[2], "  xx      yy");
    }

    #[test]
    fn format_table_no_trailing_whitespace_on_any_line() {
        let out = render_table(&[
            Demo::new(Some("foo"), None, Some("baz")),
            Demo::new(Some("xx"), None, Some("yy")),
        ]);
        for line in out.lines() {
            assert_eq!(line.trim_end(), line, "trailing ws on: {line:?}");
        }
    }

    #[test]
    fn format_table_row_extras_indented_four_spaces() {
        let out = render_table(&[
            Demo::new(Some("foo"), Some("bar"), Some("baz")).with_extras(vec!["x1", "x2"])
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4); // header + row + 2 extras
        assert_eq!(lines[2], "    x1");
        assert_eq!(lines[3], "    x2");
    }

    #[test]
    fn format_table_empty_extras_is_no_op() {
        let out = render_table(&[Demo::new(Some("foo"), Some("bar"), Some("baz"))]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn format_table_extras_preserve_order() {
        let out = render_table(&[Demo::new(Some("a"), Some("b"), Some("c"))
            .with_extras(vec!["first", "second", "third"])]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[2], "    first");
        assert_eq!(lines[3], "    second");
        assert_eq!(lines[4], "    third");
    }

    #[test]
    fn format_table_single_annotation_no_separator() {
        let out =
            render_table(&[Demo::new(Some("a"), Some("b"), Some("c")).with_anns(vec!["only"])]);
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[1].ends_with("  only"),
            "missing single-annotation suffix: {:?}",
            lines[1]
        );
    }

    #[test]
    fn format_table_multiple_annotations_join_with_comma_space() {
        let out =
            render_table(&[Demo::new(Some("a"), Some("b"), Some("c"))
                .with_anns(vec!["alpha", "beta", "gamma"])]);
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[1].ends_with("  alpha, beta, gamma"),
            "annotations not joined with ', ': {:?}",
            lines[1]
        );
    }

    #[test]
    fn format_table_empty_annotations_no_suffix() {
        let out = render_table(&[Demo::new(Some("a"), Some("b"), Some("c"))]);
        let lines: Vec<&str> = out.lines().collect();
        // AAA(Left,w=3)="a  ", BBB(Right,w=3)="  b", CCC(Left,w=3)="c  " -> trimmed
        // Gutter + 3 cells joined by " " then trim_end:
        assert_eq!(lines[1], "  a     b c");
    }

    #[test]
    fn format_table_annotations_and_extras_together() {
        let out = render_table(&[Demo::new(Some("a"), Some("b"), Some("c"))
            .with_anns(vec!["dirty (5 files)", "diverged"])
            .with_extras(vec!["note"])]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with("  dirty (5 files), diverged"));
        assert_eq!(lines[2], "    note");
    }

    #[test]
    fn format_table_widths_widen_with_longest_cell() {
        let out = render_table(&[
            Demo::new(Some("short"), Some("a"), Some("c")),
            Demo::new(Some("a-much-longer-cell-value"), Some("b"), Some("d")),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        let aaa_width = "a-much-longer-cell-value".len();
        let bbb_width = 3; // header "BBB" longer than "a"/"b"
        let ccc_width = 3; // header "CCC" longer than "c"/"d"
        let expected_header = format!(
            "  {:<aw$} {:>bw$} {:<cw$}",
            "AAA",
            "BBB",
            "CCC",
            aw = aaa_width,
            bw = bbb_width,
            cw = ccc_width,
        );
        assert_eq!(lines[0], expected_header.trim_end());
    }

    #[test]
    fn format_table_header_width_dominates_when_no_cell_is_longer() {
        let out = render_table(&[Demo::new(Some("x"), Some("y"), Some("z"))]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "  AAA BBB CCC");
    }

    #[test]
    fn format_porcelain_empty_input_writes_nothing() {
        let items: Vec<Demo> = vec![];
        let mut buf = Vec::new();
        format_porcelain(&mut buf, &items).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn format_porcelain_joins_fields_with_tab() {
        let out = render_porcelain(&[Demo::new(Some("foo"), Some("bar"), Some("baz"))]);
        // Porcelain fields are [repo, a, b, c] per Demo's impl.
        assert_eq!(out, "/repo\tfoo\tbar\tbaz\n");
    }

    #[test]
    fn format_porcelain_one_line_per_item() {
        let out = render_porcelain(&[
            Demo::new(Some("a"), Some("b"), Some("c")),
            Demo::new(Some("d"), Some("e"), Some("f")),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "/repo\ta\tb\tc");
        assert_eq!(lines[1], "/repo\td\te\tf");
    }

    #[test]
    fn format_porcelain_preserves_empty_fields() {
        let out = render_porcelain(&[Demo::new(Some("a"), None, Some("c"))]);
        // None -> "" in Demo's porcelain_fields impl; result has adjacent tabs.
        assert_eq!(out, "/repo\ta\t\tc\n");
    }

    #[test]
    fn format_porcelain_field_count_stable_across_rows() {
        let out = render_porcelain(&[
            Demo::new(Some("a"), Some("b"), Some("c")),
            Demo::new(None, None, None),
        ]);
        for line in out.lines() {
            assert_eq!(
                line.split('\t').count(),
                4,
                "field count mismatch: {line:?}"
            );
        }
    }

    #[test]
    fn format_porcelain_no_trailing_newline_swallowed() {
        // Each line ends with a single \n; no extra trailing newline.
        let out = render_porcelain(&[Demo::new(Some("a"), Some("b"), Some("c"))]);
        assert_eq!(out.matches('\n').count(), 1);
        assert!(out.ends_with('\n'));
    }

    // --- regression tests for /review-loop findings ---

    #[test]
    fn format_table_skips_empty_annotation_tokens() {
        struct Item;
        impl TidyItem for Item {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("X")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Borrowed("v"))]
            }
            fn annotations(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed(""), Cow::Borrowed("note"), Cow::Borrowed("")]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[Item]).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        // No stray "," from empty tokens; only the non-empty "note" remains.
        assert_eq!(lines[1], "  v  note");
    }

    #[test]
    fn format_table_all_empty_annotations_emit_no_suffix() {
        struct Item;
        impl TidyItem for Item {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("X")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Borrowed("v"))]
            }
            fn annotations(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed(""), Cow::Borrowed("")]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[Item]).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[1], "  v");
    }

    #[test]
    fn format_table_writes_nothing_when_all_columns_hidden() {
        struct HiddenItem;
        impl TidyItem for HiddenItem {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("A"), ColumnSpec::left("B")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![None, None]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[HiddenItem, HiddenItem]).unwrap();
        assert!(buf.is_empty(), "expected no output, got {buf:?}");
    }

    #[test]
    fn format_table_writes_nothing_when_columns_const_is_empty() {
        struct ZeroCols;
        impl TidyItem for ZeroCols {
            const COLUMNS: &'static [ColumnSpec] = &[];
            fn row(&self) -> Vec<Option<Cell>> {
                Vec::new()
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[ZeroCols]).unwrap();
        assert!(buf.is_empty());
    }

    #[test]
    fn format_table_aligns_wide_unicode_cells_by_display_width() {
        // "中文" is two CJK chars; UAX-11 width = 4 cells, byte length = 6.
        // Header "X" (width 1) padded to width 4 must produce "X   " (3 trailing
        // spaces), and the cell "中文" itself occupies 4 visual cells. With a
        // trailing single-column cell "y", the row line is:
        //   "  " + "中文" + " " + "y" => "  中文 y"
        // and the header line is:
        //   "  " + "X   " + " " + "Y" => "  X    Y" (trim_end keeps 4 spaces because Y is non-empty)
        struct Wide;
        impl TidyItem for Wide {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("X"), ColumnSpec::left("Y")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Borrowed("中文")), Some(Cow::Borrowed("y"))]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[Wide]).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "  X    Y");
        assert_eq!(lines[1], "  中文 y");
    }

    #[test]
    fn format_table_trims_trailing_whitespace_from_row_extras() {
        // The format_table contract promises trailing whitespace is trimmed from
        // every rendered line. Row extras are rendered lines too — they must
        // obey the same rule, otherwise stale " " trailing on an upstream
        // git subject leaks into our output.
        struct Item;
        impl TidyItem for Item {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("X")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Borrowed("v"))]
            }
            fn row_extras(&self) -> Vec<Cow<'static, str>> {
                vec![
                    Cow::Borrowed("note   "),
                    Cow::Borrowed("clean"),
                    Cow::Borrowed("trailing-tab\t"),
                ]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        format_table(&mut buf, &[Item]).unwrap();
        let out = String::from_utf8(buf).unwrap();
        for line in out.lines() {
            assert_eq!(
                line.trim_end(),
                line,
                "trailing ws on extras line: {line:?}"
            );
        }
        // And confirm the indent is preserved (just the trailing ws is gone).
        assert!(out.contains("\n    note\n"));
        assert!(out.contains("\n    clean\n"));
        assert!(out.contains("\n    trailing-tab\n"));
    }

    #[test]
    #[should_panic(expected = "TidyItem::row() length")]
    fn format_table_panics_in_debug_when_row_length_mismatches_columns() {
        struct Bad;
        impl TidyItem for Bad {
            const COLUMNS: &'static [ColumnSpec] = &[ColumnSpec::left("A"), ColumnSpec::left("B")];
            fn row(&self) -> Vec<Option<Cell>> {
                vec![Some(Cow::Borrowed("only-one"))]
            }
            fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
                vec![Cow::Borrowed("/repo")]
            }
        }
        let mut buf = Vec::new();
        let _ = format_table(&mut buf, &[Bad]);
    }

    // --- TidyItem for WorktreeInfo tests ---

    mod worktree_impl {
        use super::*;
        use crate::types::{Annotations, Classification, UnmatchedCommit, WorktreeInfo};
        use std::path::PathBuf;

        fn landed_worktree() -> WorktreeInfo {
            WorktreeInfo {
                path: PathBuf::from("/dev/Backend-parallel"),
                parent_repo: PathBuf::from("/repos/Backend"),
                branch: Some("fix/skip-db-init".to_string()),
                default_branch: "main".to_string(),
                classification: Classification::Landed,
                annotations: Annotations::default(),
                remote_tracking: true,
                ahead: 0,
                behind: 0,
                dirty_files: vec![],
                meaningful_dirty_files: vec![],
            }
        }

        fn active_with_ahead() -> WorktreeInfo {
            WorktreeInfo {
                path: PathBuf::from("/dev/Backend-caps"),
                parent_repo: PathBuf::from("/repos/Backend"),
                branch: Some("feature/caps".to_string()),
                default_branch: "main".to_string(),
                classification: Classification::Active,
                annotations: Annotations::default(),
                remote_tracking: true,
                ahead: 3,
                behind: 0,
                dirty_files: vec![],
                meaningful_dirty_files: vec![],
            }
        }

        fn partial_dirty_diverged() -> WorktreeInfo {
            WorktreeInfo {
                path: PathBuf::from("/dev/App-theme"),
                parent_repo: PathBuf::from("/repos/App"),
                branch: Some("alternate-icons".to_string()),
                default_branch: "main".to_string(),
                classification: Classification::LandedPartial {
                    matched: 4,
                    total: 6,
                    unmatched: vec![
                        UnmatchedCommit {
                            short_hash: "8d8a06c".to_string(),
                            subject: "Add app icon button".to_string(),
                        },
                        UnmatchedCommit {
                            short_hash: "b4cd142".to_string(),
                            subject: "Add themed icons".to_string(),
                        },
                    ],
                },
                annotations: Annotations {
                    dirty: true,
                    dirty_file_count: 5,
                    diverged: true,
                    ..Default::default()
                },
                remote_tracking: true,
                ahead: 6,
                behind: 324,
                // Production invariant: meaningful_dirty_files is dirty_files
                // after noise filtering, so |meaningful| <= |dirty|. Keep both
                // at 5 here so the fixture matches what the scanner can emit.
                dirty_files: vec!["a".into(); 5],
                meaningful_dirty_files: vec!["a".into(); 5],
            }
        }

        fn cell(c: &Option<Cell>) -> Option<&str> {
            c.as_deref()
        }

        #[test]
        fn row_has_five_cells() {
            assert_eq!(landed_worktree().row().len(), 5);
        }

        #[test]
        fn row_landed_has_no_ratio_no_ahead_behind() {
            let row = landed_worktree().row();
            assert_eq!(cell(&row[0]), Some("landed"));
            assert_eq!(cell(&row[3]), None, "RATIO should be None for Landed");
            assert_eq!(cell(&row[4]), None, "AHEAD/BEHIND should be None when 0/0");
        }

        #[test]
        fn row_active_with_ahead_has_ahead_behind() {
            let row = active_with_ahead().row();
            assert_eq!(cell(&row[0]), Some("active"));
            assert_eq!(cell(&row[3]), None);
            assert_eq!(cell(&row[4]), Some("+3/-0"));
        }

        #[test]
        fn row_partial_has_ratio_and_ahead_behind() {
            let row = partial_dirty_diverged().row();
            assert_eq!(cell(&row[0]), Some("partial"));
            assert_eq!(cell(&row[3]), Some("4/6"));
            assert_eq!(cell(&row[4]), Some("+6/-324"));
        }

        #[test]
        fn row_detached_branch_renders_marker() {
            let mut wt = landed_worktree();
            wt.branch = None;
            let row = wt.row();
            assert_eq!(cell(&row[2]), Some("(detached)"));
        }

        #[test]
        fn row_directory_uses_path_file_name() {
            let row = landed_worktree().row();
            assert_eq!(cell(&row[1]), Some("Backend-parallel"));
        }

        #[test]
        fn annotations_empty_when_clean() {
            assert!(landed_worktree().annotations().is_empty());
        }

        #[test]
        fn annotations_dirty_diverged_remote_deleted_in_order() {
            let mut wt = landed_worktree();
            wt.annotations = Annotations {
                dirty: true,
                dirty_file_count: 7,
                diverged: true,
                remote_deleted: true,
            };
            let anns: Vec<String> = wt
                .annotations()
                .into_iter()
                .map(|c| c.into_owned())
                .collect();
            assert_eq!(anns, vec!["dirty (7 files)", "diverged", "remote deleted"]);
        }

        #[test]
        fn annotations_dirty_pluralizes_count() {
            let mut wt = landed_worktree();
            wt.annotations.dirty = true;

            wt.annotations.dirty_file_count = 1;
            assert_eq!(wt.annotations()[0].as_ref(), "dirty (1 file)");

            wt.annotations.dirty_file_count = 2;
            assert_eq!(wt.annotations()[0].as_ref(), "dirty (2 files)");

            wt.annotations.dirty_file_count = 0;
            assert_eq!(wt.annotations()[0].as_ref(), "dirty (0 files)");
        }

        #[test]
        fn row_extras_landed_partial_lists_unmatched_commits() {
            let extras: Vec<String> = partial_dirty_diverged()
                .row_extras()
                .into_iter()
                .map(|c| c.into_owned())
                .collect();
            assert_eq!(
                extras,
                vec![
                    "unmatched: 8d8a06c Add app icon button",
                    "unmatched: b4cd142 Add themed icons",
                ]
            );
        }

        #[test]
        fn row_extras_empty_for_non_partial() {
            assert!(landed_worktree().row_extras().is_empty());
            assert!(active_with_ahead().row_extras().is_empty());
        }

        #[test]
        fn porcelain_fields_count_is_nine() {
            assert_eq!(landed_worktree().porcelain_fields().len(), 9);
            assert_eq!(active_with_ahead().porcelain_fields().len(), 9);
            assert_eq!(partial_dirty_diverged().porcelain_fields().len(), 9);
        }

        #[test]
        fn porcelain_field_zero_is_worktree_dir_and_field_one_is_parent_repo() {
            // worktree-tidy preserves its historical porcelain shape:
            // field [0] is the worktree directory, field [1] is the parent repo.
            // The trait doc calls this out as the exception to the otherwise
            // repo-path-first convention.
            let fields = landed_worktree().porcelain_fields();
            assert_eq!(fields[0].as_ref(), "/dev/Backend-parallel");
            assert_eq!(fields[1].as_ref(), "/repos/Backend");
        }

        #[test]
        fn porcelain_fields_order_matches_documented_contract() {
            let fields: Vec<String> = partial_dirty_diverged()
                .porcelain_fields()
                .into_iter()
                .map(|c| c.into_owned())
                .collect();
            // [path, parent, branch, class, ratio, ahead, behind, dirty_count, annotations]
            assert_eq!(fields[0], "/dev/App-theme");
            assert_eq!(fields[1], "/repos/App");
            assert_eq!(fields[2], "alternate-icons");
            assert_eq!(fields[3], "partial");
            assert_eq!(fields[4], "4/6");
            assert_eq!(fields[5], "6");
            assert_eq!(fields[6], "324");
            assert_eq!(fields[7], "5");
            assert_eq!(fields[8], "diverged,dirty");
        }

        #[test]
        fn porcelain_branch_empty_when_detached() {
            let mut wt = landed_worktree();
            wt.branch = None;
            let fields = wt.porcelain_fields();
            assert_eq!(fields[2].as_ref(), "");
        }

        #[test]
        fn porcelain_annotations_csv_order_remote_diverged_dirty() {
            let mut wt = landed_worktree();
            wt.annotations = Annotations {
                dirty: true,
                dirty_file_count: 2,
                diverged: true,
                remote_deleted: true,
            };
            let fields = wt.porcelain_fields();
            assert_eq!(fields[8].as_ref(), "remote_deleted,diverged,dirty");
        }
    }
}
