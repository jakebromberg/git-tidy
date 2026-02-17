mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_repo_tidy::types::RepoClassification;

/// Set up a repo inside a scan directory so `discover_repos` finds it.
fn set_up_repo(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let scan_dir = dir.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    repo
}

#[test]
fn scan_active_repo_with_remote() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_repo(&base, "my-repo");
    let scan_dir = base.join("projects");

    // Set up a reachable bare remote
    let bare = base.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    let git_ops = RealGit;
    let result = git_repo_tidy::scan::run_scan(&git_ops, &scan_dir, 180, &[], false).unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.repos[0].classification, RepoClassification::Active);
    assert!(result.repos[0].has_remote);
    assert!(!result.repos[0].is_dirty);
    assert!(result.repos[0].disk_usage_bytes > 0);
}

#[test]
fn scan_orphaned_no_remote() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let _repo = set_up_repo(&base, "orphan-repo");
    let scan_dir = base.join("projects");

    let git_ops = RealGit;
    let result = git_repo_tidy::scan::run_scan(&git_ops, &scan_dir, 180, &[], false).unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.repos[0].classification, RepoClassification::Orphaned);
    assert!(!result.repos[0].has_remote);
}

#[test]
fn scan_stale_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_repo(&base, "stale-repo");
    let scan_dir = base.join("projects");

    // Back-date the commit to make it stale (2 years ago)
    git(
        &repo,
        &[
            "commit",
            "--amend",
            "--date=2024-01-01T12:00:00+00:00",
            "--no-edit",
        ],
    );

    // Set up a reachable remote so it classifies as stale, not orphaned
    let bare = base.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    let git_ops = RealGit;
    // stale_days = 180 (6 months); commit is ~2 years old
    let result = git_repo_tidy::scan::run_scan(&git_ops, &scan_dir, 180, &[], false).unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.repos[0].classification, RepoClassification::Stale);
}

#[test]
fn scan_dirty_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_repo(&base, "dirty-repo");
    let scan_dir = base.join("projects");

    // Create an uncommitted file
    std::fs::write(repo.join("dirty.txt"), "uncommitted").unwrap();

    let git_ops = RealGit;
    let result = git_repo_tidy::scan::run_scan(&git_ops, &scan_dir, 180, &[], false).unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert!(result.repos[0].is_dirty);
    assert!(result.repos[0].dirty_file_count > 0);
}

#[test]
fn scan_reclaimable_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let _orphan = set_up_repo(&base, "orphan");
    let scan_dir = base.join("projects");

    let git_ops = RealGit;
    let result = git_repo_tidy::scan::run_scan(&git_ops, &scan_dir, 180, &[], false).unwrap();

    // Orphaned repo's disk usage should be reclaimable
    assert_eq!(result.repos.len(), 1);
    assert!(result.reclaimable_bytes > 0);
    assert_eq!(result.reclaimable_bytes, result.repos[0].disk_usage_bytes);
}
