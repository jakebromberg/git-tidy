use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::{Classification, ClassificationLabel};

use crate::types::{BranchInfo, BranchScanResult};

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(out, "\n{} ({} branches)", group.name, group.branches.len())?;

        for branch in &group.branches {
            write_branch_line(out, branch)?;

            // For partial landings, list unmatched commits
            if let Classification::LandedPartial { unmatched, .. } = &branch.classification {
                for commit in unmatched {
                    writeln!(
                        out,
                        "    unmatched: {} {}",
                        commit.short_hash, commit.subject
                    )?;
                }
            }
        }
    }

    shared::write_summary_line(out, result.total_scanned, &result.counts, "branches")?;

    Ok(())
}

fn write_branch_line(out: &mut dyn Write, branch: &BranchInfo) -> std::io::Result<()> {
    let label = format!("{:<8}", branch.classification.label());

    let name = if branch.is_current {
        format!("* {}", branch.name)
    } else {
        format!("  {}", branch.name)
    };

    let ratio = shared::format_landed_ratio(&branch.classification);
    let ahead_behind = shared::format_ahead_behind(branch.ahead, branch.behind);

    // Annotations
    let mut ann_strs = Vec::new();
    if branch.diverged {
        ann_strs.push("diverged");
    }
    if branch.remote_deleted {
        ann_strs.push("remote deleted");
    }
    let annotations = shared::format_annotations(&ann_strs);

    write!(out, "  {label} {name:<34}")?;
    if !ratio.is_empty() {
        write!(out, " {ratio:<8}")?;
    }
    if !ahead_behind.is_empty() {
        write!(out, " {ahead_behind:<10}")?;
    }
    if !annotations.is_empty() {
        write!(out, "  {annotations}")?;
    }
    writeln!(out)?;

    Ok(())
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &BranchScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for branch in &group.branches {
            let repo = branch.repo_path.display();
            let name = &branch.name;
            let class = branch.classification.label();
            let ratio = shared::format_landed_ratio(&branch.classification);

            let mut anns = Vec::new();
            if branch.remote_deleted {
                anns.push("remote_deleted");
            }
            if branch.diverged {
                anns.push("diverged");
            }
            let annotations = anns.join(",");

            writeln!(
                out,
                "{repo}\t{name}\t{class}\t{ratio}\t{}\t{}\t{}\t{annotations}",
                branch.ahead, branch.behind, branch.is_current,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::types::*;

    fn make_scan_result() -> BranchScanResult {
        BranchScanResult {
            repos: vec![BranchRepoGroup {
                repo_path: PathBuf::from("/repos/Backend"),
                name: "Backend".to_string(),
                branches: vec![
                    BranchInfo {
                        repo_path: PathBuf::from("/repos/Backend"),
                        name: "fix/skip-db-init".to_string(),
                        default_branch: "main".to_string(),
                        classification: Classification::Merged,
                        remote_tracking: true,
                        remote_deleted: true,
                        ahead: 0,
                        behind: 0,
                        diverged: false,
                        is_current: false,
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
                    },
                ],
            }],
            total_scanned: 2,
            counts: ScanCounts {
                merged: 1,
                landed: 0,
                partial: 0,
                active: 1,
                local: 0,
            },
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("Backend (2 branches)"));
        assert!(output.contains("merged"));
        assert!(output.contains("active"));
        assert!(output.contains("fix/skip-db-init"));
        assert!(output.contains("* feature/caps"));
        assert!(output.contains("remote deleted"));
        assert!(
            output.contains("2 branches scanned: 1 merged, 0 landed, 0 partial, 1 active, 0 local")
        );
    }

    #[test]
    fn human_output_with_partial() {
        let result = BranchScanResult {
            repos: vec![BranchRepoGroup {
                repo_path: PathBuf::from("/repos/App"),
                name: "App".to_string(),
                branches: vec![BranchInfo {
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
                }],
            }],
            total_scanned: 1,
            counts: ScanCounts {
                partial: 1,
                ..Default::default()
            },
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
        assert_eq!(arr[0]["classification"], "merged");
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
        assert_eq!(fields[2], "merged");
        assert_eq!(fields[7], "remote_deleted");

        let fields2: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields2[2], "active");
        assert_eq!(fields2[6], "true");
    }

    #[test]
    fn human_output_with_warnings() {
        let result = BranchScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: ScanCounts::default(),
            warnings: vec!["could not determine default branch for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not determine default branch"));
    }
}
