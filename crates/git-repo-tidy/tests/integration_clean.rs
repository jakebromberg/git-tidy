mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_repo_tidy::clean::{CleanOptions, run_clean};

/// Set up an orphaned repo (no remote) for clean tests.
fn set_up_orphaned_repo(base: &std::path::Path) -> std::path::PathBuf {
    let scan_dir = base.join("projects");
    std::fs::create_dir_all(&scan_dir).unwrap();

    let repo = scan_dir.join("orphan-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@test.com"]);
    git(&repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# Test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "Initial commit"]);

    repo
}

fn default_options() -> CleanOptions {
    CleanOptions {
        dry_run: false,
        force: false,
        stale_only: false,
        orphaned_only: false,
        all: false,
    }
}

#[test]
fn clean_removes_orphaned_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_orphaned_repo(&base);
    let scan_dir = base.join("projects");

    assert!(repo.exists());

    let git_ops = RealGit;
    let scan_result = git_repo_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        180,
        &[],
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let delete_fn = |path: &std::path::Path| std::fs::remove_dir_all(path);
    let mut buf = Vec::new();

    let result = run_clean(&scan_result, &default_options(), &delete_fn, &mut buf).unwrap();

    assert_eq!(result.deleted.len(), 1);
    assert!(!repo.exists(), "repo should have been deleted");
}

#[test]
fn clean_dry_run_preserves_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_orphaned_repo(&base);
    let scan_dir = base.join("projects");

    let git_ops = RealGit;
    let scan_result = git_repo_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        180,
        &[],
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let delete_fn = |path: &std::path::Path| std::fs::remove_dir_all(path);
    let mut buf = Vec::new();
    let options = CleanOptions {
        dry_run: true,
        ..default_options()
    };

    let result = run_clean(&scan_result, &options, &delete_fn, &mut buf).unwrap();

    assert_eq!(result.deleted.len(), 1); // reported as "would delete"
    assert!(repo.exists(), "repo should still exist after dry-run");

    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("would delete"));
}

#[test]
fn clean_skips_dirty_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_orphaned_repo(&base);
    let scan_dir = base.join("projects");

    // Make repo dirty
    std::fs::write(repo.join("dirty.txt"), "uncommitted").unwrap();

    let git_ops = RealGit;
    let scan_result = git_repo_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        180,
        &[],
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let delete_fn = |path: &std::path::Path| std::fs::remove_dir_all(path);
    let mut buf = Vec::new();

    let result = run_clean(&scan_result, &default_options(), &delete_fn, &mut buf).unwrap();

    assert_eq!(result.deleted.len(), 0);
    assert!(result.dirty_blocked);
    assert!(repo.exists(), "dirty repo should not be deleted");
}

#[test]
fn clean_force_deletes_dirty_repo() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = set_up_orphaned_repo(&base);
    let scan_dir = base.join("projects");

    // Make repo dirty
    std::fs::write(repo.join("dirty.txt"), "uncommitted").unwrap();

    let git_ops = RealGit;
    let scan_result = git_repo_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        180,
        &[],
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let delete_fn = |path: &std::path::Path| std::fs::remove_dir_all(path);
    let mut buf = Vec::new();
    let options = CleanOptions {
        force: true,
        ..default_options()
    };

    let result = run_clean(&scan_result, &options, &delete_fn, &mut buf).unwrap();

    assert_eq!(result.deleted.len(), 1);
    assert!(!repo.exists(), "dirty repo should be deleted with --force");
}
