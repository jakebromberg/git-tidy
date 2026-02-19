mod common;

use common::git;
use git_tidy_core::git::{GitOps, RealGit};

/// Set up a repo inside a scan directory so `discover_repos` finds it.
fn set_up_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join("my-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    (dir, repo)
}

#[test]
fn clean_drops_stash_in_real_repo() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a stash on a branch, then delete the branch (orphaned)
    git(&repo, &["checkout", "-b", "doomed"]);
    std::fs::write(repo.join("doomed.txt"), "doomed content").unwrap();
    git(&repo, &["add", "doomed.txt"]);
    git(&repo, &["stash"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", "doomed"]);

    // Verify stash exists
    let stashes = git_ops.list_stashes(&repo).unwrap();
    assert_eq!(stashes.len(), 1);

    // Scan then clean
    let scan_result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_stash_tidy::clean::CleanOptions {
        dry_run: false,
        yes: true,
        committed_only: false,
        aged_only: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_stash_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.succeeded.len(), 1);

    // Verify stash is gone
    let stashes = git_ops.list_stashes(&repo).unwrap();
    assert!(stashes.is_empty());
}

#[test]
fn clean_dry_run_does_not_drop() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create an orphaned stash
    git(&repo, &["checkout", "-b", "temp"]);
    std::fs::write(repo.join("temp.txt"), "temp").unwrap();
    git(&repo, &["add", "temp.txt"]);
    git(&repo, &["stash"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", "temp"]);

    let scan_result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_stash_tidy::clean::CleanOptions {
        dry_run: true,
        yes: true,
        committed_only: false,
        aged_only: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_stash_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Reports it would drop
    assert_eq!(result.succeeded.len(), 1);

    // But stash still exists
    let stashes = git_ops.list_stashes(&repo).unwrap();
    assert_eq!(stashes.len(), 1);

    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("would drop"));
}
