mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_lfs_tidy::clean::{CleanOptions, CleanResult};
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

fn run_scan_and_clean(
    scan_dir: &std::path::Path,
    options: &CleanOptions,
) -> (git_lfs_tidy::types::LfsScanResult, CleanResult) {
    let git_ops = RealGit;
    let scan_result = git_lfs_tidy::scan::run_scan(
        &git_ops,
        scan_dir,
        100_000,
        1000,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();
    let mut buf = Vec::new();
    let clean_result =
        git_lfs_tidy::clean::run_clean(&git_ops, &scan_result, options, &mut buf).unwrap();
    (scan_result, clean_result)
}

#[test]
fn clean_dry_run_with_untracked_blobs() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");

    // Create a large file (above 100KB threshold)
    let large_content = "x".repeat(200_000);
    std::fs::write(repo.join("large.bin"), &large_content).unwrap();
    git(&repo, &["add", "large.bin"]);
    git(&repo, &["commit", "-m", "Add large file"]);

    let options = CleanOptions {
        dry_run: true,
        yes: false,
        prune: true,
    };

    let (scan_result, clean_result) = run_scan_and_clean(&scan_dir, &options);

    // Should have untracked items but nothing to prune (no orphaned LFS objects)
    assert!(
        scan_result
            .repos
            .iter()
            .flat_map(|r| &r.items)
            .any(|i| i.classification == LfsClassification::Untracked),
        "expected untracked items"
    );
    assert_eq!(clean_result.pruned.len(), 0);
    assert!(
        clean_result
            .recommendations
            .iter()
            .any(|r| r.contains("git lfs migrate"))
    );
}

#[test]
fn clean_with_no_orphaned_is_noop() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");

    // Create a large file (above 100KB threshold)
    let large_content = "x".repeat(200_000);
    std::fs::write(repo.join("large.bin"), &large_content).unwrap();
    git(&repo, &["add", "large.bin"]);
    git(&repo, &["commit", "-m", "Add large file"]);

    let options = CleanOptions {
        dry_run: false,
        yes: true,
        prune: true,
    };

    let (_scan_result, clean_result) = run_scan_and_clean(&scan_dir, &options);

    // No LFS objects to prune, so nothing pruned
    assert_eq!(clean_result.pruned.len(), 0);
    assert_eq!(clean_result.failed.len(), 0);
}
