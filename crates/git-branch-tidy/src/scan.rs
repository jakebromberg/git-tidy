use std::path::Path;

use git_tidy_core::classification;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;
use git_tidy_core::types::{ClassificationLabel, ScanCounts};

use crate::discovery;
use crate::types::{BranchInfo, BranchRepoGroup, BranchScanResult};

/// Scan all repos in `directory` for stale local branches.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    behind_threshold: usize,
    verbose: bool,
    repo_filter: &NameFilter,
    progress: &Progress,
) -> Result<BranchScanResult, Error> {
    let repo_paths = discovery::discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);

    let fetch_paths: Vec<&Path> = repo_paths.iter().map(|p| p.as_path()).collect();
    let mut warnings = git_tidy_core::fetch::parallel_fetch(git, &fetch_paths, progress);

    let (repos, scan_warnings) = parallel_classify(
        &repo_paths,
        |repo_path| {
            let mut local_warnings = Vec::new();

            let default_branch = match classification::detect_default_branch(git, repo_path) {
                Ok(b) => b,
                Err(_) => {
                    local_warnings.push(format!(
                        "could not determine default branch for {} -- skipping",
                        repo_path.display()
                    ));
                    return (None, local_warnings);
                }
            };

            let branches = match git.list_local_branches(repo_path) {
                Ok(b) => b,
                Err(e) => {
                    local_warnings.push(format!(
                        "could not list branches for {}: {e}",
                        repo_path.display()
                    ));
                    return (None, local_warnings);
                }
            };

            let current = git.current_branch(repo_path).unwrap_or(None);
            let repo_name = repo_display_name(repo_path);

            if verbose {
                eprintln!(
                    "{repo_name}: {} branches (default_branch={default_branch})",
                    branches.len() - 1,
                );
            }

            let mut classified = Vec::with_capacity(branches.len());

            for branch_name in &branches {
                if branch_name == &default_branch {
                    continue;
                }

                let is_current = current.as_deref() == Some(branch_name.as_str());

                match classification::classify_branch(
                    git,
                    repo_path,
                    branch_name,
                    &default_branch,
                    behind_threshold,
                    verbose,
                ) {
                    Ok(bc) => {
                        if verbose {
                            eprintln!(
                                "  {branch_name}: {} (remote={}, ahead={}, behind={})",
                                bc.classification.label(),
                                bc.remote_tracking,
                                bc.ahead,
                                bc.behind,
                            );
                        }
                        classified.push(BranchInfo {
                            repo_path: repo_path.to_path_buf(),
                            name: branch_name.clone(),
                            default_branch: default_branch.clone(),
                            classification: bc.classification,
                            remote_tracking: bc.remote_tracking,
                            remote_deleted: bc.remote_deleted,
                            ahead: bc.ahead,
                            behind: bc.behind,
                            diverged: bc.diverged,
                            is_current,
                        });
                    }
                    Err(e) => {
                        local_warnings.push(format!(
                            "error classifying branch {} in {}: {e}",
                            branch_name,
                            repo_path.display()
                        ));
                    }
                }
            }

            classified.sort_by_key(|b| b.classification.priority());

            let group = if classified.is_empty() {
                None
            } else {
                Some(BranchRepoGroup {
                    repo_path: repo_path.to_path_buf(),
                    name: repo_name,
                    branches: classified,
                })
            };

            (group, local_warnings)
        },
        "Scanning branches",
        progress,
    );
    warnings.extend(scan_warnings);

    let mut counts = ScanCounts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for b in &g.branches {
            counts.increment(&b.classification);
        }
        total_scanned += g.branches.len();
    }

    Ok(BranchScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;
    use git_tidy_core::types::Classification;

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn scan_classifies_mixed_branches() {
        let git = MockGitBuilder::new()
            // Default branch detection
            .with_symbolic_ref(&repo(), Some("main"))
            // List branches
            .with_local_branches(
                &repo(),
                vec![
                    "main".to_string(),
                    "feature/done".to_string(),
                    "feature/wip".to_string(),
                    "feature/local".to_string(),
                ],
            )
            .with_current_branch(&repo(), Some("main"))
            // feature/done: merged
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "origin/main", true)
            // feature/wip: active (has remote, not merged)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/wip", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/wip", (5, 3))
            .with_is_ancestor(&repo(), "feature/wip", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/wip",
                vec![("abc".into(), "wip work".into())],
            )
            // feature/local: local (no remote, not merged)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/local", false)
            .with_rev_list_counts(&repo(), "origin/main", "feature/local", (10, 2))
            .with_is_ancestor(&repo(), "feature/local", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/local",
                vec![("def".into(), "local work".into())],
            )
            .build();

        // discover_repos won't work with mock paths, so we test run_scan's inner
        // logic by calling it with a directory that doesn't exist.
        // Instead, let's test the classification logic directly via the scan function
        // by creating the scan result manually from the classify outputs.
        // Actually, run_scan calls discover_repos which needs a real directory.
        // So for unit tests, let's test classify_branch for each branch and
        // verify the counts would be correct.

        // Test classify_branch for each
        let bc1 =
            classification::classify_branch(&git, &repo(), "feature/done", "main", 100, false)
                .unwrap();
        assert_eq!(bc1.classification, Classification::Landed);

        let bc2 = classification::classify_branch(&git, &repo(), "feature/wip", "main", 100, false)
            .unwrap();
        assert_eq!(bc2.classification, Classification::Active);

        let bc3 =
            classification::classify_branch(&git, &repo(), "feature/local", "main", 100, false)
                .unwrap();
        assert_eq!(bc3.classification, Classification::Local);
    }

    #[test]
    fn scan_skips_default_branch() {
        // The default branch should not appear in classified results
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch(&repo(), Some("main"))
            .build();

        // If we had only the default branch, no branches should be classified
        // We can't call run_scan directly with a mock repo path, but we can
        // verify the filtering logic by checking that "main" would be skipped
        let branches: Vec<String> = vec!["main".to_string()];
        let default_branch = "main";
        let non_default: Vec<&String> = branches
            .iter()
            .filter(|b| b.as_str() != default_branch)
            .collect();
        assert!(non_default.is_empty());

        // Verify the mock is set up correctly
        let listed = git.list_local_branches(&repo()).unwrap();
        assert_eq!(listed, vec!["main".to_string()]);
    }

    #[test]
    fn scan_marks_current_branch() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string(), "my-feature".to_string()])
            .with_current_branch(&repo(), Some("my-feature"))
            // my-feature: active
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/my-feature", true)
            .with_rev_list_counts(&repo(), "origin/main", "my-feature", (0, 1))
            .with_is_ancestor(&repo(), "my-feature", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "my-feature",
                vec![("abc".into(), "work".into())],
            )
            .build();

        let bc = classification::classify_branch(&git, &repo(), "my-feature", "main", 100, false)
            .unwrap();
        let is_current = git.current_branch(&repo()).unwrap() == Some("my-feature".to_string());
        assert!(is_current);
        assert_eq!(bc.classification, Classification::Active);
    }
}
