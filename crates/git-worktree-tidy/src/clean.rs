use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{Classification, CleanResult, FailedItem, ScanResult, WorktreeInfo};

/// Options controlling worktree cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Remove worktrees with meaningful uncommitted changes.
    pub force: bool,
    /// Only target structurally-proven landed worktrees.
    pub strict: bool,
    /// Include active and local worktrees in the clean flow.
    pub all: bool,
    /// Delete local branches after removing their worktrees.
    pub delete_branches: bool,
}

/// A worktree that was successfully removed (or would be in dry-run).
#[derive(Debug)]
#[allow(dead_code)]
pub struct RemovedWorktree {
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub branch_deleted: bool,
}

/// Run the clean operation on a scan result.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &ScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<RemovedWorktree>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    for group in &scan_result.repos {
        for wt in &group.worktrees {
            // Filter by classification
            if !should_clean(&wt.classification, options) {
                skipped += 1;
                continue;
            }

            let dir_name = worktree_display_name(wt);

            // Check if dirty and not forced
            if wt.annotations.dirty && !options.force {
                // LandedStale dirty is always a stale-index artifact — safe to clean
                if matches!(wt.classification, Classification::LandedStale) {
                    // Fall through to removal
                } else {
                    // Check if working tree matches default branch
                    let diff_ref = format!("origin/{}", wt.default_branch);
                    let diff_files = git
                        .diff_working_tree_files(&wt.path, &diff_ref)
                        .or_else(|_| git.diff_working_tree_files(&wt.path, &wt.default_branch));

                    match diff_files {
                        Ok(files) if files.is_empty() => {
                            // Working tree matches main — dirty is informational
                        }
                        _ => {
                            writeln!(
                                out,
                                "skipped {dir_name}: dirty ({} files), use --force to remove",
                                wt.annotations.dirty_file_count
                            )?;
                            skipped += 1;
                            continue;
                        }
                    }
                }
            }

            if options.dry_run {
                write!(out, "would remove {dir_name}")?;
                if options.delete_branches
                    && let Some(branch) = &wt.branch
                {
                    write!(out, " (and branch {branch})")?;
                }
                writeln!(out, " in {}", group.name)?;
                succeeded.push(RemovedWorktree {
                    repo: wt.parent_repo.clone(),
                    path: wt.path.clone(),
                    branch: wt.branch.clone(),
                    branch_deleted: false,
                });
                continue;
            }

            // Three-tier removal strategy
            match remove_worktree(git, wt, options, out) {
                Ok(branch_deleted) => {
                    succeeded.push(RemovedWorktree {
                        repo: wt.parent_repo.clone(),
                        path: wt.path.clone(),
                        branch: wt.branch.clone(),
                        branch_deleted,
                    });
                }
                Err(e) => {
                    writeln!(out, "error: could not remove {dir_name}: {e}")?;
                    failed.push(FailedItem {
                        repo: wt.parent_repo.clone(),
                        name: dir_name,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    Ok(CleanResult {
        succeeded,
        failed,
        skipped,
    })
}

/// Determine if a worktree should be cleaned based on its classification and options.
fn should_clean(classification: &Classification, options: &CleanOptions) -> bool {
    if options.all {
        return true;
    }

    if options.strict {
        return matches!(classification, Classification::Landed);
    }

    // Default: landed (structural) + landed-stale + landed-by-content
    matches!(
        classification,
        Classification::Landed
            | Classification::LandedStale
            | Classification::LandedByContent { .. }
    )
}

/// Extract the directory basename for display.
fn worktree_display_name(wt: &WorktreeInfo) -> String {
    wt.path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| wt.path.display().to_string())
}

/// Attempt to remove a worktree using the three-tier strategy:
/// 1. `git worktree remove <path>`
/// 2. `git worktree remove --force <path>` (if dirty)
/// 3. `rm -rf <path>` + `git worktree prune` (fallback)
///
/// Returns `Ok(branch_deleted)` on success, `Err` on failure.
fn remove_worktree(
    git: &dyn GitOps,
    wt: &WorktreeInfo,
    opts: &CleanOptions,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let dir_name = worktree_display_name(wt);

    // Tier 1 (clean) or Tier 2 (dirty with --force)
    let result = if wt.annotations.dirty {
        git.worktree_remove_force(&wt.parent_repo, &wt.path)
    } else {
        git.worktree_remove(&wt.parent_repo, &wt.path)
    };

    match result {
        Ok(()) => {
            writeln!(out, "removed {dir_name}")?;
        }
        Err(_) => {
            // Tier 3: rm -rf + prune
            if wt.path.exists() {
                std::fs::remove_dir_all(&wt.path).map_err(|e| Error::RemovalFailed {
                    path: wt.path.clone(),
                    reason: e.to_string(),
                })?;
            }
            git.worktree_prune(&wt.parent_repo)?;
            writeln!(out, "removed {dir_name} (fallback)")?;
        }
    }

    // Delete branch if requested (skip for LandedStale — the branch ref is already gone)
    let mut branch_deleted = false;
    if opts.delete_branches
        && !matches!(wt.classification, Classification::LandedStale)
        && let Some(branch) = &wt.branch
    {
        // Fail closed: if we cannot determine whether the branch is checked out elsewhere, never delete it.
        match git.is_branch_checked_out(&wt.parent_repo, branch) {
            Ok(true) => {
                writeln!(
                    out,
                    "skipped branch delete for {branch}: checked out elsewhere"
                )?;
            }
            Ok(false) => match git.branch_delete(&wt.parent_repo, branch) {
                Ok(()) => {
                    writeln!(out, "deleted branch {branch}")?;
                    branch_deleted = true;
                }
                Err(e) => {
                    writeln!(out, "warning: could not delete branch {branch}: {e}")?;
                }
            },
            Err(e) => {
                writeln!(
                    out,
                    "warning: skipped branch delete for {branch}: could not check worktree usage: {e}"
                )?;
            }
        }
    }

    Ok(branch_deleted)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;
    use git_tidy_core::types::{Annotations, RepoGroup, ScanCounts, WorktreeInfo};

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repos/test")
    }

    fn make_worktree(name: &str, branch: &str, classification: Classification) -> WorktreeInfo {
        WorktreeInfo {
            path: PathBuf::from(format!("/dev/{name}")),
            parent_repo: repo(),
            branch: Some(branch.to_string()),
            default_branch: "main".to_string(),
            classification,
            annotations: Annotations::default(),
            remote_tracking: true,
            ahead: 0,
            behind: 0,
            dirty_files: vec![],
            meaningful_dirty_files: vec![],
        }
    }

    fn make_scan(worktrees: Vec<WorktreeInfo>) -> ScanResult {
        let mut counts = ScanCounts::default();
        for wt in &worktrees {
            counts.increment(&wt.classification);
        }
        let total = worktrees.len();
        ScanResult {
            repos: vec![RepoGroup {
                repo_path: repo(),
                name: "test".to_string(),
                worktrees,
            }],
            total_scanned: total,
            counts,
            warnings: vec![],
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            force: false,
            strict: false,
            all: false,
            delete_branches: false,
        }
    }

    #[test]
    fn clean_removes_merged_worktrees() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-done", "feature/done", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].path, PathBuf::from("/dev/wt-done"));
        assert_eq!(git.remove_calls().len(), 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed wt-done"));
    }

    #[test]
    fn clean_removes_landed_by_content_worktrees() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree(
            "wt-landed",
            "feature/landed",
            Classification::LandedByContent {
                matched: 3,
                total: 3,
            },
        );
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
    }

    #[test]
    fn clean_skips_active_by_default() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-active", "feature/wip", Classification::Active);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.remove_calls().len(), 0);
    }

    #[test]
    fn clean_skips_local_by_default() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-local", "feature/local", Classification::Local);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_all_includes_active_and_local() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan(vec![
            make_worktree("wt-active", "feature/wip", Classification::Active),
            make_worktree("wt-local", "feature/local", Classification::Local),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.remove_calls().len(), 2);
    }

    #[test]
    fn clean_strict_skips_landed_by_content() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan(vec![
            make_worktree("wt-landed", "fix/done", Classification::Landed),
            make_worktree(
                "wt-content",
                "fix/content",
                Classification::LandedByContent {
                    matched: 3,
                    total: 3,
                },
            ),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            strict: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].path, PathBuf::from("/dev/wt-landed"));
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_dry_run_makes_zero_remove_calls() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan(vec![
            make_worktree("wt-a", "feature/a", Classification::Landed),
            make_worktree("wt-b", "feature/b", Classification::Landed),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.remove_calls().len(), 0);
        assert_eq!(git.remove_force_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would remove wt-a"));
        assert!(output.contains("would remove wt-b"));
    }

    #[test]
    fn clean_dirty_blocked_without_force() {
        let wt_path = PathBuf::from("/dev/wt-dirty");
        let git = MockGitBuilder::new()
            .with_diff_working_tree_files(&wt_path, "origin/main", vec!["changed.rs".to_string()])
            .build();
        let mut wt = make_worktree("wt-dirty", "fix/dirty", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 3;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.remove_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipped wt-dirty: dirty (3 files)"));
        assert!(output.contains("--force"));
    }

    #[test]
    fn clean_force_removes_dirty() {
        let git = MockGitBuilder::new().build();
        let mut wt = make_worktree("wt-dirty", "fix/dirty", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 2;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        // Dirty worktrees use worktree_remove_force
        assert_eq!(git.remove_force_calls().len(), 1);
        assert_eq!(git.remove_calls().len(), 0);
    }

    #[test]
    fn clean_fallback_on_remove_failure() {
        let git = MockGitBuilder::new()
            .with_worktree_remove_error(&PathBuf::from("/dev/wt-stubborn"), "lock held")
            .build();
        // The worktree path doesn't actually exist on disk, so rm -rf is a no-op,
        // but worktree_prune should be called.
        let wt = make_worktree("wt-stubborn", "feature/stuck", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed wt-stubborn (fallback)"));
    }

    #[test]
    fn clean_delete_branches_after_removal() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-done", "feature/done", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            delete_branches: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert!(result.succeeded[0].branch_deleted);

        let deletes = git.branch_delete_calls();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1, "feature/done");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("deleted branch feature/done"));
    }

    #[test]
    fn clean_skip_branch_delete_if_checked_out() {
        let git = MockGitBuilder::new()
            .with_is_branch_checked_out(&repo(), "feature/done", true)
            .build();
        let wt = make_worktree("wt-done", "feature/done", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            delete_branches: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert!(!result.succeeded[0].branch_deleted);
        assert!(git.branch_delete_calls().is_empty());

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("checked out elsewhere"));
    }

    #[test]
    fn clean_dry_run_mentions_branch_deletion() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-done", "feature/done", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            delete_branches: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remove_calls().len(), 0);
        assert_eq!(git.branch_delete_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would remove wt-done (and branch feature/done)"));
    }

    #[test]
    fn clean_handles_remove_error_both_tiers() {
        // Both normal and force removal fail, and the path doesn't exist on disk
        // so rm -rf is a no-op but prune should still be called.
        let wt_path = PathBuf::from("/dev/wt-fail");
        let git = MockGitBuilder::new()
            .with_worktree_remove_error(&wt_path, "tier 1 failed")
            .build();
        let wt = make_worktree("wt-fail", "feature/fail", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        // Fallback succeeds since the path doesn't exist
        assert_eq!(result.succeeded.len(), 1);
    }

    #[test]
    fn should_clean_default_includes_landed_and_by_content() {
        let options = default_options();

        assert!(should_clean(&Classification::Landed, &options));
        assert!(should_clean(&Classification::LandedStale, &options));
        assert!(should_clean(
            &Classification::LandedByContent {
                matched: 1,
                total: 1
            },
            &options
        ));
        assert!(!should_clean(
            &Classification::LandedPartial {
                matched: 1,
                total: 2,
                unmatched: vec![]
            },
            &options
        ));
        assert!(!should_clean(&Classification::Active, &options));
        assert!(!should_clean(&Classification::Local, &options));
    }

    #[test]
    fn should_clean_all_includes_everything() {
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        assert!(should_clean(&Classification::Landed, &options));
        assert!(should_clean(&Classification::LandedStale, &options));
        assert!(should_clean(&Classification::Active, &options));
        assert!(should_clean(&Classification::Local, &options));
    }

    #[test]
    fn should_clean_strict_only_structural() {
        let options = CleanOptions {
            strict: true,
            ..default_options()
        };

        assert!(should_clean(&Classification::Landed, &options));
        assert!(!should_clean(&Classification::LandedStale, &options));
        assert!(!should_clean(
            &Classification::LandedByContent {
                matched: 1,
                total: 1
            },
            &options
        ));
        assert!(!should_clean(&Classification::Active, &options));
    }

    #[test]
    fn clean_landed_stale_skips_branch_delete() {
        let git = MockGitBuilder::new().build();
        let wt = make_worktree("wt-stale", "feature/stale", Classification::LandedStale);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            delete_branches: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert!(!result.succeeded[0].branch_deleted);
        // No branch_delete calls — the branch ref is already gone
        assert!(git.branch_delete_calls().is_empty());
    }

    #[test]
    fn clean_dirty_landed_stale_bypasses_dirty_block() {
        let git = MockGitBuilder::new().build();
        let mut wt = make_worktree("wt-stale", "feature/stale", Classification::LandedStale);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 100;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remove_force_calls().len(), 1);
    }

    #[test]
    fn clean_dirty_matches_main_bypasses_dirty_block() {
        let wt_path = PathBuf::from("/dev/wt-landed");
        let git = MockGitBuilder::new()
            .with_diff_working_tree_files(&wt_path, "origin/main", vec![])
            .build();
        let mut wt = make_worktree("wt-landed", "feature/landed", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 5;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remove_force_calls().len(), 1);
    }

    #[test]
    fn clean_dirty_real_changes_still_blocked() {
        let wt_path = PathBuf::from("/dev/wt-dirty");
        let git = MockGitBuilder::new()
            .with_diff_working_tree_files(&wt_path, "origin/main", vec!["new_file.rs".to_string()])
            .build();
        let mut wt = make_worktree("wt-dirty", "feature/dirty", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 1;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipped wt-dirty: dirty (1 files)"));
    }

    #[test]
    fn clean_dirty_diff_failure_falls_back_to_blocking() {
        let wt_path = PathBuf::from("/dev/wt-dirty");
        let git = MockGitBuilder::new()
            .with_diff_working_tree_files_error(&wt_path, "origin/main", "ref not found")
            .with_diff_working_tree_files_error(&wt_path, "main", "ref not found")
            .build();
        let mut wt = make_worktree("wt-dirty", "feature/dirty", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 2;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_force_skips_diff_check_for_dirty() {
        // With --force, no diff calls should be made; the worktree is just removed
        let git = MockGitBuilder::new().build();
        let mut wt = make_worktree("wt-dirty", "feature/dirty", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 5;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remove_force_calls().len(), 1);
    }

    #[test]
    fn clean_delete_branches_fails_closed_when_checkedout_check_errors() {
        // Regression: previously `is_branch_checked_out().unwrap_or(false)` silently treated errors as "not checked out" and proceeded to delete the branch. Now the delete is skipped and a warning is emitted.
        let git = MockGitBuilder::new()
            .with_is_branch_checked_out_error(&repo(), "feature/done", "git plumbing broke")
            .build();
        let wt = make_worktree("wt-done", "feature/done", Classification::Landed);
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            delete_branches: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        // Worktree itself is still removed (that path's safety is separate).
        assert_eq!(result.succeeded.len(), 1);
        // But the branch must NOT be deleted when we cannot prove it is safe.
        assert_eq!(git.branch_delete_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("skipped branch delete for feature/done"),
            "output should warn about skipped delete, got: {output}"
        );
        assert!(
            output.contains("could not check worktree usage"),
            "output should mention the underlying error, got: {output}"
        );
    }

    #[test]
    fn clean_dirty_diff_falls_back_to_local_ref() {
        let wt_path = PathBuf::from("/dev/wt-local");
        let git = MockGitBuilder::new()
            .with_diff_working_tree_files_error(&wt_path, "origin/main", "ref not found")
            .with_diff_working_tree_files(&wt_path, "main", vec![])
            .build();
        let mut wt = make_worktree("wt-local", "feature/local", Classification::Landed);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 3;
        let scan = make_scan(vec![wt]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remove_force_calls().len(), 1);
    }
}
