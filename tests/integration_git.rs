mod common;

use common::TestRepo;
use git_worktree_tidy::git::{GitOps, RealGit};

#[test]
fn diff_commit_handles_root_commit() {
    let test = TestRepo::new();
    let git = RealGit;

    // The initial commit created by TestRepo::new() is a root commit (no parent)
    let root_hash = common::git(&test.main_repo, &["rev-parse", "HEAD"]);

    let diff = git.diff_commit(&test.main_repo, &root_hash).unwrap();
    assert!(diff.contains("README.md"), "root commit diff should show the added file");
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
    assert!(diff.contains("README.md"), "root commit diff should show the added file");
}
