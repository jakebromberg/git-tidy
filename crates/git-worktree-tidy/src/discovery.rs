use std::collections::BTreeMap;
use std::path::PathBuf;

use git_tidy_core::git::GitOps;

/// A discovered linked worktree before classification.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredWorktree {
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
    /// Absolute path to the parent (main) repo.
    #[allow(dead_code)]
    pub parent_repo: PathBuf,
}

/// Discover all linked worktrees for the given repos via `git worktree list`.
///
/// For each repo, calls `git.worktree_list()` to find linked worktrees.
/// Returns worktrees grouped by parent repo path. The main worktree is
/// excluded (handled by `GitOps::worktree_list`).
pub fn discover_worktrees(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
) -> BTreeMap<PathBuf, Vec<DiscoveredWorktree>> {
    let mut grouped: BTreeMap<PathBuf, Vec<DiscoveredWorktree>> = BTreeMap::new();

    for repo_path in repo_paths {
        let worktrees = match git.worktree_list(repo_path) {
            Ok(wts) => wts,
            Err(_) => continue,
        };

        for (wt_path, _branch) in worktrees {
            let worktree = DiscoveredWorktree {
                path: wt_path,
                parent_repo: repo_path.clone(),
            };
            grouped.entry(repo_path.clone()).or_default().push(worktree);
        }
    }

    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_tidy_core::testutil::MockGitBuilder;

    #[test]
    fn discover_worktrees_basic() {
        let repo = PathBuf::from("/repos/MyRepo");
        let wt_path = PathBuf::from("/worktrees/MyRepo-feature");

        let git = MockGitBuilder::new()
            .with_worktree_list(&repo, vec![(wt_path.clone(), Some("feature".to_string()))])
            .build();

        let result = discover_worktrees(&git, std::slice::from_ref(&repo));

        assert_eq!(result.len(), 1);
        let worktrees = &result[&repo];
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, wt_path);
        assert_eq!(worktrees[0].parent_repo, repo);
    }

    #[test]
    fn discover_worktrees_multiple_repos() {
        let repo_a = PathBuf::from("/repos/RepoA");
        let repo_b = PathBuf::from("/repos/RepoB");
        let wt_a1 = PathBuf::from("/worktrees/RepoA-feat1");
        let wt_a2 = PathBuf::from("/worktrees/RepoA-feat2");
        let wt_b1 = PathBuf::from("/worktrees/RepoB-fix");

        let git = MockGitBuilder::new()
            .with_worktree_list(
                &repo_a,
                vec![
                    (wt_a1.clone(), Some("feat1".to_string())),
                    (wt_a2.clone(), Some("feat2".to_string())),
                ],
            )
            .with_worktree_list(&repo_b, vec![(wt_b1.clone(), Some("fix".to_string()))])
            .build();

        let result = discover_worktrees(&git, &[repo_a.clone(), repo_b.clone()]);

        assert_eq!(result.len(), 2);
        assert_eq!(result[&repo_a].len(), 2);
        assert_eq!(result[&repo_b].len(), 1);
    }

    #[test]
    fn discover_worktrees_no_linked_worktrees() {
        let repo = PathBuf::from("/repos/MyRepo");

        let git = MockGitBuilder::new()
            .with_worktree_list(&repo, vec![])
            .build();

        let result = discover_worktrees(&git, &[repo]);
        assert!(result.is_empty());
    }

    #[test]
    fn discover_worktrees_empty_repos() {
        let git = MockGitBuilder::new().build();

        let result = discover_worktrees(&git, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn discover_worktrees_error_skips_repo() {
        let repo_ok = PathBuf::from("/repos/OkRepo");
        let repo_err = PathBuf::from("/repos/ErrRepo");
        let wt = PathBuf::from("/worktrees/OkRepo-feat");

        let git = MockGitBuilder::new()
            .with_worktree_list(&repo_ok, vec![(wt.clone(), Some("feat".to_string()))])
            // repo_err has no worktree_list configured, so it returns empty by default
            .build();

        let result = discover_worktrees(&git, &[repo_ok.clone(), repo_err]);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&repo_ok));
    }

    #[test]
    fn discover_worktrees_detached_head() {
        let repo = PathBuf::from("/repos/MyRepo");
        let wt_path = PathBuf::from("/worktrees/MyRepo-detached");

        let git = MockGitBuilder::new()
            .with_worktree_list(&repo, vec![(wt_path.clone(), None)])
            .build();

        let result = discover_worktrees(&git, std::slice::from_ref(&repo));

        assert_eq!(result.len(), 1);
        let worktrees = &result[&repo];
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, wt_path);
    }
}
