use std::path::Path;

use crate::dirty;
use crate::error::Error;
use crate::git::GitOps;
use crate::landed::{self, LandedCache, LandedOptions};
use crate::types::{
    Annotations, BranchClassification, Classification, ClassificationLabel, WorktreeInfo,
};

/// Decide whether a branch/worktree classification is eligible for cleanup,
/// given the shared `--all` / `--strict` flags.
///
/// This is the pure classification filter shared verbatim by branch-tidy and
/// worktree-tidy (each tool's former `should_clean`): `--all` admits everything;
/// `--strict` admits only structurally-proven [`Classification::Landed`];
/// otherwise the default admits landed, landed-stale, and landed-by-content.
pub fn should_clean_landed(classification: &Classification, all: bool, strict: bool) -> bool {
    if all {
        return true;
    }

    if strict {
        return matches!(classification, Classification::Landed);
    }

    // Default: landed (structural) + landed-stale + landed-by-content
    matches!(
        classification,
        Classification::Landed
            | Classification::LandedStale
            | Classification::LandedByContent { .. }
    )
}

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
    landed_options: &LandedOptions,
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
        let landed_result = landed::detect_landed(
            git,
            repo,
            &comparison_target,
            branch_name,
            verbose,
            landed_options,
        )?;

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

/// Classify a branch, reusing cached landed detection results from previous
/// branches in the same repo. Use this when classifying multiple branches
/// within a single repo to avoid redundant git subprocess calls for shared commits.
#[allow(clippy::too_many_arguments)]
pub fn classify_branch_cached(
    git: &dyn GitOps,
    repo: &Path,
    branch_name: &str,
    default_branch: &str,
    behind_threshold: usize,
    verbose: bool,
    landed_cache: &LandedCache,
    landed_options: &LandedOptions,
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
        // Try content-based landed detection with cache
        let landed_result = landed::detect_landed_cached(
            git,
            repo,
            &comparison_target,
            branch_name,
            verbose,
            landed_cache,
            landed_options,
        )?;

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

/// Classify a remote-only branch (one that exists on origin but has no local counterpart).
///
/// Uses `origin/{branch_name}` as the git ref for all comparisons. Always returns
/// `remote_tracking: true` and `remote_deleted: false`. Non-merged branches are
/// classified as `Active` (never `Local`, since they exist on the remote by definition).
pub fn classify_remote_branch(
    git: &dyn GitOps,
    repo: &Path,
    branch_name: &str,
    default_branch: &str,
    behind_threshold: usize,
    verbose: bool,
    landed_options: &LandedOptions,
) -> Result<BranchClassification, Error> {
    let origin_ref = format!("refs/remotes/origin/{default_branch}");
    let has_origin = git.rev_parse_verify(repo, &origin_ref)?;
    let comparison_target = if has_origin {
        format!("origin/{default_branch}")
    } else {
        default_branch.to_string()
    };

    let remote_ref = format!("origin/{branch_name}");

    let (behind, ahead) = git.rev_list_left_right_count(repo, &comparison_target, &remote_ref)?;

    if verbose {
        eprintln!("  {branch_name} (remote): ahead={ahead}, behind={behind}");
    }

    let is_merged = ahead == 0 || git.is_ancestor(repo, &remote_ref, &comparison_target)?;

    let classification = if is_merged {
        if verbose {
            eprintln!("  {branch_name} (remote): structurally merged → landed");
        }
        Classification::Landed
    } else {
        let landed_result = landed::detect_landed(
            git,
            repo,
            &comparison_target,
            &remote_ref,
            verbose,
            landed_options,
        )?;

        let cls = match &landed_result.classification {
            Classification::LandedByContent { .. } if landed_result.total > 0 => {
                landed_result.classification
            }
            Classification::LandedByContent { .. } => Classification::Landed,
            Classification::LandedPartial { .. } => landed_result.classification,
            _ => Classification::Active,
        };
        if verbose {
            eprintln!(
                "  {branch_name} (remote): content detection ({}/{}) → {}",
                landed_result.matched,
                landed_result.total,
                cls.label(),
            );
        }
        cls
    };

    Ok(BranchClassification {
        classification,
        remote_tracking: true,
        remote_deleted: false,
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

    // Detect dangling branch ref: branch name is known but ref doesn't resolve.
    // This happens when a PR is merged and the branch is deleted while the worktree remains.
    if let Some(ref branch_name) = branch {
        let ref_path = format!("refs/heads/{branch_name}");
        if !git.rev_parse_verify(parent_repo, &ref_path)? {
            let dirty_result = dirty::check_dirty(git, worktree_path, noise_patterns)?;
            return Ok(WorktreeInfo {
                path: worktree_path.to_path_buf(),
                parent_repo: parent_repo.to_path_buf(),
                branch,
                default_branch: default_branch.to_string(),
                classification: Classification::LandedStale,
                annotations: Annotations {
                    dirty: !dirty_result.meaningful_files.is_empty(),
                    dirty_file_count: dirty_result.meaningful_files.len(),
                    ..Default::default()
                },
                remote_tracking: false,
                ahead: 0,
                behind: 0,
                dirty_files: dirty_result.all_files,
                meaningful_dirty_files: dirty_result.meaningful_files,
            });
        }
    }

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
        &LandedOptions::default(),
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
        // For detached HEAD, branch_ref is a commit SHA. classify_branch computed `remote_tracking` by looking up `refs/remotes/origin/<sha>`, which never resolves — so `bc.remote_tracking` is meaninglessly false. Zero it out explicitly so downstream consumers cannot mistake "no analysis" for "no upstream".
        remote_tracking: !is_detached && bc.remote_tracking,
        ahead: if is_detached { 0 } else { bc.ahead },
        behind: if is_detached { 0 } else { bc.behind },
        dirty_files: dirty_result.all_files,
        meaningful_dirty_files: dirty_result.meaningful_files,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::dirty::DEFAULT_NOISE_PATTERNS;
    use crate::landed::LandedOptions;
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/done", true)
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/wip", true)
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/local", true)
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/dirty", true)
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/old", true)
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
            .with_rev_parse_verify(&repo(), "refs/heads/feature/gone", true)
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

    #[test]
    fn classify_detached_head_zeros_remote_fields() {
        // Regression: classify_branch is called with the detached commit SHA as branch_ref. If the underlying BranchClassification spuriously reports remote_tracking/ahead/behind (because some ref happens to resolve, or the mock fabricates one as below), the worktree layer must clamp these to false/0 — they are meaningless for a detached HEAD.
        let detached_sha = "abc123def";
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), None) // detached HEAD
            .with_rev_parse(&wt(), "HEAD", detached_sha)
            // Force classify_branch to compute non-zero remote analysis: pretend `refs/remotes/origin/<sha>` resolves and the underlying counts are non-zero.
            .with_rev_parse_verify(
                &repo(),
                &format!("refs/remotes/origin/{detached_sha}"),
                true,
            )
            .with_rev_list_counts(&repo(), "origin/main", detached_sha, (3, 5))
            .with_is_ancestor(&repo(), detached_sha, "origin/main", false)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert!(info.branch.is_none(), "test premise: branch must be None");
        assert!(
            !info.remote_tracking,
            "detached HEAD must report remote_tracking=false even when classify_branch reports otherwise",
        );
        assert_eq!(info.ahead, 0, "detached HEAD ahead must be 0");
        assert_eq!(info.behind, 0, "detached HEAD behind must be 0");
        assert!(
            !info.annotations.remote_deleted,
            "detached HEAD must not report remote_deleted",
        );
    }

    // --- classify_branch tests ---

    #[test]
    fn classify_branch_merged() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/feature/done", true)
            .with_rev_list_counts(&repo(), "origin/main", "feature/done", (0, 0))
            .with_is_ancestor(&repo(), "feature/done", "origin/main", true)
            .build();

        let result = classify_branch(
            &git,
            &repo(),
            "feature/done",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/wip",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/local",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/gone",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/old",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/behind",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

    // Origin-over-local precedence is unobservable at this seam: when
    // `refs/remotes/origin/main` exists, `detect_default_branch` returns at method 2
    // before ever calling `rev_parse_verify` on `refs/heads/main` (method 4). No mock
    // configuration of the local ref can change the outcome — the local lookup never
    // happens — so a "prefers origin over local" test would exercise the identical path
    // as `detect_default_branch_probe_main` and could not fail if precedence regressed.
    // The precedence is implied structurally by the method ordering; the local-fallback
    // path is covered by `detect_default_branch_local_main` / `_local_master`.

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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/done",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

        let result = classify_branch(
            &git,
            &repo(),
            "feature/wip",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
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

    #[test]
    fn classify_dangling_ref_returns_landed_stale() {
        // Branch name is known but the ref doesn't exist (deleted after merge)
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/merged"))
            .with_rev_parse_verify(&repo(), "refs/heads/feature/merged", false)
            .with_status_porcelain(&wt(), vec![])
            .build();

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::LandedStale);
        assert_eq!(info.branch.as_deref(), Some("feature/merged"));
        assert!(!info.annotations.dirty);
        assert!(!info.remote_tracking);
        assert_eq!(info.ahead, 0);
        assert_eq!(info.behind, 0);
    }

    #[test]
    fn classify_dangling_ref_with_dirty_files() {
        let git = MockGitBuilder::new()
            .with_worktree_branch(&wt(), Some("feature/merged-dirty"))
            .with_rev_parse_verify(&repo(), "refs/heads/feature/merged-dirty", false)
            .with_status_porcelain(&wt(), vec![" M src/lib.rs".to_string()])
            .build();

        let info =
            classify_worktree(&git, &wt(), &repo(), "main", 100, false, &default_noise()).unwrap();
        assert_eq!(info.classification, Classification::LandedStale);
        assert!(info.annotations.dirty);
        assert_eq!(info.annotations.dirty_file_count, 1);
    }

    // --- classify_remote_branch tests ---

    #[test]
    fn classify_remote_branch_landed() {
        // Remote branch whose commits are all reachable from origin/main
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_list_counts(&repo(), "origin/main", "origin/feature/done", (0, 0))
            .with_is_ancestor(&repo(), "origin/feature/done", "origin/main", true)
            .build();

        let result = classify_remote_branch(
            &git,
            &repo(),
            "feature/done",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(result.classification, Classification::Landed);
        assert!(result.remote_tracking);
        assert!(!result.remote_deleted);
    }

    #[test]
    fn classify_remote_branch_active() {
        // Remote branch with commits not yet merged into origin/main
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_list_counts(&repo(), "origin/main", "origin/feature/wip", (5, 3))
            .with_is_ancestor(&repo(), "origin/feature/wip", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "origin/feature/wip",
                vec![("abc".into(), "add feature".into())],
            )
            .build();

        let result = classify_remote_branch(
            &git,
            &repo(),
            "feature/wip",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        assert_eq!(result.classification, Classification::Active);
        assert!(result.remote_tracking);
        assert_eq!(result.ahead, 3);
        assert_eq!(result.behind, 5);
    }

    #[test]
    fn classify_remote_branch_diverged() {
        let git = MockGitBuilder::new()
            .with_rev_parse_verify(&repo(), "refs/remotes/origin/main", true)
            .with_rev_list_counts(&repo(), "origin/main", "origin/feature/old", (150, 5))
            .with_is_ancestor(&repo(), "origin/feature/old", "origin/main", false)
            .with_log_exclusive(
                &repo(),
                "origin/main",
                "origin/feature/old",
                vec![("jkl".into(), "old work".into())],
            )
            .build();

        let result = classify_remote_branch(
            &git,
            &repo(),
            "feature/old",
            "main",
            100,
            false,
            &LandedOptions::default(),
        )
        .unwrap();
        assert!(result.diverged);
        assert_eq!(result.behind, 150);
        assert_eq!(result.classification, Classification::Active);
    }
}
