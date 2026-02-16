mod common;

use common::git;
use git_tidy_core::git::{GitOps, RealGit};

/// Set up a repo with a remote so detect_default_branch works.
fn set_up_repo_with_remote() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    let bare = base.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);

    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join("my-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);

    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);
    git(&repo, &["push", "-u", "origin", "main"]);

    (dir, repo)
}

#[test]
fn clean_deletes_merged_branch_in_real_repo() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a merged branch (at same commit as main)
    git(&repo, &["branch", "feature/done"]);

    // Verify it exists
    let branches = git_ops.list_local_branches(&repo).unwrap();
    assert!(branches.contains(&"feature/done".to_string()));

    // Scan then clean
    let scan_result = git_branch_tidy::scan::run_scan(&git_ops, &scan_dir, 100, false).unwrap();

    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        yes: true,
        merged_only: false,
        landed: false,
        all: false,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.deleted.len(), 1);
    assert_eq!(result.deleted[0].name, "feature/done");

    // Verify it's actually gone
    let branches = git_ops.list_local_branches(&repo).unwrap();
    assert!(!branches.contains(&"feature/done".to_string()));
}

#[test]
fn clean_dry_run_does_not_delete() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    git(&repo, &["branch", "feature/done"]);

    let scan_result = git_branch_tidy::scan::run_scan(&git_ops, &scan_dir, 100, false).unwrap();

    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: true,
        force: false,
        yes: true,
        merged_only: false,
        landed: false,
        all: false,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Reports it would delete
    assert_eq!(result.deleted.len(), 1);

    // But the branch still exists
    let branches = git_ops.list_local_branches(&repo).unwrap();
    assert!(branches.contains(&"feature/done".to_string()));

    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("would delete feature/done"));
}

#[test]
fn clean_safe_refuses_unmerged_branch() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a branch with unique work (not merged)
    git(&repo, &["checkout", "-b", "feature/wip"]);
    std::fs::write(repo.join("wip.txt"), "work").unwrap();
    git(&repo, &["add", "wip.txt"]);
    git(&repo, &["commit", "-m", "wip"]);
    git(&repo, &["checkout", "main"]);

    let scan_result = git_branch_tidy::scan::run_scan(&git_ops, &scan_dir, 100, false).unwrap();

    // Use --all to include it, but without --force (so -d will fail)
    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        yes: true,
        merged_only: false,
        landed: false,
        all: true,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Should fail (branch -d refuses unmerged)
    assert_eq!(result.deleted.len(), 0);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].name, "feature/wip");

    // Branch still exists
    let branches = git_ops.list_local_branches(&repo).unwrap();
    assert!(branches.contains(&"feature/wip".to_string()));
}
