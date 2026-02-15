mod common;

use common::{git, TestRepo};
use git_worktree_tidy::git::RealGit;
use git_worktree_tidy::scan;

/// Full scan pipeline integration test: discover worktrees, classify them,
/// and verify the output structure against a real git repo.
#[test]
fn scan_real_repo_with_merged_and_active_worktrees() {
    let test = TestRepo::new();

    // Set up a bare remote so detect_default_branch can find origin/main
    let bare_path = test.dir.path().join("bare-origin.git");
    git(&test.main_repo, &["clone", "--bare", ".", &bare_path.to_string_lossy()]);
    git(&test.main_repo, &["remote", "add", "origin", &bare_path.to_string_lossy()]);
    git(&test.main_repo, &["fetch", "origin"]);

    // Create a worktree with a branch that is already merged (ancestor of main)
    let _wt_merged = test.add_worktree("main-repo-merged", "feature/already-merged");
    // Don't add any commits — the branch tip IS main, so it's an ancestor.

    // Create a worktree with an extra commit (not merged)
    let wt_active = test.add_worktree("main-repo-active", "feature/active-work");
    test.commit_file(
        &wt_active,
        "new-file.txt",
        "some work",
        "Add new file for active feature",
    );

    let git_ops = RealGit;
    let result = scan::run_scan(&git_ops, test.dir.path(), 100, false).unwrap();

    assert_eq!(result.total_scanned, 2);
    assert_eq!(result.repos.len(), 1);

    let group = &result.repos[0];
    assert_eq!(group.worktrees.len(), 2);

    // Worktrees should be sorted by classification priority (merged first)
    let first = &group.worktrees[0];
    let second = &group.worktrees[1];

    // The merged worktree (no extra commits) should be classified as merged
    assert_eq!(first.classification.label(), "merged");
    assert!(first.path.ends_with("main-repo-merged"));

    // The active worktree has local-only commits with no remote tracking,
    // so it should be classified as "local"
    assert_eq!(second.classification.label(), "local");
    assert!(second.path.ends_with("main-repo-active"));

    // Verify counts
    assert_eq!(result.counts.merged, 1);
    assert_eq!(result.counts.local, 1);
    assert_eq!(result.counts.active, 0);

    // Verify human-readable output can be generated without errors
    let mut buf = Vec::new();
    git_worktree_tidy::output::write_human(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("merged"));
    assert!(output.contains("2 worktrees scanned"));

    // Verify JSON output is valid
    let mut json_buf = Vec::new();
    git_worktree_tidy::output::write_json(&mut json_buf, &result).unwrap();
    let json_str = String::from_utf8(json_buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 2);
}

/// Scan with no linked worktrees produces empty results.
#[test]
fn scan_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let git = RealGit;
    let result = scan::run_scan(&git, dir.path(), 100, false).unwrap();

    assert_eq!(result.total_scanned, 0);
    assert!(result.repos.is_empty());
}
