use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use git_tidy_core::classification;
use git_tidy_core::error::Error;
use git_tidy_core::filter::NameFilter;
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::types::{ClassificationLabel, RepoGroup, ScanCounts, ScanResult};

use crate::discovery::{self, DiscoveredWorktree};

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
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
    entity_filter: &NameFilter,
    progress: &Progress,
) -> Result<ScanResult, Error> {
    let groups = discovery::discover_worktrees(directory)?;
    let groups = filter_worktrees(groups, entity_filter);

    let repo_paths: Vec<&std::path::Path> = groups.keys().map(|p| p.as_path()).collect();
    let mut warnings = git_tidy_core::fetch::parallel_fetch(git, &repo_paths, progress);

    let mut repos = Vec::new();
    let mut counts = ScanCounts::default();
    let mut total_scanned = 0;

    let pb = progress.bar(groups.len() as u64, "Scanning worktrees");
    for (repo_path, worktrees) in &groups {
        // Detect default branch
        let default_branch = match classification::detect_default_branch(git, repo_path) {
            Ok(b) => b,
            Err(_) => {
                warnings.push(format!(
                    "could not determine default branch for {} -- skipping",
                    repo_path.display()
                ));
                continue;
            }
        };

        let repo_name = repo_display_name(repo_path);

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
                    counts.increment(&info.classification);
                    total_scanned += 1;
                    classified.push(info);
                }
                Err(e) => {
                    warnings.push(format!("error classifying {}: {e}", wt.path.display()));
                }
            }
        }

        // Sort by classification priority
        classified.sort_by_key(|wt| wt.classification.priority());

        if !classified.is_empty() {
            repos.push(RepoGroup {
                repo_path: repo_path.clone(),
                name: repo_name,
                worktrees: classified,
            });
        }
        pb.inc(1);
    }
    pb.finish_and_clear();

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
}
