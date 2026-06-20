use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};
use git_tidy_core::types::ClassificationLabel;

use crate::types::{TagInfo, TagScanResult, commit_short};

impl TidyItem for TagInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("NAME"),
        ColumnSpec::left("COMMIT"),
        ColumnSpec::left("KIND"),
        ColumnSpec::left("DATE"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        let kind: Cell = if self.is_annotated {
            Cow::Borrowed("annotated")
        } else {
            Cow::Borrowed("lightweight")
        };
        vec![
            Some(Cow::Borrowed(self.classification.label())),
            Some(Cow::Owned(self.name.clone())),
            Some(Cow::Owned(commit_short(&self.commit).to_string())),
            Some(kind),
            // Conditional DATE column: None when absent so format_table hides
            // the column iff every row's DATE cell is None.
            self.tagger_date.as_ref().map(|d| Cow::Owned(d.clone())),
        ]
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        vec![
            Cow::Owned(self.repo_path.display().to_string()),
            Cow::Owned(self.name.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(self.commit.clone()),
            Cow::Owned(self.is_annotated.to_string()),
            Cow::Owned(self.tagger_date.clone().unwrap_or_default()),
            Cow::Owned(self.is_release_tag.to_string()),
            Cow::Owned(self.remote_names.join(",")),
        ]
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.tags.len() == 1 { "tag" } else { "tags" };
        writeln!(out, "\n{} ({} {noun})", group.name, group.tags.len())?;
        shared::format_table(out, &group.tags)?;
    }

    write_tag_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Ordered `(display, count key)` pairs for the tag summary breakdown.
const TAG_SUMMARY: &[(&str, &str)] = &[
    ("stale", "stale"),
    ("local_only", "local_only"),
    ("remote_only", "remote_only"),
    ("synced", "synced"),
];

/// Write the tag-specific summary line.
fn write_tag_summary(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    shared::write_summary_line(
        out,
        result.total_scanned,
        &result.counts,
        "tags",
        TAG_SUMMARY,
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        shared::format_porcelain(out, &group.tags)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;

    fn make_scan_result() -> TagScanResult {
        TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                tags: vec![
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "old-experiment".to_string(),
                        classification: TagClassification::Stale,
                        commit: "abc1234def5678".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "feature-v2-wip".to_string(),
                        classification: TagClassification::LocalOnly,
                        commit: "def5678abc1234".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "v1.0.0".to_string(),
                        classification: TagClassification::Synced,
                        commit: "789abcdef1234".to_string(),
                        is_annotated: true,
                        tagger_date: Some("2024-06-15T10:00:00+00:00".to_string()),
                        is_release_tag: true,
                        remote_names: vec!["origin".to_string()],
                    },
                ],
            }],
            total_scanned: 3,
            counts: Counts::from_pairs(&[("stale", 1), ("local_only", 1), ("synced", 1)]),
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("backend (3 tags)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("NAME"));
        assert!(output.contains("COMMIT"));
        assert!(output.contains("KIND"));
        // At least one tag has tagger_date set, so DATE column is visible.
        assert!(output.contains("DATE"));
        assert!(output.contains("stale"));
        assert!(output.contains("local_only"));
        assert!(output.contains("synced"));
        assert!(output.contains("old-experiment"));
        assert!(output.contains("v1.0.0"));
        assert!(output.contains("annotated"));
        assert!(output.contains("lightweight"));
        assert!(output.contains("3 tags scanned: 1 stale, 1 local_only, 0 remote_only, 1 synced"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = TagScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["could not list tags for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list tags for /repo/Foo"));
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
        assert_eq!(arr[0]["classification"], "stale");
        assert_eq!(arr[0]["name"], "old-experiment");
        assert!(!arr[0]["is_annotated"].as_bool().unwrap());
        assert_eq!(arr[2]["classification"], "synced");
        assert_eq!(arr[2]["name"], "v1.0.0");
        assert!(arr[2]["is_annotated"].as_bool().unwrap());
        assert!(arr[2]["is_release_tag"].as_bool().unwrap());
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
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "old-experiment");
        assert_eq!(fields[2], "stale");
        assert_eq!(fields[3], "abc1234def5678");
        assert_eq!(fields[4], "false");
        assert_eq!(fields[5], ""); // no date
        assert_eq!(fields[6], "false");
        assert_eq!(fields[7], ""); // no remotes
    }

    #[test]
    fn human_output_single_tag_noun() {
        let result = TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                tags: vec![TagInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    name: "v1.0".to_string(),
                    classification: TagClassification::Synced,
                    commit: "abc1234".to_string(),
                    is_annotated: false,
                    tagger_date: None,
                    is_release_tag: true,
                    remote_names: vec![],
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("synced", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test (1 tag)"));
    }

    #[test]
    fn human_output_omits_date_column_when_all_missing() {
        // When no tag in the group carries a tagger_date, format_table's
        // all-None auto-hide rule should drop the DATE column entirely.
        let result = TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/r"),
                name: "r".to_string(),
                tags: vec![
                    TagInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "a".to_string(),
                        classification: TagClassification::Stale,
                        commit: "abc1234".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "b".to_string(),
                        classification: TagClassification::Synced,
                        commit: "def5678".to_string(),
                        is_annotated: true,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec!["origin".to_string()],
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("stale", 1), ("synced", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("DATE"),
            "DATE column should be hidden when no row has a date: {output}"
        );
    }

    #[test]
    fn human_output_shows_date_column_when_any_present() {
        // When at least one tag has a tagger_date, the DATE column appears,
        // and rows without a date should still render cleanly.
        let result = TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/r"),
                name: "r".to_string(),
                tags: vec![
                    TagInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "lightweight-undated".to_string(),
                        classification: TagClassification::Stale,
                        commit: "abc1234".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "annotated-dated".to_string(),
                        classification: TagClassification::Synced,
                        commit: "def5678".to_string(),
                        is_annotated: true,
                        tagger_date: Some("2024-06-15T10:00:00+00:00".to_string()),
                        is_release_tag: false,
                        remote_names: vec!["origin".to_string()],
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("stale", 1), ("synced", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("DATE"), "DATE header missing: {output}");
        assert!(output.contains("2024-06-15T10:00:00+00:00"));
        assert!(output.contains("lightweight-undated"));
        assert!(output.contains("annotated-dated"));
    }

    // --- TidyItem data-shape tests ---

    fn tag_info(
        name: &str,
        classification: TagClassification,
        commit: &str,
        is_annotated: bool,
        tagger_date: Option<&str>,
        is_release_tag: bool,
        remote_names: &[&str],
    ) -> TagInfo {
        TagInfo {
            repo_path: PathBuf::from("/repos/backend"),
            name: name.to_string(),
            classification,
            commit: commit.to_string(),
            is_annotated,
            tagger_date: tagger_date.map(|s| s.to_string()),
            is_release_tag,
            remote_names: remote_names.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn tidyitem_row_shape_tag() {
        let row = tag_info(
            "v1.0.0",
            TagClassification::Synced,
            "abc1234def5678",
            true,
            Some("2024-06-15T10:00:00+00:00"),
            true,
            &["origin"],
        )
        .row();
        assert_eq!(row.len(), 5);
        assert_eq!(row[0].as_deref(), Some("synced"));
        assert_eq!(row[1].as_deref(), Some("v1.0.0"));
        assert_eq!(row[2].as_deref(), Some("abc1234"));
        assert_eq!(row[3].as_deref(), Some("annotated"));
        assert_eq!(row[4].as_deref(), Some("2024-06-15T10:00:00+00:00"));
    }

    #[test]
    fn tidyitem_row_date_none_renders_none_not_empty() {
        // tagger_date: None must yield a literal None cell (not Some("")) so
        // format_table's all-None auto-hide rule can drop the DATE column.
        let row = tag_info(
            "x",
            TagClassification::Stale,
            "abc1234",
            false,
            None,
            false,
            &[],
        )
        .row();
        assert!(row[4].is_none());
        // Lightweight tag should yield "lightweight" in KIND.
        assert_eq!(row[3].as_deref(), Some("lightweight"));
    }

    #[test]
    fn tidyitem_porcelain_tag_full_commit_and_remote_join() {
        let fields = tag_info(
            "v1.0.0",
            TagClassification::Synced,
            "abc1234def5678",
            true,
            Some("2024-06-15T10:00:00+00:00"),
            true,
            &["origin", "upstream"],
        )
        .porcelain_fields();
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[0].as_ref(), "/repos/backend");
        assert_eq!(fields[1].as_ref(), "v1.0.0");
        assert_eq!(fields[2].as_ref(), "synced");
        // Full commit, not shortened.
        assert_eq!(fields[3].as_ref(), "abc1234def5678");
        assert_eq!(fields[4].as_ref(), "true");
        assert_eq!(fields[5].as_ref(), "2024-06-15T10:00:00+00:00");
        assert_eq!(fields[6].as_ref(), "true");
        // Remotes are comma-joined (no spaces).
        assert_eq!(fields[7].as_ref(), "origin,upstream");
    }

    #[test]
    fn tidyitem_porcelain_date_none_is_empty_field() {
        let fields = tag_info(
            "x",
            TagClassification::Stale,
            "abc1234",
            false,
            None,
            false,
            &[],
        )
        .porcelain_fields();
        assert_eq!(fields[5].as_ref(), "");
        assert_eq!(fields[7].as_ref(), "");
    }
}
