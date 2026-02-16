mod common;

use common::TestRepo;
use git_tidy_core::git::{GitOps, RealGit};

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
