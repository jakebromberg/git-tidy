use std::path::Path;

use crate::dirty;
use crate::error::Error;
use crate::git::GitOps;
use crate::landed;
use crate::types::{
    Annotations, BranchClassification, Classification, ClassificationLabel, WorktreeInfo,
};

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

    // Method 4: probe for local main (no origin remote)
    if git.rev_parse_verify(repo, "refs/heads/main")? {
        return Ok("main".to_string());
    }

    // Method 5: probe for local master (no origin remote)
    if git.rev_parse_verify(repo, "refs/heads/master")? {
        return Ok("master".to_string());
    }

    Err(Error::NoDefaultBranch {
        repo: repo.to_path_buf(),
    })
}

/// Classify a branch by name without worktree-specific concerns (dirty detection).
///
/// This is the core classification logic shared by both worktree and branch tools.
pub fn classify_branch(
    git: &dyn GitOps,
    repo: &Path,
    branch_name: &str,
    default_branch: &str,
    behind_threshold: usize,
    verbose: bool,
) -> Result<BranchClassification, Error> {
    // Determine whether origin has the default branch
    let origin_ref = format!("refs/remotes/origin/{default_branch}");
    let has_origin = git.rev_parse_verify(repo, &origin_ref)?;
    let comparison_target = if has_origin {
        format!("origin/{default_branch}")
    } else {
        default_branch.to_string()
    };

    // Check remote tracking branch
    let remote_ref = format!("refs/remotes/origin/{branch_name}");
    let has_remote = git.rev_parse_verify(repo, &remote_ref)?;
    let remote_deleted = has_origin && !has_remote;

    // Ahead/behind counts
    let (behind, ahead) = git.rev_list_left_right_count(repo, &comparison_target, branch_name)?;

    if verbose {
        eprintln!("  {branch_name}: remote={has_remote}, ahead={ahead}, behind={behind}",);
    }

    // Check if structurally landed — skip the subprocess call when ahead == 0
    let is_merged = ahead == 0 || git.is_ancestor(repo, branch_name, &comparison_target)?;

    let classification = if is_merged {
        if verbose {
            eprintln!("  {branch_name}: structurally merged → landed");
        }
        Classification::Landed
    } else {
        // Try content-based landed detection
        let landed_result =
            landed::detect_landed(git, repo, &comparison_target, branch_name, verbose)?;

        enum Action {
            UseContentResult,
            Landed,
            Active,
            Local,
        }
        let action = match &landed_result.classification {
            Classification::LandedByContent { .. } if landed_result.total > 0 => {
                Action::UseContentResult
            }
            Classification::LandedByContent { .. } => Action::Landed,
            Classification::LandedPartial { .. } => Action::UseContentResult,
            _ if has_remote => Action::Active,
            _ => Action::Local,
        };
        let cls = match action {
            Action::UseContentResult => landed_result.classification,
            Action::Landed => Classification::Landed,
            Action::Active => Classification::Active,
            Action::Local => Classification::Local,
        };
        if verbose {
            eprintln!(
                "  {branch_name}: content detection ({}/{}) → {}",
                landed_result.matched,
                landed_result.total,
                cls.label(),
            );
        }
        cls
    };

    Ok(BranchClassification {
        classification,
        remote_tracking: has_remote,
        remote_deleted,
        ahead,
        behind,
        diverged: behind > behind_threshold,
    })
}

