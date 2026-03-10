use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::{Classification, ClassificationLabel, ScanResult, WorktreeInfo};

const MIN_DIR_NAME_WIDTH: usize = 20;
const MIN_BRANCH_WIDTH: usize = 20;
const MIN_RATIO_WIDTH: usize = 5;
const MIN_AHEAD_BEHIND_WIDTH: usize = 10;

struct ColumnWidths {
    dir_name: usize,
    branch: usize,
    ratio: usize,
    ahead_behind: usize,
    has_ratio: bool,
    has_ahead_behind: bool,
}

fn compute_column_widths(worktrees: &[WorktreeInfo]) -> ColumnWidths {
    let mut max_dir = 0usize;
    let mut max_branch = 0usize;
    let mut max_ratio = 0usize;
    let mut max_ab = 0usize;
    let mut has_ratio = false;
    let mut has_ahead_behind = false;

    for wt in worktrees {
        let dir_name = wt
            .path
            .file_name()
            .map(|n| n.to_string_lossy().len())
            .unwrap_or(0);
        let branch = wt.branch.as_deref().unwrap_or("(detached)").len();
        let ratio = shared::format_landed_ratio(&wt.classification);
        let ab = shared::format_ahead_behind(wt.ahead, wt.behind);

        max_dir = max_dir.max(dir_name);
        max_branch = max_branch.max(branch);
        if !ratio.is_empty() {
            has_ratio = true;
            max_ratio = max_ratio.max(ratio.len());
        }
        if !ab.is_empty() {
            has_ahead_behind = true;
            max_ab = max_ab.max(ab.len());
        }
    }

    ColumnWidths {
        dir_name: max_dir.max(MIN_DIR_NAME_WIDTH),
        branch: max_branch.max(MIN_BRANCH_WIDTH),
        ratio: max_ratio.max(MIN_RATIO_WIDTH),
        ahead_behind: max_ab.max(MIN_AHEAD_BEHIND_WIDTH),
        has_ratio,
        has_ahead_behind,
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(
            out,
            "\n{} ({} worktrees)",
            group.name,
            group.worktrees.len()
        )?;

        let widths = compute_column_widths(&group.worktrees);

        for wt in &group.worktrees {
            write_worktree_line(out, wt, &widths)?;

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

    shared::write_summary_line(out, result.total_scanned, &result.counts, "worktrees")?;
    shared::write_explain_hint(out)?;

    Ok(())
}

fn write_worktree_line(
    out: &mut dyn Write,
    wt: &WorktreeInfo,
    widths: &ColumnWidths,
) -> std::io::Result<()> {
    let label = format!("{:<8}", wt.classification.label());
    let dir_name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let branch = wt.branch.as_deref().unwrap_or("(detached)");

    let ratio = shared::format_landed_ratio(&wt.classification);
    let ahead_behind = shared::format_ahead_behind(wt.ahead, wt.behind);

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

    let dw = widths.dir_name;
    let bw = widths.branch;
    let mut line = format!("  {label} {dir_name:<dw$} {branch:<bw$}");
    if widths.has_ratio {
        let rw = widths.ratio;
        line.push_str(&format!(" {ratio:<rw$}"));
    }
    if widths.has_ahead_behind {
        let aw = widths.ahead_behind;
        line.push_str(&format!(" {ahead_behind:<aw$}"));
    }
    for ann in &annotations {
        line.push_str(&format!("  {ann}"));
    }
    let trimmed = line.trim_end();
    writeln!(out, "{trimmed}")?;

    Ok(())
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &ScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for wt in &group.worktrees {
            let path = wt.path.display();
            let parent = wt.parent_repo.display();
            let branch = wt.branch.as_deref().unwrap_or("");
            let class = wt.classification.label();
            let ratio = shared::format_landed_ratio(&wt.classification);
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
            counts: ScanCounts {
                landed: 1,
                active: 1,
                ..Default::default()
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
        assert!(output.contains("landed"));
        assert!(output.contains("active"));
        assert!(output.contains("Backend-parallel"));
        assert!(output.contains("Backend-caps"));
        assert!(output.contains(
            "2 worktrees scanned: 1 landed, 0 stale, 0 content, 0 partial, 1 active, 0 local"
        ));
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
        assert_eq!(arr[0]["classification"], "landed");
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
        assert_eq!(fields[3], "landed");

        let fields2: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields2[3], "active");
    }

    /// Collect worktree data lines from human output (lines starting with "  " but not "    unmatched:").
    fn worktree_lines(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|l| l.starts_with("  ") && !l.starts_with("    unmatched:"))
            .collect()
    }

    /// Find the byte offset where the annotation region starts on a worktree line,
    /// or None if the line has no annotations.
    fn annotation_start(line: &str) -> Option<usize> {
        for keyword in &["  dirty", "  diverged", "  remote deleted"] {
            if let Some(pos) = line.find(keyword) {
                return Some(pos);
            }
        }
        None
    }

    #[test]
    fn human_output_columns_align() {
        // Three worktrees with varying name lengths and a mix of ratio/ahead_behind.
        let result = ScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/MyApp"),
                name: "MyApp".to_string(),
                worktrees: vec![
                    WorktreeInfo {
                        path: PathBuf::from("/dev/short"),
                        parent_repo: PathBuf::from("/repos/MyApp"),
                        branch: Some("fix/a".to_string()),
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
                        path: PathBuf::from(
                            "/dev/a-very-long-worktree-directory-name-that-exceeds-thirty-two",
                        ),
                        parent_repo: PathBuf::from("/repos/MyApp"),
                        branch: Some(
                            "feature/also-a-very-long-branch-name-exceeding-thirty-two-chars"
                                .to_string(),
                        ),
                        default_branch: "main".to_string(),
                        classification: Classification::Active,
                        annotations: Annotations {
                            dirty: true,
                            dirty_file_count: 3,
                            ..Default::default()
                        },
                        remote_tracking: true,
                        ahead: 5,
                        behind: 12,
                        dirty_files: vec![],
                        meaningful_dirty_files: vec!["x".into(); 3],
                    },
                    WorktreeInfo {
                        path: PathBuf::from("/dev/medium-length-name"),
                        parent_repo: PathBuf::from("/repos/MyApp"),
                        branch: Some("feature/partial-work".to_string()),
                        default_branch: "main".to_string(),
                        classification: Classification::LandedPartial {
                            matched: 3,
                            total: 5,
                            unmatched: vec![],
                        },
                        annotations: Annotations {
                            dirty: true,
                            dirty_file_count: 1,
                            diverged: true,
                            ..Default::default()
                        },
                        remote_tracking: true,
                        ahead: 2,
                        behind: 100,
                        dirty_files: vec![],
                        meaningful_dirty_files: vec!["y".into()],
                    },
                ],
            }],
            total_scanned: 3,
            counts: ScanCounts {
                landed: 1,
                partial: 1,
                active: 1,
                ..Default::default()
            },
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines = worktree_lines(&output);
        assert_eq!(lines.len(), 3, "expected 3 worktree lines, got: {lines:?}");

        // All annotated lines must have their annotations start at the same byte offset.
        let starts: Vec<usize> = lines.iter().filter_map(|l| annotation_start(l)).collect();
        assert!(
            starts.len() >= 2,
            "expected at least 2 annotated lines, got {}: {starts:?}",
            starts.len()
        );
        assert!(
            starts.windows(2).all(|w| w[0] == w[1]),
            "annotation starts differ: {starts:?}\nlines:\n{}",
            lines.join("\n")
        );

        // Lines without annotations should not have trailing whitespace.
        for line in &lines {
            if annotation_start(line).is_none() {
                assert_eq!(
                    *line,
                    line.trim_end(),
                    "non-annotated line has trailing whitespace: {line:?}"
                );
            }
        }
    }

    #[test]
    fn human_output_omits_unused_columns() {
        // Landed-only group: no ratio, no ahead/behind.
        let result = ScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/Lib"),
                name: "Lib".to_string(),
                worktrees: vec![
                    WorktreeInfo {
                        path: PathBuf::from("/dev/Lib-old"),
                        parent_repo: PathBuf::from("/repos/Lib"),
                        branch: Some("cleanup/old".to_string()),
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
                        path: PathBuf::from("/dev/Lib-stale"),
                        parent_repo: PathBuf::from("/repos/Lib"),
                        branch: Some("fix/stale".to_string()),
                        default_branch: "main".to_string(),
                        classification: Classification::Landed,
                        annotations: Annotations::default(),
                        remote_tracking: true,
                        ahead: 0,
                        behind: 0,
                        dirty_files: vec![],
                        meaningful_dirty_files: vec![],
                    },
                ],
            }],
            total_scanned: 2,
            counts: ScanCounts {
                landed: 2,
                ..Default::default()
            },
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // No line should have trailing whitespace from empty ratio/ahead_behind columns.
        for line in worktree_lines(&output) {
            assert_eq!(
                line,
                line.trim_end(),
                "worktree line has trailing whitespace: {line:?}"
            );
        }
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
