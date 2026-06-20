use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::ScanResult;

/// Write human-readable scan output.
///
/// Per-group: heading + `format_table` over the group's `WorktreeInfo`s, which
/// reads `TidyItem for WorktreeInfo` defined in `git_tidy_core::output`.
pub fn write_human(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(
            out,
            "\n{} ({} worktrees)",
            group.name,
            group.worktrees.len()
        )?;
        shared::format_table(out, &group.worktrees)?;
    }

    shared::write_summary_line(
        out,
        result.total_scanned,
        &result.counts,
        "worktrees",
        shared::LANDED_SUMMARY,
    )?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        shared::format_porcelain(out, &group.worktrees)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use git_tidy_core::counts::Counts;
    use git_tidy_core::types::*;

    fn make_scan_result() -> ScanResult {
        ScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/Backend"),
                name: "Backend".to_string(),
                worktrees: vec![
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
                    },
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
                    },
                ],
            }],
            total_scanned: 2,
            counts: Counts::from_pairs(&[("landed", 1), ("active", 1)]),
            warnings: vec![],
        }
    }

    #[test]
    fn write_human_smoke() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Backend (2 worktrees)"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("landed"));
        assert!(output.contains("active"));
        assert!(output.contains("Backend-parallel"));
        assert!(output.contains("Backend-caps"));
        assert!(output.contains(
            "2 worktrees scanned: 1 landed, 0 stale, 0 content, 0 partial, 1 active, 0 local"
        ));
        assert!(output.contains("hint: run 'git tidy explain'"));
    }

    #[test]
    fn write_human_with_partial_includes_unmatched_extras() {
        let result = ScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                worktrees: vec![WorktreeInfo {
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
                    dirty_files: vec![],
                    meaningful_dirty_files: vec!["a".into(); 5],
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
        assert!(output.contains("    unmatched: 8d8a06c Add app icon button"));
        assert!(output.contains("    unmatched: b4cd142 Add themed icons"));
        // Annotation separator is now ", " (was "  " before the TidyItem migration).
        assert!(output.contains("dirty (5 files), diverged"));
    }

    #[test]
    fn write_human_with_warnings() {
        let result = ScanResult {
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

    #[test]
    fn write_porcelain_smoke() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[3], "landed");
        let fields2: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields2[3], "active");
    }

    #[test]
    fn write_json_smoke() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["classification"], "landed");
        assert_eq!(arr[1]["classification"], "active");
    }
}