/// Classify a single worktree.
///
/// Resolves the branch from the worktree, delegates to `classify_branch` for
/// the core classification, then layers on worktree-specific concerns (dirty detection).
pub fn classify_worktree(
    git: &dyn GitOps,
    worktree_path: &Path,
    parent_repo: &Path,
    default_branch: &str,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
) -> Result<WorktreeInfo, Error> {
    let branch = git.worktree_branch(worktree_path)?;

    // Determine the ref to compare against
    let detached_head;
    let branch_ref: &str = match &branch {
        Some(b) => b,
        None => {
            // Detached HEAD — use the HEAD commit directly
            detached_head = git.rev_parse(worktree_path, "HEAD")?;
            &detached_head
        }
    };

    // For detached HEAD, remote_deleted doesn't apply
    let is_detached = branch.is_none();

    // Use classify_branch for the core classification logic
    let bc = classify_branch(
        git,
        parent_repo,
        branch_ref,
        default_branch,
        behind_threshold,
        verbose,
    )?;

    // Dirty detection (worktree-specific)
    let dirty_result = dirty::check_dirty(git, worktree_path, noise_patterns)?;

    if verbose && !dirty_result.meaningful_files.is_empty() {
        eprintln!(
            "  {}: {} dirty files (total={}, noise-filtered={})",
            branch_ref,
            dirty_result.meaningful_files.len(),
            dirty_result.all_files.len(),
            dirty_result.all_files.len() - dirty_result.meaningful_files.len(),
        );
    }

    let annotations = Annotations {
        // For detached HEAD, there's no branch so remote_deleted doesn't apply.
        // For named branches, remote_deleted is set by classify_branch.
        remote_deleted: !is_detached && bc.remote_deleted,
        diverged: bc.diverged,
        dirty: !dirty_result.meaningful_files.is_empty(),
        dirty_file_count: dirty_result.meaningful_files.len(),
    };

    Ok(WorktreeInfo {
        path: worktree_path.to_path_buf(),
        parent_repo: parent_repo.to_path_buf(),
        branch,
        default_branch: default_branch.to_string(),
        classification: bc.classification,
        annotations,
        remote_tracking: bc.remote_tracking,
        ahead: bc.ahead,
        behind: bc.behind,
        dirty_files: dirty_result.all_files,
        meaningful_dirty_files: dirty_result.meaningful_files,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::dirty::DEFAULT_NOISE_PATTERNS;
    use crate::testutil::MockGitBuilder;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn wt() -> PathBuf {
        PathBuf::from("/worktree")
    }

    fn default_noise() -> Vec<String> {
        DEFAULT_NOISE_PATTERNS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::Landed);
        assert!(!info.annotations.dirty);
        assert!(!info.annotations.remote_deleted);
    }

    #[test]
    fn classify_active_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/wip"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::Active);
        assert_eq!(info.ahead, 3);
        assert_eq!(info.behind, 5);
    }

    #[test]
    fn classify_local_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/local"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
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
                vec![" M src/main.rs".to_string(), "?? .DS_Store".to_string()],
            )
            .build();

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert!(info.annotations.dirty);
        assert_eq!(info.annotations.dirty_file_count, 1); // .DS_Store filtered
    }

    #[test]
    fn classify_diverged_branch() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/old"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert!(info.annotations.diverged);
        assert_eq!(info.behind, 150);
    }

    #[test]
    fn classify_remote_deleted() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/gone"))
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/gone", false)
            .with_rev_list_counts(&repo(), "origin/main", "feature/gone", (0, 0))
            .with_is_ancestor(&repo(), "feature/gone", "origin/main", true)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::Landed);
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::Landed);
        assert!(info.branch.is_none());
    }

    // --- classify_branch tests ---

    #[test]
    fn classify_branch_merged() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "origin/main", true)
            .build();

        let result = classify_branch(&git, &repo(), "feature/done", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Landed);
        assert!(result.remote_tracking);
        assert!(!result.remote_deleted);
        assert!(!result.diverged);
    }

    #[test]
    fn classify_branch_active() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/wip", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/wip", (5, 3))
            .with_is_ancestor(&repo(), "feature/wip", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/wip",
                vec![("abc".into(), "add feature".into())],
            )
            .build();

        let result = classify_branch(&git, &repo(), "feature/wip", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Active);
        assert!(result.remote_tracking);
        assert_eq!(result.ahead, 3);
        assert_eq!(result.behind, 5);
    }

    #[test]
    fn classify_branch_local() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let result = classify_branch(&git, &repo(), "feature/local", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Local);
        assert!(!result.remote_tracking);
    }

    #[test]
    fn classify_branch_remote_deleted() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/gone", false)
            .with_rev_list_counts(&repo(), "origin/main", "feature/gone", (0, 0))
            .with_is_ancestor(&repo(), "feature/gone", "origin/main", true)
            .build();

        let result = classify_branch(&git, &repo(), "feature/gone", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Landed);
        assert!(result.remote_deleted);
    }

    #[test]
    fn classify_branch_diverged() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/old", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/old", (150, 5))
            .with_is_ancestor(&repo(), "feature/old", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "feature/old",
                vec![("jkl".into(), "old work".into())],
            )
            .build();

        let result = classify_branch(&git, &repo(), "feature/old", "main", 100, false).unwrap();
        assert!(result.diverged);
        assert_eq!(result.behind, 150);
    }

    #[test]
    fn classify_branch_merged_ahead_zero_skips_is_ancestor() {
        // When ahead == 0 the branch is fully merged regardless of behind count,
        // so is_ancestor is skipped (not configured in the mock).
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/behind", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/behind", (50, 0))
            // No is_ancestor configured — proves the short-circuit works
            .build();

        let result = classify_branch(&git, &repo(), "feature/behind", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Landed);
        assert_eq!(result.behind, 50);
        assert_eq!(result.ahead, 0);
    }

    // --- detect_default_branch local fallback tests ---

    #[test]
    fn detect_default_branch_local_main() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/master", false)
            .with_rev_parse_verify(&repo(), "refs/heads/main", true)
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "main");
    }

    #[test]
    fn detect_default_branch_local_master() {
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/master", false)
            .with_rev_parse_verify(&repo(), "refs/heads/main", false)
            .with_rev_parse_verify(&repo(), "refs/heads/master", true)
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "master");
    }

    #[test]
    fn detect_default_branch_prefers_origin_over_local() {
        // Both origin/main and local main exist — origin check wins (step 2 before step 4)
        let git = MockGitBuilder::new()
            .with_symbolic_ref(&repo(), None)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            // local main also exists, but we never reach step 4
            .build();
        let result = detect_default_branch(&git, &repo()).unwrap();
        assert_eq!(result, "main");
    }

    // --- classify_branch local-only repo tests ---

    #[test]
    fn classify_branch_local_only_landed() {
        // No origin remote at all — branch merged into local main
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_list_counts(&repo(), "main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "main", true)
            .build();

        let result = classify_branch(&git, &repo(), "feature/done", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Landed);
        assert!(!result.remote_tracking);
        assert!(!result.remote_deleted); // no origin → remote_deleted should be false
    }

    #[test]
    fn classify_branch_local_only_active() {
        // No origin remote — branch ahead of local main
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/wip", false)
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", false)
            .with_rev_list_counts(&repo(), "main", "feature/wip", (0, 3))
            .with_is_ancestor(&repo(), "feature/wip", "main", false)
            .with_log_exclusive(
                &repo(),
                "main",
                "feature/wip",
                vec![("abc".into(), "local work".into())],
            )
            .build();

        let result = classify_branch(&git, &repo(), "feature/wip", "main", 100, false).unwrap();
        assert_eq!(result.classification, Classification::Local);
        assert!(!result.remote_tracking);
        assert!(!result.remote_deleted); // no origin → remote_deleted should be false
    }

    #[test]
    fn classify_detached_head_local() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), None)
            .with_rev_parse(&wt(), "HEAD", "def456abc")
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
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

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::Local);
    }
}
