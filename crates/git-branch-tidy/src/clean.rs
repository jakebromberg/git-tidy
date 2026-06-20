use std::io::Write;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{Classification, CleanResult, FailedItem, RemovedRef};

use crate::types::BranchScanResult;

/// Options controlling branch cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be deleted.
    pub dry_run: bool,
    /// Force-delete branches (git branch -D instead of -d).
    pub force: bool,
    /// Only target structurally-proven landed branches.
    pub strict: bool,
    /// Include all classifications in the interactive flow.
    pub all: bool,
    /// Also delete remote tracking branches.
    pub include_remote: bool,
}

/// Run the clean operation on a scan result.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &BranchScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<RemovedRef>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    for group in &scan_result.repos {
        for branch in &group.branches {
            // Never delete the currently checked-out branch
            if branch.is_current {
                skipped += 1;
                continue;
            }

            // Filter by classification
            if !should_clean(&branch.classification, options) {
                skipped += 1;
                continue;
            }

            // Remote-only branches: delete from origin directly, skip local deletion
            if branch.remote_only {
                if options.dry_run {
                    writeln!(out, "would delete remote {} in {}", branch.name, group.name)?;
                    succeeded.push(RemovedRef {
                        repo: branch.repo_path.clone(),
                        name: branch.name.clone(),
                        remote_deleted: true,
                    });
                    continue;
                }

                match git.delete_remote_branch(&branch.repo_path, "origin", &branch.name) {
                    Ok(()) => {
                        writeln!(out, "deleted remote {}", branch.name)?;
                        succeeded.push(RemovedRef {
                            repo: branch.repo_path.clone(),
                            name: branch.name.clone(),
                            remote_deleted: true,
                        });
                    }
                    Err(e) => {
                        writeln!(out, "error: could not delete remote {}: {e}", branch.name)?;
                        failed.push(FailedItem {
                            repo: branch.repo_path.clone(),
                            name: branch.name.clone(),
                            reason: e.to_string(),
                        });
                    }
                }
                continue;
            }

            if options.dry_run {
                write!(out, "would delete {}", branch.name)?;
                if options.include_remote && branch.remote_tracking && !branch.remote_deleted {
                    write!(out, " (and remote)")?;
                }
                writeln!(out, " in {}", group.name)?;
                succeeded.push(RemovedRef {
                    repo: branch.repo_path.clone(),
                    name: branch.name.clone(),
                    remote_deleted: false,
                });
                continue;
            }

            // Delete the local branch.
            // Use force-delete for branches our analysis has confirmed as landed,
            // since git's built-in merge check doesn't understand squash merges.
            let force_delete = options.force
                || matches!(
                    branch.classification,
                    Classification::Landed
                        | Classification::LandedStale
                        | Classification::LandedByContent { .. }
                );
            let delete_result = if force_delete {
                git.branch_delete(&branch.repo_path, &branch.name)
            } else {
                git.branch_delete_safe(&branch.repo_path, &branch.name)
            };

            match delete_result {
                Ok(()) => {
                    let mut remote_deleted = false;

                    // Delete remote branch if requested
                    if options.include_remote
                        && branch.remote_tracking
                        && !branch.remote_deleted
                        && let Some(remote) =
                            derive_remote_name(git, &branch.repo_path, &branch.name)
                    {
                        match git.delete_remote_branch(&branch.repo_path, &remote, &branch.name) {
                            Ok(()) => {
                                remote_deleted = true;
                            }
                            Err(e) => {
                                writeln!(
                                    out,
                                    "warning: could not delete remote branch {}/{}: {e}",
                                    remote, branch.name
                                )?;
                            }
                        }
                    }

                    writeln!(
                        out,
                        "deleted {}{}",
                        branch.name,
                        if remote_deleted { " (and remote)" } else { "" }
                    )?;
                    succeeded.push(RemovedRef {
                        repo: branch.repo_path.clone(),
                        name: branch.name.clone(),
                        remote_deleted,
                    });
                }
                Err(e) => {
                    writeln!(out, "error: could not delete {}: {e}", branch.name)?;
                    failed.push(FailedItem {
                        repo: branch.repo_path.clone(),
                        name: branch.name.clone(),
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

/// Determine if a branch should be cleaned based on its classification and options.
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

/// Derive the remote name from a branch's upstream tracking info.
/// Returns None if there's no tracking info.
fn derive_remote_name(git: &dyn GitOps, repo: &std::path::Path, branch: &str) -> Option<String> {
    let upstream = git.upstream_branch(repo, branch).ok()??;
    // upstream is like "origin/feature-x" — extract the remote name
    upstream.split('/').next().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::counts::Counts;
    use git_tidy_core::testutil::MockGitBuilder;
    use git_tidy_core::types::ClassificationLabel;

    use super::*;
    use crate::types::{BranchInfo, BranchRepoGroup, BranchScanResult};

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(branches: Vec<BranchInfo>) -> BranchScanResult {
        let mut counts = Counts::default();
        for b in &branches {
            counts.increment(b.classification.label());
        }
        BranchScanResult {
            repos: vec![BranchRepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                branches,
            }],
            total_scanned: 0,
            counts,
            warnings: vec![],
        }
    }

    fn merged_branch(name: &str) -> BranchInfo {
        BranchInfo {
            repo_path: repo(),
            name: name.to_string(),
            default_branch: "main".to_string(),
            classification: Classification::Landed,
            remote_tracking: false,
            remote_deleted: true,
            ahead: 0,
            behind: 0,
            diverged: false,
            is_current: false,
            remote_only: false,
        }
    }

    fn active_branch(name: &str) -> BranchInfo {
        BranchInfo {
            repo_path: repo(),
            name: name.to_string(),
            default_branch: "main".to_string(),
            classification: Classification::Active,
            remote_tracking: true,
            remote_deleted: false,
            ahead: 3,
            behind: 0,
            diverged: false,
            is_current: false,
            remote_only: false,
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            force: false,
            strict: false,
            all: false,
            include_remote: false,
        }
    }

    #[test]
    fn clean_deletes_merged_branches() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![merged_branch("feature/done")]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "feature/done");
        // Landed branches use force-delete since git's merge check is redundant
        assert_eq!(git.branch_delete_calls().len(), 1);
    }

    #[test]
    fn clean_skips_current_branch() {
        let git = MockGitBuilder::new().build();
        let mut branch = merged_branch("feature/current");
        branch.is_current = true;
        let scan = make_scan_result(vec![branch]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_skips_active_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![active_branch("feature/wip")]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_all_includes_active() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![active_branch("feature/wip")]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        // --all without --force uses branch_delete_safe
        assert_eq!(git.branch_delete_safe_calls().len(), 1);
    }

    #[test]
    fn clean_force_uses_branch_delete() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![merged_branch("feature/done")]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        // --force uses branch_delete (-D) instead of branch_delete_safe (-d)
        assert_eq!(git.branch_delete_calls().len(), 1);
        assert_eq!(git.branch_delete_safe_calls().len(), 0);
    }

    #[test]
    fn clean_dry_run_makes_zero_delete_calls() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![merged_branch("feature/a"), merged_branch("feature/b")]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.branch_delete_safe_calls().len(), 0);
        assert_eq!(git.branch_delete_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would delete feature/a"));
        assert!(output.contains("would delete feature/b"));
    }

    #[test]
    fn clean_strict_skips_landed_by_content() {
        let git = MockGitBuilder::new().build();
        let mut content = merged_branch("feature/content");
        content.classification = Classification::LandedByContent {
            matched: 3,
            total: 3,
        };
        let scan = make_scan_result(vec![merged_branch("feature/landed"), content]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            strict: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "feature/landed");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_landed_by_content_uses_force_delete() {
        let git = MockGitBuilder::new().build();
        let mut branch = merged_branch("feature/squashed");
        branch.classification = Classification::LandedByContent {
            matched: 2,
            total: 2,
        };
        let scan = make_scan_result(vec![branch]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        // LandedByContent uses force-delete since git's merge check
        // doesn't understand squash merges
        assert_eq!(git.branch_delete_calls().len(), 1);
        assert_eq!(git.branch_delete_safe_calls().len(), 0);
    }

    #[test]
    fn clean_include_remote_deletes_remote_branch() {
        let git = MockGitBuilder::new()
            .with_upstream_branch(&repo(), "feature/done", Some("origin/feature/done"))
            .build();
        let mut branch = merged_branch("feature/done");
        branch.remote_tracking = true;
        branch.remote_deleted = false;
        let scan = make_scan_result(vec![branch]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            include_remote: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert!(result.succeeded[0].remote_deleted);
        assert_eq!(git.delete_remote_branch_calls().len(), 1);
        assert_eq!(
            git.delete_remote_branch_calls()[0],
            (repo(), "origin".to_string(), "feature/done".to_string())
        );
    }

    #[test]
    fn clean_include_remote_warns_on_failure() {
        let git = MockGitBuilder::new()
            .with_upstream_branch(&repo(), "feature/done", Some("origin/feature/done"))
            .with_delete_remote_branch_error(
                &repo(),
                "origin",
                "feature/done",
                "remote not reachable",
            )
            .build();
        let mut branch = merged_branch("feature/done");
        branch.remote_tracking = true;
        branch.remote_deleted = false;
        let scan = make_scan_result(vec![branch]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            include_remote: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        // Local delete succeeded
        assert_eq!(result.succeeded.len(), 1);
        assert!(!result.succeeded[0].remote_deleted);
        // Warning in output
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("warning: could not delete remote branch"));
    }

    #[test]
    fn clean_handles_delete_failure() {
        let git = MockGitBuilder::new()
            .with_branch_delete_error(&repo(), "feature/unmerged", "not fully merged")
            .build();
        let scan = make_scan_result(vec![merged_branch("feature/unmerged")]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "feature/unmerged");
    }

    fn remote_only_branch(name: &str) -> BranchInfo {
        BranchInfo {
            repo_path: repo(),
            name: name.to_string(),
            default_branch: "main".to_string(),
            classification: Classification::Landed,
            remote_tracking: true,
            remote_deleted: false,
            ahead: 0,
            behind: 0,
            diverged: false,
            is_current: false,
            remote_only: true,
        }
    }

    #[test]
    fn clean_deletes_remote_only_branch() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote_only_branch("feature/remote")]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "feature/remote");
        assert!(result.succeeded[0].remote_deleted);
        // Should NOT call local branch_delete
        assert_eq!(git.branch_delete_calls().len(), 0);
        assert_eq!(git.branch_delete_safe_calls().len(), 0);
        // Should call delete_remote_branch
        assert_eq!(git.delete_remote_branch_calls().len(), 1);
        assert_eq!(
            git.delete_remote_branch_calls()[0],
            (repo(), "origin".to_string(), "feature/remote".to_string())
        );
    }

    #[test]
    fn clean_dry_run_remote_only() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote_only_branch("feature/remote")]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.delete_remote_branch_calls().len(), 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would delete remote feature/remote"));
    }

    #[test]
    fn clean_dry_run_mixed_local_and_remote_only() {
        let git = MockGitBuilder::new()
            .with_upstream_branch(&repo(), "feature/local", Some("origin/feature/local"))
            .build();
        let mut local = merged_branch("feature/local");
        local.remote_tracking = true;
        local.remote_deleted = false;
        let scan = make_scan_result(vec![local, remote_only_branch("feature/remote")]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            include_remote: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would delete feature/local (and remote)"));
        assert!(output.contains("would delete remote feature/remote"));
    }
}
