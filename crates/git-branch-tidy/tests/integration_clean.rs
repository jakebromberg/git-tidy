mod common;

use common::{git, set_up_repo_with_remote};
use git_tidy_core::git::{GitOps, RealGit};

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
    let scan_result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        false,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        strict: false,
        all: false,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.succeeded.len(), 1);
    assert_eq!(result.succeeded[0].name, "feature/done");

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

    let scan_result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        false,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: true,
        force: false,
        strict: false,
        all: false,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Reports it would delete
    assert_eq!(result.succeeded.len(), 1);

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

    let scan_result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        false,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Use --all to include it, but without --force (so -d will fail)
    let options = git_branch_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        strict: false,
        all: true,
        include_remote: false,
    };

    let mut buf = Vec::new();
    let result =
        git_branch_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Should fail (branch -d refuses unmerged)
    assert_eq!(result.succeeded.len(), 0);
    assert_eq!(result.failed.len(), 1);
    assert_eq!(result.failed[0].name, "feature/wip");

    // Branch still exists
    let branches = git_ops.list_local_branches(&repo).unwrap();
    assert!(branches.contains(&"feature/wip".to_string()));
}
