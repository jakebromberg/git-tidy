mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_remote_tidy::types::RemoteClassification;

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
fn scan_real_repo_with_reachable_remote() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create a bare remote and push to it
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);

    let remote = &result.repos[0].remotes[0];
    assert_eq!(remote.name, "origin");
    assert_eq!(remote.classification, RemoteClassification::Active);
    assert!(remote.is_origin);
    assert!(remote.tracking_count > 0);
}

#[test]
fn scan_repo_with_unreachable_remote() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Add a remote pointing to a nonexistent path
    git(
        &repo,
        &["remote", "add", "origin", "/nonexistent/path/repo.git"],
    );

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    assert_eq!(result.total_scanned, 1);

    let remote = &result.repos[0].remotes[0];
    assert_eq!(remote.name, "origin");
    assert_eq!(remote.classification, RemoteClassification::Unreachable);
}

#[test]
fn scan_repo_with_orphaned_refs() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Manually create a ref under refs/remotes/stale/ (no config for "stale")
    let head_hash = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["update-ref", "refs/remotes/stale/main", &head_hash],
    );

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);

    // Should detect the orphaned remote
    let orphaned: Vec<_> = result.repos[0]
        .remotes
        .iter()
        .filter(|r| r.classification == RemoteClassification::Orphaned)
        .collect();
    assert_eq!(orphaned.len(), 1);
    assert_eq!(orphaned[0].name, "stale");
    assert!(orphaned[0].url.is_none());
    assert_eq!(orphaned[0].tracking_count, 1);
}

#[test]
fn scan_entity_filter_includes_matching_remotes() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Add multiple remotes (all offline/unreachable for simplicity)
    git(
        &repo,
        &["remote", "add", "origin", "/nonexistent/origin.git"],
    );
    git(
        &repo,
        &["remote", "add", "upstream", "/nonexistent/upstream.git"],
    );
    git(&repo, &["remote", "add", "fork", "/nonexistent/fork.git"]);

    // Filter to only "origin"
    let entity_filter = git_tidy_core::filter::NameFilter::new(&["origin".to_string()], &[]);

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        true, // offline to avoid network
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let names: Vec<&str> = result.repos[0]
        .remotes
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(names.contains(&"origin"));
    assert!(!names.contains(&"upstream"));
    assert!(!names.contains(&"fork"));
    assert_eq!(result.total_scanned, 1);
}

#[test]
fn scan_entity_filter_exclude_takes_precedence() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    git(
        &repo,
        &["remote", "add", "origin", "/nonexistent/origin.git"],
    );
    git(
        &repo,
        &["remote", "add", "origin-backup", "/nonexistent/backup.git"],
    );
    git(&repo, &["remote", "add", "fork", "/nonexistent/fork.git"]);

    // Include "origin" but exclude "backup"
    let entity_filter =
        git_tidy_core::filter::NameFilter::new(&["origin".to_string()], &["backup".to_string()]);

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        true,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &entity_filter,
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);
    let names: Vec<&str> = result.repos[0]
        .remotes
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(names.contains(&"origin"));
    assert!(!names.contains(&"origin-backup"));
    assert!(!names.contains(&"fork"));
    assert_eq!(result.total_scanned, 1);
}

#[test]
fn scan_empty_repo_no_remotes() {
    let (dir, _repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Repo has no remotes, so no groups
    assert!(result.repos.is_empty());
    assert_eq!(result.total_scanned, 0);
}

#[test]
fn scan_offline_mode() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Add a remote pointing to a nonexistent path -- offline mode should skip reachability
    git(
        &repo,
        &["remote", "add", "origin", "/nonexistent/path/repo.git"],
    );

    let result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        true,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    assert_eq!(result.repos.len(), 1, "warnings: {:?}", result.warnings);

    let remote = &result.repos[0].remotes[0];
    assert_eq!(remote.name, "origin");
    // In offline mode, configured remotes default to Active
    assert_eq!(remote.classification, RemoteClassification::Active);
}
