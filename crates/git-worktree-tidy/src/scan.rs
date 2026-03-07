use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use git_tidy_core::classification;
use git_tidy_core::discovery;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;
use git_tidy_core::types::{ClassificationLabel, RepoGroup, ScanCounts, ScanResult};

use crate::discovery::{self as wt_discovery, DiscoveredWorktree};

/// Filter discovered worktrees by basename using a `NameFilter`.
///
/// Retains only worktrees whose directory basename passes the filter.
/// Repos with no remaining worktrees are dropped entirely.
/// A default (empty) filter returns all worktrees unchanged.
fn filter_worktrees(
    groups: BTreeMap<PathBuf, Vec<DiscoveredWorktree>>,
    filter: &NameFilter,
) -> BTreeMap<PathBuf, Vec<DiscoveredWorktree>> {
    if filter.is_empty() {
        return groups;
    }

    groups
        .into_iter()
        .filter_map(|(repo, worktrees)| {
            let matched: Vec<DiscoveredWorktree> = worktrees
                .into_iter()
                .filter(|wt| {
                    let basename = wt.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    filter.matches(basename)
                })
                .collect();

            if matched.is_empty() {
                None
            } else {
                Some((repo, matched))
            }
        })
        .collect()
}

/// Scan all worktrees under `directory` and classify them.
///
/// Discovers repos, then queries each for linked worktrees via
/// `git worktree list`. Applies entity and repo filters, then delegates
/// to [`run_scan_repos`] for fetching and classification.
#[allow(clippy::too_many_arguments)]
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
    entity_filter: &NameFilter,
    repo_filter: &NameFilter,
    progress: &Progress,
) -> Result<ScanResult, Error> {
    let repo_paths = discovery::discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);
    let groups = wt_discovery::discover_worktrees(git, &repo_paths);
    let groups = filter_worktrees(groups, entity_filter);

    run_scan_repos(
        git,
        groups,
        behind_threshold,
        verbose,
        noise_patterns,
        progress,
    )
}

