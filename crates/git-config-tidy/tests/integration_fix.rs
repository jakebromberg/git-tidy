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
fn fix_json_writes_valid_json_to_stdout_and_progress_to_stderr() {
    // Regression: previously `fix --json` wrote the lint JSON to stdout then run_fix wrote "removed section X in repo" lines to the same stream, producing a JSON document followed by free-form text that no JSON parser can consume. The fix must keep stdout as a single valid JSON value and route progress to stderr.
    let test = TestRepo::new();
    set_up_orphaned_config(&test);

    let binary = env!("CARGO_BIN_EXE_git-config-tidy");
    let output = std::process::Command::new(binary)
        .args(["fix", "--json"])
        .arg(&test.main_repo)
        .output()
        .expect("failed to run git-config-tidy");

    assert!(
        output.status.success() || output.status.code() == Some(0),
        "git-config-tidy fix --json failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("non-utf8 stdout");
    serde_json::from_str::<serde_json::Value>(&stdout).unwrap_or_else(|e| {
        panic!("stdout is not valid JSON: {e}\n--- stdout ---\n{stdout}");
    });

    let stderr = String::from_utf8(output.stderr).expect("non-utf8 stderr");
    assert!(
        stderr.contains("removed section"),
        "expected progress on stderr, got: {stderr}",
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
