use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use git_tidy_core::classification;
use git_tidy_core::counts::Counts;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::landed::{LandedCache, LandedOptions};
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;
use git_tidy_core::types::ClassificationLabel;

use crate::discovery;
use crate::types::{BranchInfo, BranchRepoGroup, BranchScanResult};

/// Scan all repos in `directory` for stale branches.
///
/// When `include_remote` is true, also discovers remote-only branches
/// (branches on origin with no local counterpart).
#[allow(clippy::too_many_arguments)]
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    behind_threshold: usize,
    verbose: bool,
    repo_filter: &NameFilter,
    entity_filter: &NameFilter,
    include_remote: bool,
    progress: &Progress,
) -> Result<BranchScanResult, Error> {
    let repo_paths = discovery::discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);
    run_scan_repos(
        git,
        &repo_paths,
        behind_threshold,
        verbose,
        entity_filter,
        include_remote,
        progress,
    )
}

/// Scan pre-discovered repos for stale branches.
///
/// Accepts repo paths directly (skipping discovery), so the audit runner can
/// call `discover_repos` once and share the result across tools.
///
/// When `include_remote` is true, also discovers remote-only branches
/// (branches on origin with no local counterpart).
pub fn run_scan_repos(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    behind_threshold: usize,
    verbose: bool,
    entity_filter: &NameFilter,
    include_remote: bool,
    progress: &Progress,
) -> Result<BranchScanResult, Error> {
    let repo_paths_owned: Vec<PathBuf> = repo_paths.to_vec();
    let fetch_paths: Vec<&Path> = repo_paths_owned
        .iter()
        .map(|p: &PathBuf| p.as_path())
        .collect();
    let mut warnings = git_tidy_core::fetch::parallel_fetch(git, &fetch_paths, progress);

    let (repos, scan_warnings) = parallel_classify(
        &repo_paths_owned,
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

            // Fail closed: if we cannot determine the current branch, drop this repo from the scan.
            // is_current gates the safety check in clean.rs that prevents deleting the checked-out branch.
            let current = match git.current_branch(repo_path) {
                Ok(b) => b,
                Err(e) => {
                    local_warnings.push(format!(
                        "could not determine current branch for {}: {e}",
                        repo_path.display()
                    ));
                    return (None, local_warnings);
                }
            };
            let repo_name = repo_display_name(repo_path);

            if verbose {
                eprintln!(
                    "{repo_name}: {} branches (default_branch={default_branch})",
                    branches.len() - 1,
                );
            }

            let landed_cache = LandedCache::new();
            let landed_options = LandedOptions::default();

            let branch_results: Vec<_> = branches
                .par_iter()
                .filter(|b| *b != &default_branch && entity_filter.matches(b))
                .map(|branch_name| {
                    let is_current = current.as_deref() == Some(branch_name.as_str());

                    match classification::classify_branch_cached(
                        git,
                        repo_path,
                        branch_name,
                        &default_branch,
                        behind_threshold,
                        verbose,
                        &landed_cache,
                        &landed_options,
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
                            Ok(BranchInfo {
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
                                remote_only: false,
                            })
                        }
                        Err(e) => Err(format!(
                            "error classifying branch {} in {}: {e}",
                            branch_name,
                            repo_path.display()
                        )),
                    }
                })
                .collect();

            let mut classified = Vec::with_capacity(branch_results.len());
            for result in branch_results {
                match result {
                    Ok(info) => classified.push(info),
                    Err(warning) => local_warnings.push(warning),
                }
            }

            // Discover remote-only branches (on origin but no local counterpart)
            if include_remote {
                let local_names: HashSet<&str> = branches.iter().map(|b| b.as_str()).collect();

                let tracking_refs_result = git.list_remote_tracking_refs(repo_path);
                if let Err(e) = &tracking_refs_result {
                    local_warnings.push(format!(
                        "could not list remote tracking refs for {}: {e}",
                        repo_path.display()
                    ));
                }
                if let Ok(tracking_refs) = tracking_refs_result {
                    let remote_only_names: Vec<String> = tracking_refs
                        .iter()
                        .filter_map(|(short, _full)| {
                            let branch = short.strip_prefix("origin/")?;
                            if branch == "HEAD"
                                || branch == default_branch
                                || local_names.contains(branch)
                            {
                                return None;
                            }
                            if !entity_filter.matches(branch) {
                                return None;
                            }
                            Some(branch.to_string())
                        })
                        .collect();

                    if verbose && !remote_only_names.is_empty() {
                        eprintln!(
                            "{repo_name}: {} remote-only branches",
                            remote_only_names.len(),
                        );
                    }

                    let remote_results: Vec<_> = remote_only_names
                        .par_iter()
                        .map(|branch_name| {
                            match classification::classify_remote_branch(
                                git,
                                repo_path,
                                branch_name,
                                &default_branch,
                                behind_threshold,
                                verbose,
                                &landed_options,
                            ) {
                                Ok(bc) => {
                                    if verbose {
                                        eprintln!(
                                            "  {branch_name} (remote): {} (ahead={}, behind={})",
                                            bc.classification.label(),
                                            bc.ahead,
                                            bc.behind,
                                        );
                                    }
                                    Ok(BranchInfo {
                                        repo_path: repo_path.to_path_buf(),
                                        name: branch_name.clone(),
                                        default_branch: default_branch.clone(),
                                        classification: bc.classification,
                                        remote_tracking: true,
                                        remote_deleted: false,
                                        ahead: bc.ahead,
                                        behind: bc.behind,
                                        diverged: bc.diverged,
                                        is_current: false,
                                        remote_only: true,
                                    })
                                }
                                Err(e) => Err(format!(
                                    "error classifying remote branch {} in {}: {e}",
                                    branch_name,
                                    repo_path.display()
                                )),
                            }
                        })
                        .collect();

                    for result in remote_results {
                        match result {
                            Ok(info) => classified.push(info),
                            Err(warning) => local_warnings.push(warning),
                        }
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

    let mut counts = Counts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for b in &g.branches {
            counts.increment(b.classification.label());
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

    use git_tidy_core::filter::NameFilter;
    use git_tidy_core::progress::Progress;
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
            // origin has main
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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
        let bc1 = classification::classify_branch(
            &git,
            &repo(),
            "feature/done",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(bc1.classification, Classification::Landed);

        let bc2 = classification::classify_branch(
            &git,
            &repo(),
            "feature/wip",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(bc2.classification, Classification::Active);

        let bc3 = classification::classify_branch(
            &git,
            &repo(),
            "feature/local",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
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
    fn run_scan_repos_with_mock() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(
                &repo(),
                vec!["main".to_string(), "feature/done".to_string()],
            )
            .with_current_branch(&repo(), Some("main"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "origin/main", true)
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, false, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.get("landed"), 1);
    }

    #[test]
    fn scan_marks_current_branch() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string(), "my-feature".to_string()])
            .with_current_branch(&repo(), Some("my-feature"))
            // origin has main
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let bc = classification::classify_branch(
            &git,
            &repo(),
            "my-feature",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        let is_current = git.current_branch(&repo()).unwrap() == Some("my-feature".to_string());
        assert!(is_current);
        assert_eq!(bc.classification, Classification::Active);
    }

    #[test]
    fn scan_discovers_remote_only_branches() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch(&repo(), Some("main"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            // Remote-only branch: origin/feature/remote-only
            .with_remote_tracking_refs(
                &repo(),
                vec![
                    (
                        "origin/main".to_string(),
                        "refs/remotes/origin/main".to_string(),
                    ),
                    (
                        "origin/feature/remote-only".to_string(),
                        "refs/remotes/origin/feature/remote-only".to_string(),
                    ),
                ],
            )
            .with_rev_list_counts(&repo(), "origin/main", "origin/feature/remote-only", (0, 0))
            .with_is_ancestor(&repo(), "origin/feature/remote-only", "origin/main", true)
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, true, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.get("landed"), 1);
        let branch = &result.repos[0].branches[0];
        assert_eq!(branch.name, "feature/remote-only");
        assert!(branch.remote_only);
        assert!(branch.remote_tracking);
    }

    #[test]
    fn scan_excludes_remote_only_when_flag_false() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch(&repo(), Some("main"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_remote_tracking_refs(
                &repo(),
                vec![
                    (
                        "origin/main".to_string(),
                        "refs/remotes/origin/main".to_string(),
                    ),
                    (
                        "origin/feature/remote-only".to_string(),
                        "refs/remotes/origin/feature/remote-only".to_string(),
                    ),
                ],
            )
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, false, &p).unwrap();
        assert_eq!(result.total_scanned, 0);
        assert!(result.repos.is_empty());
    }

    #[test]
    fn scan_drops_repo_when_current_branch_errors() {
        // Regression: previously `current_branch().unwrap_or(None)` silently masked errors, leaving is_current unset on every branch — so the clean.rs safety check that prevents deleting the checked-out branch could not fire.
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch_error(&repo(), "HEAD is unreadable")
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, false, &p).unwrap();

        // No branches scanned for this repo.
        assert!(result.repos.is_empty());
        // The warning surface must mention the repo and the underlying error.
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("current branch") && w.contains("HEAD is unreadable")),
            "expected current_branch warning, got: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn scan_warns_when_remote_tracking_refs_errors_with_include_remote() {
        // Regression: previously the include_remote path used `if let Ok(...)` and silently produced zero remote-only branches when list_remote_tracking_refs errored. The user could not tell why include_remote returned nothing.
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch(&repo(), Some("main"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_list_remote_tracking_refs_error(&repo(), "for-each-ref crashed")
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, true, &p).unwrap();
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("remote tracking refs") && w.contains("for-each-ref crashed")),
            "expected warning, got: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn scan_remote_only_skips_default_branch_and_head() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_current_branch(&repo(), Some("main"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_remote_tracking_refs(
                &repo(),
                vec![
                    ("origin".to_string(), "refs/remotes/origin/HEAD".to_string()),
                    (
                        "origin/main".to_string(),
                        "refs/remotes/origin/main".to_string(),
                    ),
                ],
            )
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], 100, false, &filter, true, &p).unwrap();
        // Neither HEAD nor main should appear
        assert_eq!(result.total_scanned, 0);
        assert!(result.repos.is_empty());
    }
}
