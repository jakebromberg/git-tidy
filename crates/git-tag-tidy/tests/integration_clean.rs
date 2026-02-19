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
fn clean_removes_local_only_tag() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Set up a bare remote
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    // Create a local-only tag
    git(&repo, &["tag", "local-wip"]);

    // Verify tag exists
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(tags.contains(&"local-wip".to_string()));

    // Scan then clean
    let scan_result = git_tag_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_tag_tidy::clean::CleanOptions {
        dry_run: false,
        yes: true,
        force: false,
        stale_only: false,
        local_only: false,
        include_remote: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_tag_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.succeeded.len(), 1);

    // Verify tag is gone
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(!tags.contains(&"local-wip".to_string()));
}

#[test]
fn clean_dry_run_preserves_tags() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Set up a bare remote
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    git(&repo, &["tag", "local-wip"]);

    let scan_result = git_tag_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_tag_tidy::clean::CleanOptions {
        dry_run: true,
        yes: true,
        force: false,
        stale_only: false,
        local_only: false,
        include_remote: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_tag_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Reports would-delete
    assert_eq!(result.succeeded.len(), 1);

    // But tag still exists
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(tags.contains(&"local-wip".to_string()));

    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("would delete"));
}

#[test]
fn clean_removes_stale_tag() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create an orphan branch, tag it, delete the branch
    git(&repo, &["checkout", "--orphan", "orphan-branch"]);
    std::fs::write(repo.join("orphan.txt"), "orphan content\n").unwrap();
    git(&repo, &["add", "orphan.txt"]);
    git(&repo, &["commit", "-m", "Orphan commit"]);
    git(&repo, &["tag", "stale-tag"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", "orphan-branch"]);

    // Verify tag exists
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(tags.contains(&"stale-tag".to_string()));

    // Scan (offline since no remote) then clean
    let scan_result = git_tag_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        true,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_tag_tidy::clean::CleanOptions {
        dry_run: false,
        yes: true,
        force: false,
        stale_only: false,
        local_only: false,
        include_remote: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_tag_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert!(result.succeeded.iter().any(|r| r.name == "stale-tag"));

    // Verify tag is gone
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(!tags.contains(&"stale-tag".to_string()));
}

#[test]
fn clean_include_remote_deletes_from_remote() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Set up a bare remote
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    // Create an orphan branch, tag it, push tag, delete branch
    git(&repo, &["checkout", "--orphan", "orphan-branch"]);
    std::fs::write(repo.join("orphan.txt"), "orphan content\n").unwrap();
    git(&repo, &["add", "orphan.txt"]);
    git(&repo, &["commit", "-m", "Orphan commit"]);
    git(&repo, &["tag", "stale-pushed"]);
    git(&repo, &["push", "origin", "stale-pushed"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", "orphan-branch"]);

    // Scan then clean with --include-remote
    let scan_result = git_tag_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_tag_tidy::clean::CleanOptions {
        dry_run: false,
        yes: true,
        force: false,
        stale_only: false,
        local_only: false,
        include_remote: true,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_tag_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert!(result.succeeded.iter().any(|r| r.name == "stale-pushed"));

    // Verify local tag is gone
    let tags = git_ops.list_local_tags(&repo).unwrap();
    assert!(!tags.contains(&"stale-pushed".to_string()));

    // Verify remote tag is also gone
    let remote_tags = git_ops.list_remote_tags(&repo, "origin").unwrap();
    let remote_tag_names: Vec<_> = remote_tags.iter().map(|(name, _)| name.as_str()).collect();
    assert!(!remote_tag_names.contains(&"stale-pushed"));
}
