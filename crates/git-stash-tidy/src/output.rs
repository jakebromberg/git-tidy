use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};
use git_tidy_core::types::ClassificationLabel;

use crate::types::{StashInfo, StashScanResult};

impl TidyItem for StashInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("REF"),
        ColumnSpec::left("MESSAGE"),
        ColumnSpec::left("AGE"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        // AGE cell is always Some (possibly empty string) so the column stays
        // visible — matching pre-migration behavior. Stash entries with no
        // recorded age render a blank AGE cell rather than hiding the column.
        let age: Cell = Cow::Owned(match self.age_days {
            Some(0) => "today".to_string(),
            Some(1) => "1 day ago".to_string(),
            Some(d) => format!("{d} days ago"),
            None => String::new(),
        });
        vec![
            Some(Cow::Borrowed(self.classification.label())),
            Some(Cow::Owned(self.stash_ref.clone())),
            Some(Cow::Owned(self.message.clone())),
            Some(age),
        ]
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        vec![
            Cow::Owned(self.repo_path.display().to_string()),
            Cow::Owned(self.stash_ref.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(self.branch.clone().unwrap_or_default()),
            Cow::Owned(self.age_days.map(|d| d.to_string()).unwrap_or_default()),
            Cow::Owned(self.message.clone()),
        ]
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(out, "\n{} ({} stashes)", group.name, group.items.len())?;
        shared::format_table(out, &group.items)?;
    }

    write_stash_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Ordered `(display, count key)` pairs for the stash summary breakdown.
const STASH_SUMMARY: &[(&str, &str)] = &[
    ("committed", "committed"),
    ("orphaned", "orphaned"),
    ("aged", "aged"),
    ("active", "active"),
];

/// Write the stash-specific summary line.
fn write_stash_summary(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    shared::write_summary_line(
        out,
        result.total_scanned,
        &result.counts,
        "stashes",
        STASH_SUMMARY,
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        shared::format_porcelain(out, &group.items)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;
    use git_tidy_core::scan::RepoGroup;

    fn make_scan_result() -> StashScanResult {
        StashScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/my-repo"),
                name: "my-repo".to_string(),
                items: vec![
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{0}".to_string(),
                        classification: StashClassification::Committed,
                        branch: Some("feature-x".to_string()),
                        age_days: Some(23),
                        message: "WIP on feature-x: abc1234 Add login".to_string(),
                    },
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{1}".to_string(),
                        classification: StashClassification::Orphaned,
                        branch: Some("deleted-branch".to_string()),
                        age_days: Some(45),
                        message: "WIP on deleted-branch: def5678 Fix UI".to_string(),
                    },
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{2}".to_string(),
                        classification: StashClassification::Active,
                        branch: Some("main".to_string()),
                        age_days: Some(2),
                        message: "WIP on main: ghi9012 Temp changes".to_string(),
                    },
                ],
            }],
            total_scanned: 3,
            counts: Counts::from_pairs(&[("committed", 1), ("orphaned", 1), ("active", 1)]),
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("my-repo (3 stashes)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("REF"));
        assert!(output.contains("MESSAGE"));
        assert!(output.contains("AGE"));
        assert!(output.contains("committed"));
        assert!(output.contains("orphaned"));
        assert!(output.contains("active"));
        assert!(output.contains("stash@{0}"));
        assert!(output.contains("23 days ago"));
        assert!(output.contains("45 days ago"));
        assert!(output.contains("2 days ago"));
        assert!(output.contains("3 stashes scanned: 1 committed, 1 orphaned, 0 aged, 1 active"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = StashScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["could not list stashes for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list stashes for /repo/Foo"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["classification"], "committed");
        assert_eq!(arr[0]["branch"], "feature-x");
        assert_eq!(arr[0]["age_days"], 23);
        assert_eq!(arr[1]["classification"], "orphaned");
        assert_eq!(arr[2]["classification"], "active");
    }

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "/repos/my-repo");
        assert_eq!(fields[1], "stash@{0}");
        assert_eq!(fields[2], "committed");
        assert_eq!(fields[3], "feature-x");
        assert_eq!(fields[4], "23");
        assert_eq!(fields[5], "WIP on feature-x: abc1234 Add login");
    }

    // --- TidyItem data-shape tests ---

    fn stash_info(
        stash_ref: &str,
        classification: StashClassification,
        branch: Option<&str>,
        age_days: Option<u64>,
        message: &str,
    ) -> StashInfo {
        StashInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            stash_ref: stash_ref.to_string(),
            classification,
            branch: branch.map(|s| s.to_string()),
            age_days,
            message: message.to_string(),
        }
    }

    #[test]
    fn tidyitem_row_shape_stash() {
        let row = stash_info(
            "stash@{0}",
            StashClassification::Committed,
            Some("feature-x"),
            Some(23),
            "WIP on feature-x: abc1234 Add login",
        )
        .row();
        assert_eq!(row.len(), 4);
        assert_eq!(row[0].as_deref(), Some("committed"));
        assert_eq!(row[1].as_deref(), Some("stash@{0}"));
        assert_eq!(
            row[2].as_deref(),
            Some("WIP on feature-x: abc1234 Add login")
        );
        assert_eq!(row[3].as_deref(), Some("23 days ago"));
    }

    #[test]
    fn tidyitem_row_age_today_and_one_day() {
        let row_today =
            stash_info("stash@{0}", StashClassification::Active, None, Some(0), "m").row();
        assert_eq!(row_today[3].as_deref(), Some("today"));

        let row_yesterday =
            stash_info("stash@{0}", StashClassification::Active, None, Some(1), "m").row();
        assert_eq!(row_yesterday[3].as_deref(), Some("1 day ago"));
    }

    #[test]
    fn tidyitem_row_age_none_renders_empty_cell() {
        // age_days: None must yield Some("") so the AGE column stays visible
        // even when no stash has a recorded age — matching pre-migration
        // behavior.
        let row = stash_info("stash@{0}", StashClassification::Active, None, None, "m").row();
        assert_eq!(row[3].as_deref(), Some(""));
    }

    #[test]
    fn tidyitem_porcelain_stash_field_order() {
        let fields = stash_info(
            "stash@{0}",
            StashClassification::Committed,
            Some("feature-x"),
            Some(23),
            "WIP on feature-x: abc1234 Add login",
        )
        .porcelain_fields();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0].as_ref(), "/repos/my-repo");
        assert_eq!(fields[1].as_ref(), "stash@{0}");
        assert_eq!(fields[2].as_ref(), "committed");
        assert_eq!(fields[3].as_ref(), "feature-x");
        assert_eq!(fields[4].as_ref(), "23");
        assert_eq!(fields[5].as_ref(), "WIP on feature-x: abc1234 Add login");
    }

    #[test]
    fn tidyitem_porcelain_null_branch_and_age_are_empty_fields() {
        let fields = stash_info("stash@{0}", StashClassification::Active, None, None, "m")
            .porcelain_fields();
        assert_eq!(fields[3].as_ref(), "");
        assert_eq!(fields[4].as_ref(), "");
    }
}
