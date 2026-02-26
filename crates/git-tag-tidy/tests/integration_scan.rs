mod common;

use common::git;
use git_tidy_core::git::RealGit;

use git_tag_tidy::types::TagClassification;

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
fn scan_synced_tag() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Set up a bare remote and push
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    // Create a tag and push it
    git(&repo, &["tag", "v1.0.0"]);
    git(&repo, &["push", "origin", "v1.0.0"]);

    let result = git_tag_tidy::scan::run_scan(
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
    assert!(result.total_scanned >= 1);

    let synced: Vec<_> = result.repos[0]
        .tags
        .iter()
        .filter(|t| t.name == "v1.0.0")
        .collect();
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].classification, TagClassification::Synced);
    assert!(synced[0].is_release_tag);
    assert!(!synced[0].commit.is_empty());
}

#[test]
fn scan_local_only_tag() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Set up a bare remote
    let bare = dir.path().canonicalize().unwrap().join("remote.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-u", "origin", "main"]);

    // Create a tag but don't push it
    git(&repo, &["tag", "local-only-tag"]);

    let result = git_tag_tidy::scan::run_scan(
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

    let local: Vec<_> = result.repos[0]
        .tags
        .iter()
        .filter(|t| t.name == "local-only-tag")
        .collect();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].classification, TagClassification::LocalOnly);
    assert!(!local[0].is_release_tag);
}

#[test]
fn scan_stale_tag() {
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

    let result = git_tag_tidy::scan::run_scan(
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

    let stale: Vec<_> = result.repos[0]
        .tags
        .iter()
        .filter(|t| t.name == "stale-tag")
        .collect();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].classification, TagClassification::Stale);
}

#[test]
fn scan_annotated_tag() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create an annotated tag
    git(&repo, &["tag", "-a", "v2.0.0", "-m", "Release 2.0.0"]);

    let result = git_tag_tidy::scan::run_scan(
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

    let annotated: Vec<_> = result.repos[0]
        .tags
        .iter()
        .filter(|t| t.name == "v2.0.0")
        .collect();
    assert_eq!(annotated.len(), 1);
    assert!(annotated[0].is_annotated);
    assert!(annotated[0].tagger_date.is_some());
    assert!(annotated[0].is_release_tag);
}

#[test]
fn scan_entity_filter_includes_matching_tags() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    // Create multiple tags
    git(&repo, &["tag", "v1.0.0"]);
    git(&repo, &["tag", "v2.0.0"]);
    git(&repo, &["tag", "experiment-alpha"]);

    // Filter to only v1 tags
    let entity_filter = git_tidy_core::filter::NameFilter::new(&["v1".to_string()], &[]);

    let result = git_tag_tidy::scan::run_scan(
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
        .tags
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"v1.0.0"));
    assert!(!names.contains(&"v2.0.0"));
    assert!(!names.contains(&"experiment-alpha"));
    assert_eq!(result.total_scanned, 1);
}

#[test]
fn scan_entity_filter_exclude_takes_precedence() {
    let (dir, repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    git(&repo, &["tag", "v1.0.0"]);
    git(&repo, &["tag", "v1.0.0-rc1"]);
    git(&repo, &["tag", "experiment"]);

    // Include "v1" but exclude "rc"
    let entity_filter =
        git_tidy_core::filter::NameFilter::new(&["v1".to_string()], &["rc".to_string()]);

    let result = git_tag_tidy::scan::run_scan(
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
        .tags
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"v1.0.0"));
    assert!(!names.contains(&"v1.0.0-rc1"));
    assert!(!names.contains(&"experiment"));
    assert_eq!(result.total_scanned, 1);
}

#[test]
fn scan_empty_repo_no_tags() {
    let (dir, _repo) = set_up_repo();
    let scan_dir = dir.path().canonicalize().unwrap().join("projects");
    let git_ops = RealGit;

    let result = git_tag_tidy::scan::run_scan(
        &git_ops,
        &scan_dir,
        true,
        false,
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::filter::NameFilter::default(),
        &git_tidy_core::progress::Progress::disabled(),
    )
    .unwrap();

    // Repo has no tags, so no groups
    assert!(result.repos.is_empty());
    assert_eq!(result.total_scanned, 0);
}
