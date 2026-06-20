mod common;

use common::{git, set_up_repo};
use git_tidy_core::git::{GitOps, RealGit};

#[test]
fn clean_removes_remote_in_real_repo() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Add a remote pointing to a nonexistent path (will be unreachable)
    git(
        &repo,
        &["remote", "add", "stale", "/nonexistent/path/repo.git"],
    );

    // Verify remote exists
    let remotes = git_ops.list_remotes(&repo).unwrap();
    assert!(remotes.contains(&"stale".to_string()));

    // Scan then clean
    let scan_result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_remote_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_remote_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.succeeded.len(), 1);

    // Verify remote is gone
    let remotes = git_ops.list_remotes(&repo).unwrap();
    assert!(!remotes.contains(&"stale".to_string()));
}

#[test]
fn clean_dry_run_does_not_remove() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    git(
        &repo,
        &["remote", "add", "stale", "/nonexistent/path/repo.git"],
    );

    let scan_result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_remote_tidy::clean::CleanOptions {
        dry_run: true,
        force: false,
        all: false,
    };

    let mut buf = Vec::new();
    let result =
        git_remote_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    // Reports it would remove
    assert_eq!(result.succeeded.len(), 1);

    // But remote still exists
    let remotes = git_ops.list_remotes(&repo).unwrap();
    assert!(remotes.contains(&"stale".to_string()));

    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("would remove"));
}

#[test]
fn clean_prunes_orphaned_refs() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Manually create orphaned refs
    let head_hash = git(&repo, &["rev-parse", "HEAD"]);
    git(
        &repo,
        &["update-ref", "refs/remotes/stale/main", &head_hash],
    );
    git(
        &repo,
        &["update-ref", "refs/remotes/stale/feature", &head_hash],
    );

    // Verify refs exist
    let refs = git_ops.list_remote_tracking_refs(&repo).unwrap();
    let stale_refs: Vec<_> = refs
        .iter()
        .filter(|(s, _)| s.starts_with("stale/"))
        .collect();
    assert_eq!(stale_refs.len(), 2);

    // Scan then clean with --all
    let scan_result = git_remote_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        false,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    let options = git_remote_tidy::clean::CleanOptions {
        dry_run: false,
        force: false,
        all: true,
    };

    let mut buf = Vec::new();
    let result =
        git_remote_tidy::clean::run_clean(&git_ops, &scan_result, &options, &mut buf).unwrap();

    assert_eq!(result.succeeded.len(), 1);
    assert!(result.succeeded[0].refs_pruned > 0);

    // Verify refs are gone
    let refs = git_ops.list_remote_tracking_refs(&repo).unwrap();
    let stale_refs: Vec<_> = refs
        .iter()
        .filter(|(s, _)| s.starts_with("stale/"))
        .collect();
    assert!(stale_refs.is_empty());
}
