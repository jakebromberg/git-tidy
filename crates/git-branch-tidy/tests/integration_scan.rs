mod common;

use common::git;
use git_tidy_core::git::RealGit;
use git_tidy_core::types::Classification;

/// Set up a repo with a remote so detect_default_branch works.
/// Returns (scan_dir, repo_path) where scan_dir is the directory to pass to run_scan.
fn set_up_repo_with_remote() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    // Create a bare "remote" repo
    let bare = base.join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);

    // Create the scan directory that holds the working repo
    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    // Create the working repo inside scan_dir
    let repo = scan_dir.join("my-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    // Add remote
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);

    // Initial commit
    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    // Push main to remote so origin/main exists
    git(&repo, &["push", "-u", "origin", "main"]);

    (dir, repo)
}

#[test]
fn scan_real_repo_with_mixed_branches() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a merged branch (at same commit as main)
    git(&repo, &["branch", "feature/done"]);

    // Create an unmerged branch with unique work
    git(&repo, &["checkout", "-b", "feature/wip"]);
    std::fs::write(repo.join("wip.txt"), "work in progress").unwrap();
    git(&repo, &["add", "wip.txt"]);
    git(&repo, &["commit", "-m", "wip commit"]);
    git(&repo, &["checkout", "main"]);

    let result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Should find our repo
    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let repo_group = &result.repos[0];

    // Default branch (main) should be excluded, so we get feature/done and feature/wip
    assert_eq!(repo_group.branches.len(), 2);

    // feature/done should be merged (it's at same commit as main)
    let done = repo_group
        .branches
        .iter()
        .find(|b| b.name == "feature/done")
        .expect("should find feature/done");
    assert_eq!(done.classification, Classification::Landed);

    // feature/wip should be local (no remote tracking, has unique commits)
    let wip = repo_group
        .branches
        .iter()
        .find(|b| b.name == "feature/wip")
        .expect("should find feature/wip");
    assert_eq!(wip.classification, Classification::Local);

    // Sorted by priority: merged first, then local
    assert_eq!(repo_group.branches[0].name, "feature/done");
    assert_eq!(repo_group.branches[1].name, "feature/wip");

    // Counts
    assert_eq!(result.total_scanned, 2);
    assert_eq!(result.counts.landed, 1);
    assert_eq!(result.counts.local, 1);
}

#[test]
fn scan_marks_current_branch_in_real_repo() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a branch and check it out
    git(&repo, &["checkout", "-b", "my-feature"]);
    std::fs::write(repo.join("f.txt"), "content").unwrap();
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-m", "commit on feature"]);
    // Don't switch back -- my-feature is the current branch

    let result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let branch = &result.repos[0].branches[0];
    assert_eq!(branch.name, "my-feature");
    assert!(branch.is_current);
}

#[test]
fn scan_entity_filter_includes_matching_branches() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create several branches
    git(&repo, &["branch", "feature/login"]);
    git(&repo, &["branch", "feature/signup"]);
    git(&repo, &["branch", "bugfix/crash"]);

    // Filter to only feature branches
    let entity_filter = git_tidy_core::filter::NameFilter::new(&["feature".to_string()], &[]);

    let result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let names: Vec<&str> = result.repos[0]
        .branches
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert!(names.contains(&"feature/login"));
    assert!(names.contains(&"feature/signup"));
    assert!(!names.contains(&"bugfix/crash"));
    assert_eq!(result.total_scanned, 2);
}

#[test]
fn scan_entity_filter_exclude_takes_precedence() {
    let (dir, repo) = set_up_repo_with_remote();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    git(&repo, &["branch", "feature/login"]);
    git(&repo, &["branch", "feature/login-wip"]);
    git(&repo, &["branch", "bugfix/crash"]);

    // Include "feature" but exclude "wip"
    let entity_filter =
        git_tidy_core::filter::NameFilter::new(&["feature".to_string()], &["wip".to_string()]);

    let result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let names: Vec<&str> = result.repos[0]
        .branches
        .iter()
        .map(|b| b.name.as_str())
        .collect();
    assert!(names.contains(&"feature/login"));
    assert!(!names.contains(&"feature/login-wip"));
    assert!(!names.contains(&"bugfix/crash"));
    assert_eq!(result.total_scanned, 1);
}

#[test]
fn scan_skips_repo_without_default_branch() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    // Create a bare-bones repo with no remote (so no default branch detected)
    let repo = base.join("no-default");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "develop"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("f.txt"), "hello").unwrap();
    git(&repo, &["add", "f.txt"]);
    git(&repo, &["commit", "-m", "init"]);

    let git_ops = RealGit;
    let result = git_branch_tidy::scan::run_scan(
        &git_ops,
        &base,
        100,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Repo should be skipped with a warning
    assert!(result.repos.is_empty());
    assert!(!result.warnings.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.contains("could not determine default branch"))
    );
}
