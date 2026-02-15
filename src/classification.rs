use std::path::Path;

use crate::dirty;
use crate::error::Error;
use crate::git::GitOps;
use crate::landed;
use crate::types::{Annotations, Classification, WorktreeInfo};

/// Detect the default branch for a repo.
/// 1. Try `git symbolic-ref refs/remotes/origin/HEAD`
/// 2. Probe for `origin/main`, then `origin/master`
/// 3. Return error if neither exists
pub fn detect_default_branch(git: &dyn GitOps, repo: &Path) -> Result<String, Error> {
    // Method 1: symbolic-ref
    if let Some(branch) = git.symbolic_ref_origin_head(repo)? {
        return Ok(branch);
    }

    // Method 2: probe for origin/main
    if git.rev_parse_verify(repo, "refs/remotes/origin/main")? {
        return Ok("main".to_string());
    }

    // Method 3: probe for origin/master
    if git.rev_parse_verify(repo, "refs/remotes/origin/master")? {
        return Ok("master".to_string());
    }

    Err(Error::NoDefaultBranch {
        repo: repo.to_path_buf(),
    })
}

/// Classify a single worktree.
pub fn classify_worktree(
    git: &dyn GitOps,
    worktree_path: &Path,
    parent_repo: &Path,
    default_branch: &str,
    behind_threshold: usize,
    verbose: bool,
) -> Result<WorktreeInfo, Error> {
    let branch = git.worktree_branch(worktree_path)?;
    let origin_default = format!("origin/{default_branch}");

    // Determine the ref to compare against
    let branch_ref = match &branch {
        Some(b) => b.clone(),
        None => {
            // Detached HEAD — use the HEAD commit directly
            git.rev_parse(worktree_path, "HEAD")?
        }
    };

    // Check remote tracking branch
    let remote_ref = branch
        .as_ref()
        .map(|b| format!("refs/remotes/origin/{b}"));
    let has_remote = match &remote_ref {
        Some(rr) => git.rev_parse_verify(parent_repo, rr)?,
        None => false,
    };

    // Remote deleted: had a tracking branch that no longer exists after fetch --prune
    // (We check after fetch, so if it doesn't exist, it was pruned)
    let remote_deleted = branch.is_some() && !has_remote;

    // Ahead/behind counts
    let (behind, ahead) = git.rev_list_left_right_count(
        parent_repo,
        &origin_default,
        &branch_ref,
    )?;

    // Dirty detection
    let dirty_result = dirty::check_dirty(git, worktree_path)?;

    // Check if merged
    let is_merged = git.is_ancestor(parent_repo, &branch_ref, &origin_default)?;

    let classification = if is_merged {
        Classification::Merged
    } else {
        // Try landed detection
        let landed_result =
            landed::detect_landed(git, parent_repo, &origin_default, &branch_ref, verbose)?;

        match &landed_result.classification {
            Classification::Landed { .. } if landed_result.total > 0 => {
                landed_result.classification.clone()
            }
            Classification::Landed { .. } => {
                // 0 unique commits — effectively merged
                Classification::Merged
            }
            Classification::LandedPartial { .. } => landed_result.classification.clone(),
            _ => {
                // No commits landed — classify as active or local
                if has_remote {
                    Classification::Active
                } else {
                    Classification::Local
                }
            }
        }
    };

    let annotations = Annotations {
        remote_deleted,
        diverged: behind > behind_threshold,
        dirty: !dirty_result.meaningful_files.is_empty(),
        dirty_file_count: dirty_result.meaningful_files.len(),
    };

    Ok(WorktreeInfo {
        path: worktree_path.to_path_buf(),
        parent_repo: parent_repo.to_path_buf(),
        branch,
        default_branch: default_branch.to_string(),
        classification,
        annotations,
        remote_tracking: has_remote,
        ahead,
        behind,
        dirty_files: dirty_result.all_files,
        meaningful_dirty_files: dirty_result.meaningful_files,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::git::tests::MockGitBuilder;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn wt() -> PathBuf {
        PathBuf::from("/worktree")
    }

    #[test]
    fn detect_default_branch_symbolic_ref() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), Some("main"))
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "main");
    }

    #[test]
    fn detect_default_branch_probe_main() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "main");
    }

    #[test]
    fn detect_default_branch_probe_master() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/master", true)
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "master");
    }

    #[test]
    fn detect_default_branch_none() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/master", false)
            .build();
        let result = detect_default_branch(&git, &repo());
        assert!(result.is_err());
    }

    #[test]
    fn classify_merged_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/done"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "origin/main", true)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Merged);
        assert!(!info.annotations.dirty);
        assert!(!info.annotations.remote_deleted);
    }

    #[test]
    fn classify_active_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/wip"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/wip", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/wip", (5, 3))
            .with_is_ancestor(&repo(), "feature/wip", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/wip",
                vec![("abc".into(), "add feature".into())],
            )
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Active);
        assert_eq!(info.ahead, 3);
        assert_eq!(info.behind, 5);
    }

    #[test]
    fn classify_local_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/local"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/local", false)
            .with_rev_list_counts(&repo(), "origin/main", "feature/local", (10, 2))
            .with_is_ancestor(&repo(), "feature/local", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/local",
                vec![("def".into(), "local work".into())],
            )
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Local);
        assert!(!info.remote_tracking);
    }

    #[test]
    fn classify_dirty_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/dirty"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/dirty", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/dirty", (0, 1))
            .with_is_ancestor(&repo(), "feature/dirty", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/dirty",
                vec![("ghi".into(), "wip".into())],
            )
            .with_status_porcelain(
                &wt(),
                vec![
                    " M src/main.rs".to_string(),
                    "?? .DS_Store".to_string(),
                ],
            )
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert!(info.annotations.dirty);
        assert_eq!(info.annotations.dirty_file_count, 1); // .DS_Store filtered
    }

    #[test]
    fn classify_diverged_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/old"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/old", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/old", (150, 5))
            .with_is_ancestor(&repo(), "feature/old", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/old",
                vec![("jkl".into(), "old work".into())],
            )
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert!(info.annotations.diverged);
        assert_eq!(info.behind, 150);
    }

    #[test]
    fn classify_remote_deleted() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/gone"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/gone", false)
            .with_rev_list_counts(&repo(), "origin/main", "feature/gone", (0, 0))
            .with_is_ancestor(&repo(), "feature/gone", "origin/main", true)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Merged);
        assert!(info.annotations.remote_deleted);
    }

    #[test]
    fn classify_detached_head_merged() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), None) // detached HEAD
            .with_rev_parse(&wt(), "HEAD", "abc123def")
            .with_rev_list_counts(&repo(), "origin/main", "abc123def", (0, 0))
            .with_is_ancestor(&repo(), "abc123def", "origin/main", true)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Merged);
        assert!(info.branch.is_none());
    }

    #[test]
    fn classify_detached_head_local() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), None)
            .with_rev_parse(&wt(), "HEAD", "def456abc")
            .with_rev_list_counts(&repo(), "origin/main", "def456abc", (10, 3))
            .with_is_ancestor(&repo(), "def456abc", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "def456abc",
                vec![("xyz".into(), "detached work".into())],
            )
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info = classify_worktree(&git, &wt(), &repo(), "main", 100, false).unwrap();
        assert_eq!(info.classification, Classification::Local);
    }
}
