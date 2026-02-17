use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;

use crate::output::format_bytes;
use crate::types::{LfsClassification, LfsScanResult};

/// Options controlling LFS cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Skip confirmation prompts.
    pub yes: bool,
    /// Enable pruning of orphaned LFS objects.
    pub prune: bool,
}

/// Result of a clean operation.
#[derive(Debug)]
pub struct CleanResult {
    /// Repos that were pruned (or would be in dry-run).
    pub pruned: Vec<PrunedRepo>,
    /// Repos that failed to prune.
    pub failed: Vec<FailedRepo>,
    /// Items that were skipped (not actionable).
    pub skipped: usize,
    /// Recommendations printed to the user.
    pub recommendations: Vec<String>,
}

/// A repo whose LFS objects were successfully pruned.
#[derive(Debug)]
pub struct PrunedRepo {
    pub repo: PathBuf,
    pub objects_pruned: usize,
    pub bytes_freed: u64,
}

/// A repo whose LFS prune failed.
#[derive(Debug)]
pub struct FailedRepo {
    pub repo: PathBuf,
    pub reason: String,
}

/// Run the clean operation on a scan result.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &LfsScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult, Error> {
    let mut pruned = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;
    let mut recommendations = Vec::new();

    for group in &scan_result.repos {
        let mut has_untracked = false;
        let mut has_missing = false;

        for item in &group.items {
            match item.classification {
                LfsClassification::Orphaned if options.prune => {
                    let count = item
                        .path
                        .trim_start_matches('<')
                        .split_once(' ')
                        .and_then(|(n, _)| n.parse::<usize>().ok())
                        .unwrap_or(0);
                    let bytes = item.size_bytes.unwrap_or(0);

                    if options.dry_run {
                        writeln!(
                            out,
                            "would prune {} orphaned LFS objects ({}) in {}",
                            count,
                            format_bytes(bytes),
                            group.name,
                        )?;
                        pruned.push(PrunedRepo {
                            repo: group.repo_path.clone(),
                            objects_pruned: count,
                            bytes_freed: bytes,
                        });
                    } else {
                        match git.lfs_prune(&group.repo_path) {
                            Ok(()) => {
                                writeln!(
                                    out,
                                    "pruned {} orphaned LFS objects ({}) in {}",
                                    count,
                                    format_bytes(bytes),
                                    group.name,
                                )?;
                                pruned.push(PrunedRepo {
                                    repo: group.repo_path.clone(),
                                    objects_pruned: count,
                                    bytes_freed: bytes,
                                });
                            }
                            Err(e) => {
                                writeln!(
                                    out,
                                    "error: could not prune LFS objects in {}: {e}",
                                    group.name,
                                )?;
                                failed.push(FailedRepo {
                                    repo: group.repo_path.clone(),
                                    reason: e.to_string(),
                                });
                            }
                        }
                    }
                }
                LfsClassification::Untracked => {
                    has_untracked = true;
                    skipped += 1;
                }
                LfsClassification::Missing => {
                    has_missing = true;
                    skipped += 1;
                }
                _ => {
                    skipped += 1;
                }
            }
        }

        if has_untracked {
            let msg = format!(
                "{}: large files not tracked by LFS -- consider `git lfs migrate` or `git-filter-repo`",
                group.name,
            );
            writeln!(out, "recommendation: {msg}")?;
            recommendations.push(msg);
        }
        if has_missing {
            let msg = format!(
                "{}: missing LFS objects -- run `git lfs fetch --all`",
                group.name,
            );
            writeln!(out, "recommendation: {msg}")?;
            recommendations.push(msg);
        }
    }

    Ok(CleanResult {
        pruned,
        failed,
        skipped,
        recommendations,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(items: Vec<LfsInfo>) -> LfsScanResult {
        let mut counts = LfsCounts::default();
        for item in &items {
            counts.increment(item.classification);
        }
        LfsScanResult {
            repos: vec![LfsRepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                items,
                lfs_available: true,
                track_patterns: vec![],
            }],
            total_scanned: 0,
            counts,
            warnings: vec![],
            lfs_installed: true,
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            yes: false,
            prune: false,
        }
    }

    #[test]
    fn clean_prune_orphaned() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "<3 orphaned LFS objects>".to_string(),
            classification: LfsClassification::Orphaned,
            oid: String::new(),
            size_bytes: Some(2_500_000),
        }]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            prune: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 1);
        assert_eq!(result.pruned[0].objects_pruned, 3);
        assert_eq!(result.pruned[0].bytes_freed, 2_500_000);
        assert_eq!(git.lfs_prune_calls().len(), 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("pruned 3 orphaned LFS objects"));
    }

    #[test]
    fn clean_dry_run_does_not_prune() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "<2 orphaned LFS objects>".to_string(),
            classification: LfsClassification::Orphaned,
            oid: String::new(),
            size_bytes: Some(1_000_000),
        }]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            prune: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 1);
        assert_eq!(git.lfs_prune_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would prune"));
    }

    #[test]
    fn clean_without_prune_flag_skips_orphaned() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "<2 orphaned LFS objects>".to_string(),
            classification: LfsClassification::Orphaned,
            oid: String::new(),
            size_bytes: Some(1_000_000),
        }]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.lfs_prune_calls().len(), 0);
    }

    #[test]
    fn clean_with_no_orphaned_is_noop() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "video.mp4".to_string(),
            classification: LfsClassification::Untracked,
            oid: "hash1".to_string(),
            size_bytes: Some(5_000_000),
        }]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            prune: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.lfs_prune_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("recommendation:"));
        assert!(output.contains("git lfs migrate"));
    }

    #[test]
    fn clean_handles_prune_failure() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_prune_error(&repo(), "permission denied")
            .build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "<1 orphaned LFS objects>".to_string(),
            classification: LfsClassification::Orphaned,
            oid: String::new(),
            size_bytes: Some(500_000),
        }]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            prune: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 0);
        assert_eq!(result.failed.len(), 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not prune LFS objects"));
    }

    #[test]
    fn clean_prints_missing_recommendation() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "missing.bin".to_string(),
            classification: LfsClassification::Missing,
            oid: "oid1".to_string(),
            size_bytes: None,
        }]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.recommendations.len(), 1);
        assert!(result.recommendations[0].contains("git lfs fetch --all"));
    }

    #[test]
    fn clean_healthy_items_skipped() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();
        let scan = make_scan_result(vec![LfsInfo {
            repo_path: repo(),
            path: "good.bin".to_string(),
            classification: LfsClassification::Healthy,
            oid: "oid1".to_string(),
            size_bytes: None,
        }]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.pruned.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.recommendations.len(), 0);
    }
}