/// Classify pre-discovered worktree groups.
///
/// Accepts a `BTreeMap` of parent-repo to worktrees (as returned by
/// [`discovery::discover_worktrees`] and optional filtering), fetches each
/// repo, classifies every worktree, and returns a [`ScanResult`].
pub fn run_scan_repos(
    git: &dyn GitOps,
    groups: BTreeMap<PathBuf, Vec<DiscoveredWorktree>>,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
    progress: &Progress,
) -> Result<ScanResult, Error> {
    let repo_paths: Vec<PathBuf> = groups.keys().cloned().collect();
    let fetch_paths: Vec<&Path> = repo_paths.iter().map(|p| p.as_path()).collect();
    let mut warnings = git_tidy_core::fetch::parallel_fetch(git, &fetch_paths, progress);

    let (repos, scan_warnings) = parallel_classify(
        &repo_paths,
        |repo_path| {
            let mut local_warnings = Vec::new();

            let worktrees = match groups.get(repo_path) {
                Some(wts) => wts,
                None => return (None, vec![]),
            };

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

            let repo_name = repo_display_name(repo_path);

            if verbose {
                eprintln!(
                    "{repo_name}: {} worktrees (default_branch={default_branch})",
                    worktrees.len(),
                );
            }

            let mut classified = Vec::new();
            for wt in worktrees {
                match classification::classify_worktree(
                    git,
                    &wt.path,
                    repo_path,
                    &default_branch,
                    behind_threshold,
                    verbose,
                    noise_patterns,
                ) {
                    Ok(info) => {
                        if verbose {
                            let wt_name =
                                wt.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                            eprintln!(
                                "  {wt_name}: {} (branch={}, ahead={}, behind={})",
                                info.classification.label(),
                                info.branch.as_deref().unwrap_or("(detached)"),
                                info.ahead,
                                info.behind,
                            );
                        }
                        classified.push(info);
                    }
                    Err(e) => {
                        local_warnings
                            .push(format!("error classifying {}: {e}", wt.path.display()));
                    }
                }
            }

            classified.sort_by_key(|wt| wt.classification.priority());

            let group = if classified.is_empty() {
                None
            } else {
                Some(RepoGroup {
                    repo_path: repo_path.to_path_buf(),
                    name: repo_name,
                    worktrees: classified,
                })
            };

            (group, local_warnings)
        },
        "Scanning worktrees",
        progress,
    );
    warnings.extend(scan_warnings);

    let mut counts = ScanCounts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for wt in &g.worktrees {
            counts.increment(&wt.classification);
        }
        total_scanned += g.worktrees.len();
    }

    Ok(ScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use git_tidy_core::progress::Progress;
    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;
    use crate::discovery::DiscoveredWorktree;

    fn make_worktree(name: &str, repo: &str) -> DiscoveredWorktree {
        DiscoveredWorktree {
            path: PathBuf::from(format!("/dev/{name}")),
            parent_repo: PathBuf::from(format!("/dev/{repo}")),
        }
    }

    fn make_groups(
        entries: &[(&str, Vec<DiscoveredWorktree>)],
    ) -> BTreeMap<PathBuf, Vec<DiscoveredWorktree>> {
        entries
            .iter()
            .map(|(repo, wts)| (PathBuf::from(repo), wts.clone()))
            .collect()
    }

    #[test]
    fn filter_worktrees_substring_match() {
        let groups = make_groups(&[(
            "/dev/MyRepo",
            vec![
                make_worktree("MyRepo-feature", "MyRepo"),
                make_worktree("MyRepo-bugfix", "MyRepo"),
                make_worktree("OtherProject-thing", "MyRepo"),
            ],
        )]);

        let filter = NameFilter::new(&["MyRepo".to_string()], &[]);
        let filtered = filter_worktrees(groups, &filter);
        let wts = &filtered[&PathBuf::from("/dev/MyRepo")];
        assert_eq!(wts.len(), 2);
        assert!(wts.iter().all(|w| {
            let name = w.path.file_name().unwrap().to_str().unwrap();
            name.contains("MyRepo")
        }));
    }

    #[test]
    fn filter_worktrees_removes_empty_repos() {
        let groups = make_groups(&[
            ("/dev/RepoA", vec![make_worktree("RepoA-feat", "RepoA")]),
            ("/dev/RepoB", vec![make_worktree("RepoB-fix", "RepoB")]),
        ]);

        let filter = NameFilter::new(&["RepoA".to_string()], &[]);
        let filtered = filter_worktrees(groups, &filter);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key(&PathBuf::from("/dev/RepoA")));
        assert!(!filtered.contains_key(&PathBuf::from("/dev/RepoB")));
    }

    #[test]
    fn filter_worktrees_case_insensitive() {
        let groups = make_groups(&[(
            "/dev/MyRepo",
            vec![
                make_worktree("myrepo-feature", "MyRepo"),
                make_worktree("MYREPO-bugfix", "MyRepo"),
            ],
        )]);

        let filter = NameFilter::new(&["MyRepo".to_string()], &[]);
        let filtered = filter_worktrees(groups, &filter);
        let wts = &filtered[&PathBuf::from("/dev/MyRepo")];
        assert_eq!(wts.len(), 2);
    }

    #[test]
    fn filter_worktrees_multiple_patterns_or() {
        let groups = make_groups(&[(
            "/dev/MyRepo",
            vec![
                make_worktree("alpha-feature", "MyRepo"),
                make_worktree("beta-bugfix", "MyRepo"),
                make_worktree("gamma-thing", "MyRepo"),
            ],
        )]);

        let filter = NameFilter::new(&["alpha".to_string(), "gamma".to_string()], &[]);
        let filtered = filter_worktrees(groups, &filter);
        let wts = &filtered[&PathBuf::from("/dev/MyRepo")];
        assert_eq!(wts.len(), 2);
    }

    #[test]
    fn filter_worktrees_empty_filter_passes_all() {
        let groups = make_groups(&[(
            "/dev/MyRepo",
            vec![
                make_worktree("MyRepo-feature", "MyRepo"),
                make_worktree("MyRepo-bugfix", "MyRepo"),
            ],
        )]);

        let filter = NameFilter::default();
        let filtered = filter_worktrees(groups.clone(), &filter);
        assert_eq!(filtered, groups);
    }

    #[test]
    fn filter_worktrees_exclude_takes_precedence() {
        let groups = make_groups(&[(
            "/dev/MyRepo",
            vec![
                make_worktree("feat-login", "MyRepo"),
                make_worktree("feat-wip-draft", "MyRepo"),
                make_worktree("bugfix-urgent", "MyRepo"),
            ],
        )]);

        let filter = NameFilter::new(&["feat".to_string()], &["wip".to_string()]);
        let filtered = filter_worktrees(groups, &filter);
        let wts = &filtered[&PathBuf::from("/dev/MyRepo")];
        assert_eq!(wts.len(), 1);
        assert_eq!(
            wts[0].path.file_name().unwrap().to_str().unwrap(),
            "feat-login"
        );
    }

    #[test]
    fn run_scan_repos_empty_groups() {
        let groups: BTreeMap<PathBuf, Vec<DiscoveredWorktree>> = BTreeMap::new();
        let git = MockGitBuilder::new().build();
        let progress = Progress::disabled();
        let result = run_scan_repos(&git, groups, 100, false, &[], &progress).unwrap();
        assert_eq!(result.total_scanned, 0);
        assert!(result.repos.is_empty());
        assert!(result.warnings.is_empty());
    }
}
