use std::io::Write;

use crate::types::{Classification, JsonWorktree, ScanResult, WorktreeInfo};

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    for warning in &result.warnings {
        writeln!(out, "warning: {warning}")?;
    }

    for group in &result.repos {
        writeln!(
            out,
            "\n{} ({} worktrees)",
            group.name,
            group.worktrees.len()
        )?;

        for wt in &group.worktrees {
            write_worktree_line(out, wt)?;

            // For partial landings, list unmatched commits
            if let Classification::LandedPartial { unmatched, .. } = &wt.classification {
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

    writeln!(
        out,
        "\n{} worktrees scanned: {} merged, {} landed, {} partial, {} active, {} local",
        result.total_scanned,
        result.counts.merged,
        result.counts.landed,
        result.counts.partial,
        result.counts.active,
        result.counts.local,
    )?;

    Ok(())
}

fn write_worktree_line(out: &mut dyn Write, wt: &WorktreeInfo) -> std::io::Result<()> {
    let label = format!("{:<8}", wt.classification.label());
    let dir_name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let branch = wt.branch.as_deref().unwrap_or("(detached)");

    // Landed ratio
    let ratio = match &wt.classification {
        Classification::Landed { matched, total } => format!("{matched}/{total}"),
        Classification::LandedPartial { matched, total, .. } => format!("{matched}/{total}"),
        _ => String::new(),
    };

    // Ahead/behind
    let ahead_behind = if wt.ahead > 0 || wt.behind > 0 {
        format!("+{}/{}-0", wt.ahead, wt.behind)
    } else {
        String::new()
    };

    // Annotations
    let mut annotations = Vec::new();
    if wt.annotations.dirty {
        annotations.push(format!("dirty ({} files)", wt.annotations.dirty_file_count));
    }
    if wt.annotations.diverged {
        annotations.push("diverged".to_string());
    }
    if wt.annotations.remote_deleted {
        annotations.push("remote deleted".to_string());
    }

    write!(out, "  {label} {dir_name:<32} {branch:<32}")?;
    if !ratio.is_empty() {
        write!(out, " {ratio:<8}")?;
    }
    if !ahead_behind.is_empty() {
        write!(out, " {ahead_behind:<10}")?;
    }
    for ann in &annotations {
        write!(out, "  {ann}")?;
    }
    writeln!(out)?;

    Ok(())
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    let all_worktrees: Vec<JsonWorktree> = result
        .repos
        .iter()
        .flat_map(|g| g.worktrees.iter())
        .map(JsonWorktree::from)
        .collect();

    let json = serde_json::to_string_pretty(&all_worktrees).map_err(std::io::Error::other)?;
    writeln!(out, "{json}")?;
    Ok(())
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for wt in &group.worktrees {
            let path = wt.path.display();
            let parent = wt.parent_repo.display();
            let branch = wt.branch.as_deref().unwrap_or("");
            let class = wt.classification.label();

            let ratio = match &wt.classification {
                Classification::Landed { matched, total } => format!("{matched}/{total}"),
                Classification::LandedPartial { matched, total, .. } => {
                    format!("{matched}/{total}")
                }
                _ => String::new(),
            };

            let dirty_count = wt.annotations.dirty_file_count;

            let mut anns = Vec::new();
            if wt.annotations.remote_deleted {
                anns.push("remote_deleted");
            }
            if wt.annotations.diverged {
                anns.push("diverged");
            }
            if wt.annotations.dirty {
                anns.push("dirty");
            }
            let annotations = anns.join(",");

            writeln!(
                out,
                "{path}\t{parent}\t{branch}\t{class}\t{ratio}\t{}\t{}\t{dirty_count}\t{annotations}",
                wt.ahead, wt.behind
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
                        classification: Classification::Merged,
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

        assert!(output.contains("Backend (2 worktrees)"));
        assert!(output.contains("merged"));
        assert!(output.contains("active"));
        assert!(output.contains("Backend-parallel"));
        assert!(output.contains("Backend-caps"));
        assert!(
            output
                .contains("2 worktrees scanned: 1 merged, 0 landed, 0 partial, 1 active, 0 local")
        );
    }

    #[test]
    fn human_output_with_partial() {
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
        assert!(output.contains("dirty (5 files)"));
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
        assert_eq!(arr[1]["classification"], "active");
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
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[3], "merged");

        let fields2: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields2[3], "active");
    }

    #[test]
    fn human_output_with_warnings() {
        let result = ScanResult {
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
