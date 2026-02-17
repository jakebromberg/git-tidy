mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_lfs_tidy::types::LfsClassification;

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
fn scan_repo_with_no_large_blobs() {
    let (dir, _repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Default threshold is 1MB, README.md is tiny
    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 1_000_000, 1000).unwrap();

    // Should find the repo but no items (all blobs below threshold)
    assert!(result.repos.is_empty() || result.repos[0].items.is_empty());
}

#[test]
fn scan_repo_with_large_blob() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a file larger than the threshold (100KB threshold)
    let large_content = "x".repeat(200_000);
    std::fs::write(repo.join("large.bin"), &large_content).unwrap();
    git(&repo, &["add", "large.bin"]);
    git(&repo, &["commit", "-m", "Add large file"]);

    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 100_000, 1000).unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);

    let untracked: Vec<_> = result.repos[0]
        .items
        .iter()
        .filter(|i| i.classification == LfsClassification::Untracked)
        .collect();
    assert!(
        !untracked.is_empty(),
        "expected at least one untracked item"
    );

    let large_item = untracked.iter().find(|i| i.path == "large.bin");
    assert!(large_item.is_some(), "expected large.bin in results");
    assert!(large_item.unwrap().size_bytes.unwrap() >= 200_000);
}

#[test]
fn scan_repo_with_large_blob_below_threshold() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a 50KB file, scan with 100KB threshold
    let content = "x".repeat(50_000);
    std::fs::write(repo.join("medium.bin"), &content).unwrap();
    git(&repo, &["add", "medium.bin"]);
    git(&repo, &["commit", "-m", "Add medium file"]);

    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 100_000, 1000).unwrap();

    // Should not flag medium.bin since it's below threshold
    let untracked_count: usize = result
        .repos
        .iter()
        .flat_map(|r| &r.items)
        .filter(|i| i.classification == LfsClassification::Untracked)
        .count();
    assert_eq!(untracked_count, 0);
}

#[test]
fn scan_multiple_repos() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    // Repo A: has a large blob
    let repo_a = scan_dir.join("repo-a");
    std::fs::create_dir_all(&repo_a).unwrap();
    git(&repo_a, &["init", "-b", "main"]);
    git(&repo_a, &["config", "user.email", "test@test.com"]);
    git(&repo_a, &["config", "user.name", "Test"]);
    let large = "x".repeat(200_000);
    std::fs::write(repo_a.join("big.dat"), &large).unwrap();
    git(&repo_a, &["add", "big.dat"]);
    git(&repo_a, &["commit", "-m", "Add big file"]);

    // Repo B: only small files
    let repo_b = scan_dir.join("repo-b");
    std::fs::create_dir_all(&repo_b).unwrap();
    git(&repo_b, &["init", "-b", "main"]);
    git(&repo_b, &["config", "user.email", "test@test.com"]);
    git(&repo_b, &["config", "user.name", "Test"]);
    std::fs::write(repo_b.join("small.txt"), "hello\n").unwrap();
    git(&repo_b, &["add", "small.txt"]);
    git(&repo_b, &["commit", "-m", "Add small file"]);

    let git_ops = RealGit;
    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 100_000, 1000).unwrap();

    // Only repo-a should have items (untracked large blob)
    let repos_with_items: Vec<_> = result
        .repos
        .iter()
        .filter(|r| !r.items.is_empty())
        .collect();
    assert_eq!(repos_with_items.len(), 1);
    assert!(repos_with_items[0].name.contains("repo-a"));
}

#[test]
fn scan_repo_with_no_commits() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join("empty-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);

    let git_ops = RealGit;
    // Should not crash on an empty repo
    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 1_000_000, 1000).unwrap();

    // Empty repo should have no items
    let total_items: usize = result.repos.iter().map(|r| r.items.len()).sum();
    assert_eq!(total_items, 0);
}

#[test]
fn scan_threshold_edge_case() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a file of exactly 1000 bytes and set threshold to 1000
    let content = "x".repeat(1000);
    std::fs::write(repo.join("edge.bin"), &content).unwrap();
    git(&repo, &["add", "edge.bin"]);
    git(&repo, &["commit", "-m", "Add edge-case file"]);

    // Threshold = 1000: file is exactly at threshold, should be flagged (>= comparison)
    let result = git_lfs_tidy::scan::run_scan(&git_ops, &scan_dir, 1000, 1000).unwrap();

    let untracked: Vec<_> = result
        .repos
        .iter()
        .flat_map(|r| &r.items)
        .filter(|i| i.classification == LfsClassification::Untracked && i.path == "edge.bin")
        .collect();

    // find_large_blobs uses `>=` so a 1000-byte file with threshold=1000 should be flagged
    assert_eq!(untracked.len(), 1, "file at threshold should be flagged");
}
