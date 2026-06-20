mod common;

use common::{git, set_up_repo};
use git_tidy_core::git::RealGit;

use git_lfs_tidy::clean::{CleanOptions, LfsCleanResult};
use git_lfs_tidy::types::LfsClassification;

fn run_scan_and_clean(
    scan_dir: &std::path::Path,
    options: &CleanOptions,
) -> (git_lfs_tidy::types::LfsScanResult, LfsCleanResult) {
    let git_ops = RealGit;
    let scan_result = git_lfs_tidy::scan::run_scan(
        &git_ops,
        scan_dir,
        100_000,
        1000,
        false,
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
    assert_eq!(clean_result.succeeded.len(), 0);
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
        prune: true,
    };

    let (_scan_result, clean_result) = run_scan_and_clean(&scan_dir, &options);

    // No LFS objects to prune, so nothing pruned
    assert_eq!(clean_result.succeeded.len(), 0);
    assert_eq!(clean_result.failed.len(), 0);
}
