use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};
use git_tidy_core::types::{Classification, ClassificationLabel};

use crate::types::{BranchInfo, BranchScanResult};

impl TidyItem for BranchInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("BRANCH"),
        ColumnSpec::left("RATIO"),
        ColumnSpec::left("AHEAD/BEHIND"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        let status: Cell = Cow::Borrowed(self.classification.label());
        // Bake the current-branch marker into the BRANCH cell so both starred
        // and unstarred rows occupy the same column width.
        let branch: Cell = if self.is_current {
            Cow::Owned(format!("* {}", self.name))
        } else {
            Cow::Owned(format!("  {}", self.name))
        };

        let ratio = shared::format_landed_ratio(&self.classification);
        let ratio_cell: Option<Cell> = if ratio.is_empty() {
            None
        } else {
            Some(Cow::Owned(ratio))
        };

        let ab = shared::format_ahead_behind(self.ahead, self.behind);
        let ab_cell: Option<Cell> = if ab.is_empty() {
            None
        } else {
            Some(Cow::Owned(ab))
        };

        vec![Some(status), Some(branch), ratio_cell, ab_cell]
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
        if self.remote_only {
            anns.push(Cow::Borrowed("remote"));
        }
        if self.diverged {
            anns.push(Cow::Borrowed("diverged"));
        }
        if self.remote_deleted {
            anns.push(Cow::Borrowed("remote deleted"));
        }
        anns
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        let mut porcelain_anns: Vec<&str> = Vec::new();
        if self.remote_only {
            porcelain_anns.push("remote_only");
        }
        if self.remote_deleted {
            porcelain_anns.push("remote_deleted");
        }
        if self.diverged {
            porcelain_anns.push("diverged");
        }

        vec![
            Cow::Owned(self.repo_path.display().to_string()),
            Cow::Owned(self.name.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(shared::format_landed_ratio(&self.classification)),
            Cow::Owned(self.ahead.to_string()),
            Cow::Owned(self.behind.to_string()),
            Cow::Owned(self.is_current.to_string()),
            Cow::Owned(porcelain_anns.join(",")),
        ]
    }
}

