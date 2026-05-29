mod common;

use std::collections::HashSet;

use git_config_tidy::fix::{self, FixOptions};
use git_config_tidy::lint::lint_repo;
use git_config_tidy::types::IssueKind;
use git_tidy_core::git::RealGit;

use common::*;

/// Set up a repo with an orphaned branch config entry.
fn set_up_orphaned_config(test: &TestRepo) {
    git(&test.main_repo, &["checkout", "-b", "stale-branch"]);
    git(
        &test.main_repo,
        &["config", "branch.stale-branch.remote", "origin"],
    );
    git(
        &test.main_repo,
        &[
            "config",
            "branch.stale-branch.merge",
            "refs/heads/stale-branch",
        ],
    );
    git(&test.main_repo, &["checkout", "main"]);
    git(
        &test.main_repo,
        &["update-ref", "-d", "refs/heads/stale-branch"],
    );
}

#[test]
fn fix_removes_orphaned_config() {
    let test = TestRepo::new();
    set_up_orphaned_config(&test);

    let git_ops = RealGit;
    let builtins = HashSet::new();

    // Verify the issue is detected
    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::OrphanedBranchConfig);

    // Build a lint result for run_fix
    let lint_result = git_config_tidy::types::ConfigLintResult {
        repos: vec![git_config_tidy::types::ConfigRepoGroup {
            repo_path: test.main_repo.clone(),
            name: "main-repo".to_string(),
            issues,
        }],
        total_scanned: 1,
        counts: git_config_tidy::types::IssueCounts {
            orphaned_branch_config: 1,
            ..Default::default()
        },
        warnings: vec![],
    };

    let options = FixOptions { dry_run: false };
    let mut buf = Vec::new();
    let result = fix::run_fix(&git_ops, &lint_result, &options, &mut buf).unwrap();

    assert_eq!(result.fixed.len(), 1);
    assert!(result.failed.is_empty());

    // Verify the config is gone
    let issues_after = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();
    assert!(issues_after.is_empty());
}

#[test]
fn fix_dry_run_preserves_config() {
    let test = TestRepo::new();
    set_up_orphaned_config(&test);

    let git_ops = RealGit;
    let builtins = HashSet::new();

    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();
    assert_eq!(issues.len(), 1);

    let lint_result = git_config_tidy::types::ConfigLintResult {
        repos: vec![git_config_tidy::types::ConfigRepoGroup {
            repo_path: test.main_repo.clone(),
            name: "main-repo".to_string(),
            issues,
        }],
        total_scanned: 1,
        counts: git_config_tidy::types::IssueCounts {
            orphaned_branch_config: 1,
            ..Default::default()
        },
        warnings: vec![],
    };

    let options = FixOptions { dry_run: true };
    let mut buf = Vec::new();
    let result = fix::run_fix(&git_ops, &lint_result, &options, &mut buf).unwrap();

    assert_eq!(result.fixed.len(), 1);

    // Config should still be there
    let issues_after = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();
    assert_eq!(issues_after.len(), 1);
}
