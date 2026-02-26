mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_stash_tidy::types::StashClassification;

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
fn scan_real_repo_with_stashes() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a stash on main
    std::fs::write(repo.join("temp.txt"), "temporary content").unwrap();
    git(&repo, &["add", "temp.txt"]);
    git(&repo, &["stash"]);

    let result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);

    let stash = &result.repos[0].stashes[0];
    assert_eq!(stash.stash_ref, "stash@{0}");
    assert!(stash.branch.as_deref() == Some("main"));
    // Stash is recent and branch exists, so should be Active
    // (diff won't match since stash is new content, not committed to the branch)
    assert_eq!(stash.classification, StashClassification::Active);
}

#[test]
fn scan_repo_with_orphaned_stash() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a branch, stash on it, then delete the branch
    git(&repo, &["checkout", "-b", "temp-feature"]);
    std::fs::write(repo.join("feature.txt"), "feature content").unwrap();
    git(&repo, &["add", "feature.txt"]);
    git(&repo, &["stash"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", "temp-feature"]);

    let result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);

    let stash = &result.repos[0].stashes[0];
    assert_eq!(stash.classification, StashClassification::Orphaned);
    assert_eq!(stash.branch.as_deref(), Some("temp-feature"));
}

#[test]
fn scan_entity_filter_includes_matching_stashes() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create stashes on different branches
    git(&repo, &["checkout", "-b", "feature-alpha"]);
    std::fs::write(repo.join("alpha.txt"), "alpha content").unwrap();
    git(&repo, &["add", "alpha.txt"]);
    git(&repo, &["stash"]);

    git(&repo, &["checkout", "main"]);
    git(&repo, &["checkout", "-b", "bugfix-beta"]);
    std::fs::write(repo.join("beta.txt"), "beta content").unwrap();
    git(&repo, &["add", "beta.txt"]);
    git(&repo, &["stash"]);

    git(&repo, &["checkout", "main"]);

    // Filter to only stashes on branches matching "feature"
    let entity_filter = git_tidy_core::filter::NameFilter::new(&["feature".to_string()], &[]);

    let result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);
    assert_eq!(
        result.repos[0].stashes[0].branch.as_deref(),
        Some("feature-alpha")
    );
}

#[test]
fn scan_entity_filter_exclude_takes_precedence() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create stashes on different branches
    git(&repo, &["checkout", "-b", "feature-login"]);
    std::fs::write(repo.join("login.txt"), "login content").unwrap();
    git(&repo, &["add", "login.txt"]);
    git(&repo, &["stash"]);

    git(&repo, &["checkout", "main"]);
    git(&repo, &["checkout", "-b", "feature-wip"]);
    std::fs::write(repo.join("wip.txt"), "wip content").unwrap();
    git(&repo, &["add", "wip.txt"]);
    git(&repo, &["stash"]);

    git(&repo, &["checkout", "main"]);

    // Include "feature" but exclude "wip"
    let entity_filter =
        git_tidy_core::filter::NameFilter::new(&["feature".to_string()], &["wip".to_string()]);

    let result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);
    assert_eq!(
        result.repos[0].stashes[0].branch.as_deref(),
        Some("feature-login")
    );
}

#[test]
fn scan_empty_repo_no_stashes() {
    let (dir, _repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    let result = git_stash_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        90,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Repo exists but has no stashes, so no groups reported
    assert!(result.repos.is_empty());
    assert_eq!(result.total_scanned, 0);
}
