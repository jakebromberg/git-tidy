mod common;

use std::collections::BTreeMap;

use common::TestRepo;
use git_tidy_core::git::{GitOps, RealGit};
use git_tidy_core::progress::Progress;
use git_tidy_core::types::Classification;
use git_worktree_tidy::discovery::DiscoveredWorktree;

#[test]
fn diff_commit_handles_root_commit() {
    let test = TestRepo::new();
    let git = RealGit;

    // The initial commit created by TestRepo::new() is a root commit (no parent)
    let root_hash = common::git(&test.main_repo, &["rev-parse", "HEAD"]);

    let diff = git.diff_commit(&test.main_repo, &root_hash).unwrap();
    assert!(
        diff.contains("README.md"),
        "root commit diff should show the added file"
    );
}

#[test]
fn diff_commit_files_handles_root_commit() {
    let test = TestRepo::new();
    let git = RealGit;

    let root_hash = common::git(&test.main_repo, &["rev-parse", "HEAD"]);

    let files = git.diff_commit_files(&test.main_repo, &root_hash).unwrap();
    assert!(
        files.contains(&"README.md".to_string()),
        "root commit should list README.md as a changed file, got: {files:?}"
    );
}

#[test]
fn diff_commit_on_ref_handles_root_commit() {
    let test = TestRepo::new();
    let git = RealGit;

    let root_hash = common::git(&test.main_repo, &["rev-parse", "HEAD"]);

    let diff = git.diff_commit_on_ref(&test.main_repo, &root_hash).unwrap();
    assert!(
        diff.contains("README.md"),
        "root commit diff should show the added file"
    );
}

#[test]
fn list_local_branches_returns_all_branches() {
    let test = TestRepo::new();
    let git = RealGit;

    // Create a couple of branches
    common::git(&test.main_repo, &["branch", "feature-a"]);
    common::git(&test.main_repo, &["branch", "feature-b"]);

    let branches = git.list_local_branches(&test.main_repo).unwrap();
    assert!(branches.contains(&"main".to_string()));
    assert!(branches.contains(&"feature-a".to_string()));
    assert!(branches.contains(&"feature-b".to_string()));
    assert_eq!(branches.len(), 3);
}

#[test]
fn current_branch_returns_checked_out_branch() {
    let test = TestRepo::new();
    let git = RealGit;

    let branch = git.current_branch(&test.main_repo).unwrap();
    assert_eq!(branch, Some("main".to_string()));
}

#[test]
fn current_branch_returns_none_for_detached_head() {
    let test = TestRepo::new();
    let git = RealGit;

    let head = common::git(&test.main_repo, &["rev-parse", "HEAD"]);
    common::git(&test.main_repo, &["checkout", "--detach", &head]);

    let branch = git.current_branch(&test.main_repo).unwrap();
    assert_eq!(branch, None);
}

#[test]
fn branch_delete_safe_deletes_merged_branch() {
    let test = TestRepo::new();
    let git = RealGit;

    // Create and merge a branch
    common::git(&test.main_repo, &["branch", "merged-feature"]);
    // It's already at main's tip so it's considered merged

    git.branch_delete_safe(&test.main_repo, "merged-feature")
        .unwrap();

    let branches = git.list_local_branches(&test.main_repo).unwrap();
    assert!(!branches.contains(&"merged-feature".to_string()));
}

#[test]
fn branch_delete_safe_refuses_unmerged_branch() {
    let test = TestRepo::new();
    let git = RealGit;

    // Create a branch with unique work
    common::git(&test.main_repo, &["checkout", "-b", "unmerged-feature"]);
    test.commit_file(&test.main_repo, "new.txt", "content", "unique commit");
    common::git(&test.main_repo, &["checkout", "main"]);

    let result = git.branch_delete_safe(&test.main_repo, "unmerged-feature");
    assert!(
        result.is_err(),
        "branch -d should refuse to delete unmerged branch"
    );
}

#[test]
fn worktree_with_deleted_branch_classifies_as_landed_stale() {
    let test = TestRepo::new();
    let git = RealGit;

    // Create a worktree on a feature branch
    let wt_path = test.add_worktree("main-repo-stale", "feature/stale-branch");

    // Merge the branch into main, then delete the branch ref directly
    // (git branch -d won't delete a branch checked out in a worktree)
    common::git(&test.main_repo, &["merge", "feature/stale-branch"]);
    common::git(
        &test.main_repo,
        &["update-ref", "-d", "refs/heads/feature/stale-branch"],
    );

    // Verify the branch ref is gone
    assert!(
        !git.rev_parse_verify(&test.main_repo, "refs/heads/feature/stale-branch")
            .unwrap(),
        "branch ref should be deleted"
    );

    // Verify the worktree still reports the branch name
    let branch = git.worktree_branch(&wt_path).unwrap();
    assert_eq!(
        branch.as_deref(),
        Some("feature/stale-branch"),
        "worktree HEAD should still reference the deleted branch"
    );

    // Scan using run_scan_repos and verify LandedStale classification
    let mut groups = BTreeMap::new();
    groups.insert(
        test.main_repo.clone(),
        vec![DiscoveredWorktree {
            path: wt_path.clone(),
            parent_repo: test.main_repo.clone(),
        }],
    );

    let progress = Progress::disabled();
    let result =
        git_worktree_tidy::scan::run_scan_repos(&git, groups, 100, false, &[], &progress).unwrap();

    assert_eq!(result.total_scanned, 1);
    assert_eq!(result.repos.len(), 1);
    let wt_info = &result.repos[0].worktrees[0];
    assert_eq!(
        wt_info.classification,
        Classification::LandedStale,
        "worktree with deleted branch ref should be classified as LandedStale"
    );
    assert_eq!(wt_info.branch.as_deref(), Some("feature/stale-branch"));
    assert_eq!(result.counts.landed_stale, 1);
}
