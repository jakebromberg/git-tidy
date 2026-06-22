use std::io::Write;
use std::ops::Deref;
use std::path::PathBuf;

use git_tidy_core::clean::{Decision, Outcome, run_clean as core_run_clean};
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{CleanResult, FailedItem};

use crate::output::format_bytes;
use crate::types::{LfsClassification, LfsInfo, LfsScanResult};

/// Options controlling LFS cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Enable pruning of orphaned LFS objects.
    pub prune: bool,
}

/// Result of an LFS clean operation: the shared [`CleanResult`] plus the
/// LFS-specific health recommendations surfaced during the pass.
///
/// Derefs to the inner [`CleanResult`] so callers read `succeeded` / `failed` /
/// `skipped` without going through `.result`.
#[derive(Debug)]
pub struct LfsCleanResult {
    /// Shared succeeded / failed / skipped aggregation.
    pub result: CleanResult<PrunedRepo>,
    /// Recommendations printed to the user (e.g. `git lfs migrate`). Already
    /// emitted inline during the pass; retained here for tests and future JSON
    /// output. Computed tool-side around the per-group `run_clean` call, since the
    /// shared pipeline does not model per-group aggregate extras.
    #[allow(dead_code)]
    pub recommendations: Vec<String>,
}

impl Deref for LfsCleanResult {
    type Target = CleanResult<PrunedRepo>;

    fn deref(&self) -> &Self::Target {
        &self.result
    }
}

/// A repo whose LFS objects were successfully pruned (or would have been, in dry-run).
#[derive(Debug)]
#[allow(dead_code)]
pub struct PrunedRepo {
    pub repo: PathBuf,
    pub objects_pruned: usize,
    pub bytes_freed: u64,
    /// True for dry-run entries — nothing was actually freed. Downstream JSON consumers can filter on this to distinguish a preview from a real prune.
    pub dry_run: bool,
}

/// Determine if an LFS item should be acted on by `act`.
///
/// Only orphaned objects are prunable, and only when `--prune` is set. Every
/// other classification (untracked, missing, healthy) is skipped here; the
/// pipeline counts the rejection as a skip, matching the historical
/// `skipped += 1; continue;`. Untracked/missing items still drive per-group
/// recommendations, which are detected tool-side around the per-group call.
fn should_clean(item: &LfsInfo, options: &CleanOptions) -> bool {
    matches!(item.classification, LfsClassification::Orphaned) && options.prune
}

/// Run the clean operation on a scan result.
///
/// lfs groups items by repo. We call the shared [`core_run_clean`] loop once per
/// group over that group's items, accumulating each group's [`CleanResult`] into
/// a single aggregate. The per-group health recommendations are not modeled by the
/// pipeline, so they stay tool-side: we detect `has_untracked` / `has_missing`
/// from the group's classifications and emit the recommendation lines *after* that
/// group's `core_run_clean` call. This keeps a group's prune lines, then its
/// recommendation lines, contiguous before the next group is processed.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &LfsScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<LfsCleanResult, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0usize;
    let mut recommendations = Vec::new();

    for group in &scan_result.repos {
        // The seam owns iterate/filter/act/aggregate for this group's items; the
        // per-group recommendations below are the aggregate extra it doesn't model.
        let result = core_run_clean(
            &group.items,
            |item| {
                if should_clean(item, options) {
                    Decision::Clean
                } else {
                    Decision::Skip
                }
            },
            |item, out| {
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
                    return Ok(Outcome::Cleaned(PrunedRepo {
                        repo: group.repo_path.clone(),
                        objects_pruned: count,
                        bytes_freed: bytes,
                        dry_run: true,
                    }));
                }

                match git.lfs_prune(&group.repo_path) {
                    Ok(()) => {
                        writeln!(
                            out,
                            "pruned {} orphaned LFS objects ({}) in {}",
                            count,
                            format_bytes(bytes),
                            group.name,
                        )?;
                        Ok(Outcome::Cleaned(PrunedRepo {
                            repo: group.repo_path.clone(),
                            objects_pruned: count,
                            bytes_freed: bytes,
                            dry_run: false,
                        }))
                    }
                    Err(e) => {
                        writeln!(
                            out,
                            "error: could not prune LFS objects in {}: {e}",
                            group.name,
                        )?;
                        Ok(Outcome::Failed(FailedItem {
                            repo: group.repo_path.clone(),
                            name: group.name.clone(),
                            reason: e.to_string(),
                        }))
                    }
                }
            },
            out,
        )?;

        succeeded.extend(result.succeeded);
        failed.extend(result.failed);
        skipped += result.skipped;

        // Per-group recommendations: emitted after this group's prune lines so a
        // group's output stays contiguous, then collected for tests/JSON.
        let has_untracked = group
            .items
            .iter()
            .any(|i| i.classification == LfsClassification::Untracked);
        let has_missing = group
            .items
            .iter()
            .any(|i| i.classification == LfsClassification::Missing);

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

    Ok(LfsCleanResult {
        result: CleanResult {
            succeeded,
            failed,
            skipped,
        },
        recommendations,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::counts::Counts;
    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(items: Vec<LfsInfo>) -> LfsScanResult {
        let mut counts = Counts::default();
        for item in &items {
            counts.increment(item.classification.label());
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
            dry_run: false,
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].objects_pruned, 3);
        assert_eq!(result.succeeded[0].bytes_freed, 2_500_000);
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
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.lfs_prune_calls().len(), 0);
        // Regression: dry-run entries must be distinguishable from real prunes so downstream JSON consumers can filter them out.
        assert!(
            result.succeeded[0].dry_run,
            "dry-run PrunedRepo must have dry_run=true",
        );

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would prune"));
    }

    #[test]
    fn clean_real_prune_marks_dry_run_false() {
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
            dry_run: false,
            prune: true,
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();
        assert_eq!(result.succeeded.len(), 1);
        assert!(!result.succeeded[0].dry_run);
        assert_eq!(git.lfs_prune_calls().len(), 1);
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

        assert_eq!(result.succeeded.len(), 0);
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
            dry_run: false,
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
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
            dry_run: false,
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
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

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.recommendations.len(), 0);
    }
}
