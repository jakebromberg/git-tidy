use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};
use git_tidy_core::types::ClassificationLabel;

use crate::types::{RemoteInfo, RemoteScanResult};

impl TidyItem for RemoteInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("NAME"),
        ColumnSpec::left("URL"),
        ColumnSpec::left("TRACKING"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        // Conditional TRACKING column: None when this remote has no tracking
        // refs, so format_table hides the column iff every row's TRACKING cell
        // is None — matching the legacy `has_tracking` auto-hide.
        let tracking: Option<Cell> = if self.tracking_count > 0 {
            let branch_noun = if self.tracking_count == 1 {
                "tracking branch"
            } else {
                "tracking branches"
            };
            Some(Cow::Owned(format!(
                "({} {branch_noun})",
                self.tracking_count
            )))
        } else {
            None
        };
        vec![
            Some(Cow::Borrowed(self.classification.label())),
            Some(Cow::Owned(self.name.clone())),
            Some(Cow::Owned(self.url.clone().unwrap_or_default())),
            tracking,
        ]
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        vec![
            Cow::Owned(self.repo_path.display().to_string()),
            Cow::Owned(self.name.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(self.url.clone().unwrap_or_default()),
            Cow::Owned(self.tracking_count.to_string()),
            Cow::Owned(self.is_origin.to_string()),
        ]
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.remotes.len() == 1 {
            "remote"
        } else {
            "remotes"
        };
        writeln!(out, "\n{} ({} {noun})", group.name, group.remotes.len())?;
        shared::format_table(out, &group.remotes)?;
    }

    write_remote_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the remote-specific summary line.
fn write_remote_summary(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} remotes scanned: {} unreachable, {} orphaned, {} active",
        result.total_scanned,
        c.get("unreachable"),
        c.get("orphaned"),
        c.get("active"),
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        shared::format_porcelain(out, &group.remotes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;

    fn make_scan_result() -> RemoteScanResult {
        RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                remotes: vec![
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "origin".to_string(),
                        classification: RemoteClassification::Unreachable,
                        url: Some("https://github.com/old-org/backend.git".to_string()),
                        tracking_count: 12,
                        is_origin: true,
                    },
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "upstream".to_string(),
                        classification: RemoteClassification::Active,
                        url: Some("https://github.com/new-org/backend.git".to_string()),
                        tracking_count: 5,
                        is_origin: false,
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("unreachable", 1), ("active", 1)]),
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("backend (2 remotes)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("NAME"));
        assert!(output.contains("URL"));
        assert!(output.contains("TRACKING"));
        assert!(output.contains("unreachable"));
        assert!(output.contains("active"));
        assert!(output.contains("origin"));
        assert!(output.contains("upstream"));
        assert!(output.contains("12 tracking branches"));
        assert!(output.contains("5 tracking branches"));
        assert!(output.contains("2 remotes scanned: 1 unreachable, 0 orphaned, 1 active"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = RemoteScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["could not list remotes for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list remotes for /repo/Foo"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["classification"], "unreachable");
        assert_eq!(arr[0]["name"], "origin");
        assert_eq!(arr[0]["tracking_count"], 12);
        assert!(arr[0]["is_origin"].as_bool().unwrap());
        assert_eq!(arr[1]["classification"], "active");
        assert_eq!(arr[1]["name"], "upstream");
    }

    #[test]
    fn json_output_orphaned_null_url() {
        let result = RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                remotes: vec![RemoteInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    name: "stale".to_string(),
                    classification: RemoteClassification::Orphaned,
                    url: None,
                    tracking_count: 3,
                    is_origin: false,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("orphaned", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert!(arr[0]["url"].is_null());
        assert_eq!(arr[0]["classification"], "orphaned");
    }

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "origin");
        assert_eq!(fields[2], "unreachable");
        assert_eq!(fields[3], "https://github.com/old-org/backend.git");
        assert_eq!(fields[4], "12");
        assert_eq!(fields[5], "true");
    }

    // --- TidyItem data-shape tests ---

    fn remote_info(
        name: &str,
        classification: RemoteClassification,
        url: Option<&str>,
        tracking_count: usize,
        is_origin: bool,
    ) -> RemoteInfo {
        RemoteInfo {
            repo_path: PathBuf::from("/repos/backend"),
            name: name.to_string(),
            classification,
            url: url.map(|s| s.to_string()),
            tracking_count,
            is_origin,
        }
    }

    #[test]
    fn tidyitem_row_shape_remote() {
        let row = remote_info(
            "origin",
            RemoteClassification::Active,
            Some("https://github.com/o/r.git"),
            5,
            true,
        )
        .row();
        assert_eq!(row.len(), 4);
        assert_eq!(row[0].as_deref(), Some("active"));
        assert_eq!(row[1].as_deref(), Some("origin"));
        assert_eq!(row[2].as_deref(), Some("https://github.com/o/r.git"));
        assert_eq!(row[3].as_deref(), Some("(5 tracking branches)"));
    }

    #[test]
    fn tidyitem_row_tracking_singular_noun() {
        let row = remote_info(
            "upstream",
            RemoteClassification::Active,
            Some("u"),
            1,
            false,
        )
        .row();
        assert_eq!(row[3].as_deref(), Some("(1 tracking branch)"));
    }

    #[test]
    fn tidyitem_row_tracking_zero_is_none() {
        // tracking_count: 0 must yield a literal None cell so format_table's
        // all-None auto-hide rule can drop the TRACKING column.
        let row = remote_info("a", RemoteClassification::Active, Some("u"), 0, false).row();
        assert!(row[3].is_none());
    }

    #[test]
    fn tidyitem_row_orphaned_null_url_renders_empty() {
        let row = remote_info("stale", RemoteClassification::Orphaned, None, 3, false).row();
        assert_eq!(row[2].as_deref(), Some(""));
    }

    #[test]
    fn human_output_hides_tracking_column_when_all_zero() {
        let result = RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/r"),
                name: "r".to_string(),
                remotes: vec![
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "a".to_string(),
                        classification: RemoteClassification::Active,
                        url: Some("ua".to_string()),
                        tracking_count: 0,
                        is_origin: false,
                    },
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/r"),
                        name: "b".to_string(),
                        classification: RemoteClassification::Active,
                        url: Some("ub".to_string()),
                        tracking_count: 0,
                        is_origin: false,
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("active", 2)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            !output.contains("TRACKING"),
            "TRACKING column should be hidden when no row has tracking refs: {output}"
        );
    }

    #[test]
    fn tidyitem_porcelain_remote_field_order() {
        let fields = remote_info(
            "origin",
            RemoteClassification::Active,
            Some("https://github.com/o/r.git"),
            5,
            true,
        )
        .porcelain_fields();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0].as_ref(), "/repos/backend");
        assert_eq!(fields[1].as_ref(), "origin");
        assert_eq!(fields[2].as_ref(), "active");
        assert_eq!(fields[3].as_ref(), "https://github.com/o/r.git");
        assert_eq!(fields[4].as_ref(), "5");
        assert_eq!(fields[5].as_ref(), "true");
    }

    #[test]
    fn tidyitem_porcelain_null_url_is_empty_field() {
        let fields =
            remote_info("stale", RemoteClassification::Orphaned, None, 3, false).porcelain_fields();
        assert_eq!(fields[3].as_ref(), "");
    }

    #[test]
    fn human_output_single_remote_noun() {
        let result = RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                remotes: vec![RemoteInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    name: "origin".to_string(),
                    classification: RemoteClassification::Active,
                    url: Some("u".to_string()),
                    tracking_count: 1,
                    is_origin: true,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("active", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test (1 remote)"));
    }
}
