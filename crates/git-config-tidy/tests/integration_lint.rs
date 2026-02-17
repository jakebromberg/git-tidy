mod common;

use std::collections::HashSet;

use git_config_tidy::lint::lint_repo;
use git_config_tidy::types::IssueKind;
use git_tidy_core::git::{GitOps, RealGit};

use common::*;

#[test]
fn lint_detects_orphaned_branch_config() {
    let test = TestRepo::new();

    // Create a branch with tracking config
    git(&test.main_repo, &["checkout", "-b", "feature-x"]);
    git(
        &test.main_repo,
        &["config", "branch.feature-x.remote", "origin"],
    );
    git(
        &test.main_repo,
        &["config", "branch.feature-x.merge", "refs/heads/feature-x"],
    );
    git(&test.main_repo, &["checkout", "main"]);

    // Delete the branch via low-level ref manipulation (leaves config behind)
    git(
        &test.main_repo,
        &["update-ref", "-d", "refs/heads/feature-x"],
    );

    let git_ops = RealGit;
    let builtins = HashSet::new();
    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::OrphanedBranchConfig);
    assert_eq!(issues[0].section, Some("branch.feature-x".to_string()));
}

#[test]
fn lint_clean_repo_has_no_issues() {
    let test = TestRepo::new();

    let git_ops = RealGit;
    let builtins = HashSet::new();
    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();

    assert!(issues.is_empty());
}

#[test]
fn lint_detects_alias_shadowing_builtin() {
    let test = TestRepo::new();

    // Set a local alias that shadows "log"
    git(
        &test.main_repo,
        &["config", "--local", "alias.log", "log --oneline --graph"],
    );

    let git_ops = RealGit;
    // Get real builtin commands
    let builtins: HashSet<String> = git_ops
        .list_builtin_commands()
        .unwrap()
        .into_iter()
        .collect();

    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::AliasShadowsBuiltin);
    assert_eq!(issues[0].key, "alias.log");
}

#[test]
fn lint_existing_branch_not_orphaned() {
    let test = TestRepo::new();

    // Create a branch and set config for it
    git(&test.main_repo, &["checkout", "-b", "develop"]);
    git(
        &test.main_repo,
        &["config", "branch.develop.remote", "origin"],
    );
    git(
        &test.main_repo,
        &["config", "branch.develop.merge", "refs/heads/develop"],
    );
    git(&test.main_repo, &["checkout", "main"]);

    let git_ops = RealGit;
    let builtins = HashSet::new();
    let issues = lint_repo(&git_ops, &test.main_repo, &builtins).unwrap();

    assert!(issues.is_empty());
}