/// Write human-readable scan output.
///
/// Per-group: heading + `format_table` over the group's `BranchInfo`s, which
/// reads `TidyItem for BranchInfo` defined above.
pub fn write_human(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(out, "\n{} ({} branches)", group.name, group.items.len())?;
        shared::format_table(out, &group.items)?;
    }

    shared::write_summary_line(
        out,
        result.total_scanned,
        &result.counts,
        "branches",
        shared::LANDED_SUMMARY,
    )?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
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
    use git_tidy_core::types::*;

    fn make_scan_result() -> BranchScanResult {
        BranchScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/Backend"),
                name: "Backend".to_string(),
                items: vec![
                    BranchInfo {
                        repo_path: PathBuf::from("/repos/Backend"),
                        name: "fix/skip-db-init".to_string(),
                        default_branch: "main".to_string(),
                        classification: Classification::Landed,
                        remote_tracking: true,
                        remote_deleted: true,
                        ahead: 0,
                        behind: 0,
                        diverged: false,
                        is_current: false,
                        remote_only: false,
                    },
                    BranchInfo {
                        repo_path: PathBuf::from("/repos/Backend"),
                        name: "feature/caps".to_string(),
                        default_branch: "main".to_string(),
                        classification: Classification::Active,
                        remote_tracking: true,
                        remote_deleted: false,
                        ahead: 3,
                        behind: 0,
                        diverged: false,
                        is_current: true,
                        remote_only: false,
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("landed", 1), ("active", 1)]),
            warnings: vec![],
        }
    }

    fn make_branch(name: &str) -> BranchInfo {
        BranchInfo {
            repo_path: PathBuf::from("/repos/Demo"),
            name: name.to_string(),
            default_branch: "main".to_string(),
            classification: Classification::Active,
            remote_tracking: true,
            remote_deleted: false,
            ahead: 0,
            behind: 0,
            diverged: false,
            is_current: false,
            remote_only: false,
        }
    }

    // --- Smoke tests on the public formatters (preserved across migration) ---

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("Backend (2 branches)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("BRANCH"));
        assert!(output.contains("landed"));
        assert!(output.contains("active"));
        assert!(output.contains("fix/skip-db-init"));
        assert!(output.contains("* feature/caps"));
        assert!(output.contains("remote deleted"));
        assert!(output.contains(
            "2 branches scanned: 1 landed, 0 stale, 0 content, 0 partial, 1 active, 0 local"
        ));
    }

    #[test]
    fn human_output_with_partial() {
        let result = BranchScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                items: vec![BranchInfo {
                    repo_path: PathBuf::from("/repos/App"),
                    name: "alternate-icons".to_string(),
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
                    remote_tracking: true,
                    remote_deleted: false,
                    ahead: 6,
                    behind: 324,
                    diverged: true,
                    is_current: false,
                    remote_only: false,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("partial", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("partial"));
        assert!(output.contains("4/6"));
        assert!(output.contains("unmatched: 8d8a06c Add app icon button"));
        assert!(output.contains("unmatched: b4cd142 Add themed icons"));
        assert!(output.contains("diverged"));
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
        assert_eq!(arr[0]["classification"], "landed");
        assert_eq!(arr[0]["remote_deleted"], true);
        assert_eq!(arr[1]["classification"], "active");
        assert_eq!(arr[1]["is_current"], true);
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
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[0], "/repos/Backend");
        assert_eq!(fields[1], "fix/skip-db-init");
        assert_eq!(fields[2], "landed");
        assert_eq!(fields[7], "remote_deleted");

        let fields2: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields2[2], "active");
        assert_eq!(fields2[6], "true");
    }

    #[test]
    fn human_output_remote_only_annotation() {
        let result = BranchScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                items: vec![BranchInfo {
                    repo_path: PathBuf::from("/repos/App"),
                    name: "feature/stale".to_string(),
                    default_branch: "main".to_string(),
                    classification: Classification::Landed,
                    remote_tracking: true,
                    remote_deleted: false,
                    ahead: 0,
                    behind: 0,
                    diverged: false,
                    is_current: false,
                    remote_only: true,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("landed", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("remote"));
        assert!(output.contains("feature/stale"));
    }

    #[test]
    fn porcelain_output_remote_only_annotation() {
        let result = BranchScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                items: vec![BranchInfo {
                    repo_path: PathBuf::from("/repos/App"),
                    name: "feature/stale-remote".to_string(),
                    default_branch: "main".to_string(),
                    classification: Classification::Landed,
                    remote_tracking: true,
                    remote_deleted: false,
                    ahead: 0,
                    behind: 0,
                    diverged: false,
                    is_current: false,
                    remote_only: true,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("landed", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let fields: Vec<&str> = output.lines().next().unwrap().split('\t').collect();
        assert_eq!(fields[7], "remote_only");
    }

    #[test]
    fn json_output_includes_remote_only_field() {
        let result = BranchScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                items: vec![BranchInfo {
                    repo_path: PathBuf::from("/repos/App"),
                    name: "feature/stale-remote".to_string(),
                    default_branch: "main".to_string(),
                    classification: Classification::Landed,
                    remote_tracking: true,
                    remote_deleted: false,
                    ahead: 0,
                    behind: 0,
                    diverged: false,
                    is_current: false,
                    remote_only: true,
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("landed", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr[0]["remote_only"], true);
    }

    #[test]
    fn human_output_with_warnings() {
        let result = BranchScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["could not determine default branch for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not determine default branch"));
    }

    // --- TidyItem data-shape tests on BranchInfo ---

    #[test]
    fn row_has_one_cell_per_column() {
        let b = make_branch("feature/x");
        assert_eq!(b.row().len(), BranchInfo::COLUMNS.len());
    }

    #[test]
    fn row_status_and_branch_always_present() {
        let b = make_branch("feature/x");
        let row = b.row();
        assert!(row[0].is_some(), "STATUS must be present");
        assert!(row[1].is_some(), "BRANCH must be present");
    }

    #[test]
    fn row_ratio_and_ab_hidden_when_empty() {
        let mut b = make_branch("feature/x");
        b.ahead = 0;
        b.behind = 0;
        let row = b.row();
        assert!(row[2].is_none(), "RATIO must hide when no ratio applies");
        assert!(row[3].is_none(), "AHEAD/BEHIND must hide when 0/0");
    }

    #[test]
    fn row_ratio_present_for_landed_partial() {
        let mut b = make_branch("feature/x");
        b.classification = Classification::LandedPartial {
            matched: 4,
            total: 6,
            unmatched: vec![],
        };
        let row = b.row();
        assert_eq!(row[2].as_deref(), Some("4/6"));
    }

    #[test]
    fn row_ab_present_when_ahead_or_behind_nonzero() {
        let mut b = make_branch("feature/x");
        b.ahead = 3;
        b.behind = 7;
        let row = b.row();
        assert!(row[3].is_some());
    }

    #[test]
    fn row_branch_cell_carries_current_marker() {
        let mut b = make_branch("feature/x");
        b.is_current = true;
        let row = b.row();
        let cell = row[1].as_deref().unwrap();
        assert!(cell.starts_with("* "), "got: {cell:?}");
        assert!(cell.ends_with("feature/x"));
    }

    #[test]
    fn row_branch_cell_pads_unstarred_for_alignment() {
        let mut b = make_branch("feature/x");
        b.is_current = false;
        let row = b.row();
        let cell = row[1].as_deref().unwrap();
        assert!(cell.starts_with("  "), "got: {cell:?}");
        assert!(cell.ends_with("feature/x"));
    }

    #[test]
    fn row_extras_empty_for_non_partial() {
        let b = make_branch("feature/x");
        assert!(b.row_extras().is_empty());
    }

    #[test]
    fn row_extras_lists_unmatched_for_landed_partial() {
        let mut b = make_branch("feature/x");
        b.classification = Classification::LandedPartial {
            matched: 1,
            total: 2,
            unmatched: vec![UnmatchedCommit {
                short_hash: "abc123".to_string(),
                subject: "Drop tracing".to_string(),
            }],
        };
        let extras = b.row_extras();
        assert_eq!(extras.len(), 1);
        assert_eq!(extras[0].as_ref(), "unmatched: abc123 Drop tracing");
    }

    #[test]
    fn annotations_empty_when_no_flags() {
        let b = make_branch("feature/x");
        assert!(b.annotations().is_empty());
    }

    #[test]
    fn annotations_human_order_remote_diverged_remote_deleted() {
        let mut b = make_branch("feature/x");
        b.remote_only = true;
        b.diverged = true;
        b.remote_deleted = true;
        let tokens: Vec<String> = b
            .annotations()
            .into_iter()
            .map(|c| c.into_owned())
            .collect();
        assert_eq!(tokens, vec!["remote", "diverged", "remote deleted"]);
    }

    #[test]
    fn porcelain_fields_count_is_eight() {
        let b = make_branch("feature/x");
        assert_eq!(b.porcelain_fields().len(), 8);
    }

    #[test]
    fn porcelain_fields_path_first() {
        let b = make_branch("feature/x");
        let fields = b.porcelain_fields();
        assert_eq!(fields[0].as_ref(), "/repos/Demo");
    }

    #[test]
    fn porcelain_fields_order_and_types() {
        let mut b = make_branch("feature/x");
        b.classification = Classification::Active;
        b.ahead = 3;
        b.behind = 7;
        b.is_current = true;
        let fields = b.porcelain_fields();
        assert_eq!(fields[1].as_ref(), "feature/x");
        assert_eq!(fields[2].as_ref(), "active");
        assert_eq!(fields[3].as_ref(), ""); // ratio empty for Active
        assert_eq!(fields[4].as_ref(), "3");
        assert_eq!(fields[5].as_ref(), "7");
        assert_eq!(fields[6].as_ref(), "true");
        assert_eq!(fields[7].as_ref(), "");
    }

    #[test]
    fn porcelain_annotations_order_remote_only_remote_deleted_diverged() {
        let mut b = make_branch("feature/x");
        b.remote_only = true;
        b.remote_deleted = true;
        b.diverged = true;
        let fields = b.porcelain_fields();
        assert_eq!(fields[7].as_ref(), "remote_only,remote_deleted,diverged");
    }
}
