use std::collections::BTreeMap;
use std::path::Path;

use git_tidy_core::caching::CachingGitOps;
use git_tidy_core::config;
use git_tidy_core::git::{GitOps, RealGit};
use git_tidy_core::progress::Progress;

use crate::runner::matches_filter;
use crate::types::{AuditResult, TOOL_SPECS, ToolResult};

// Default parameter values matching each tool's CLI defaults.

/// Default behind threshold (worktree-tidy, branch-tidy).
const DEFAULT_BEHIND_THRESHOLD: usize = 100;
/// Default stash age threshold in days (stash-tidy).
const DEFAULT_AGE_THRESHOLD: u64 = 90;
/// Default stale months (repo-tidy), converted to days.
const DEFAULT_STALE_DAYS: u64 = 6 * 30;
/// Default LFS size threshold in bytes (1MB).
const DEFAULT_LFS_SIZE_THRESHOLD: u64 = 1_000_000;
/// Default LFS commit depth for large blob scanning.
const DEFAULT_LFS_DEPTH: usize = 1000;

/// Run an audit by calling each tool's scan/lint function in-process,
/// sharing a `CachingGitOps` to avoid redundant git calls.
pub fn run_audit_inprocess(
    directory: &Path,
    tool_filter: Option<&[String]>,
    progress: &Progress,
) -> AuditResult {
    let git = RealGit;
    let caching = CachingGitOps::new(&git);

    // Resolve noise patterns from config file (no CLI overrides in audit mode).
    let noise_patterns = resolve_noise_patterns();

    // Collect tools to run (for progress bar length).
    let specs: Vec<_> = TOOL_SPECS
        .iter()
        .filter(|spec| {
            tool_filter
                .map(|f| f.iter().any(|entry| matches_filter(spec.binary, entry)))
                .unwrap_or(true)
        })
        .collect();

    let pb = progress.bar(specs.len() as u64, "Auditing");
    let mut results = Vec::new();
    let mut tools_found = Vec::new();

    // Sub-tool scans get disabled progress to avoid visual conflicts.
    let sub_progress = Progress::disabled();

    for (idx, spec) in specs.iter().enumerate() {
        pb.set_message(format!(
            "[{}/{}] Scanning {}...",
            idx + 1,
            specs.len(),
            spec.item_noun
        ));

        tools_found.push(spec.binary.to_string());

        let result = run_tool_scan(
            spec.binary,
            &caching,
            directory,
            &noise_patterns,
            &sub_progress,
        );
        results.push(result);
        pb.inc(1);
    }
    pb.finish_and_clear();

    AuditResult {
        directory: directory.to_path_buf(),
        tools_found,
        tools_missing: vec![],
        results,
    }
}

/// Resolve noise patterns from the config file with default settings.
fn resolve_noise_patterns() -> Vec<String> {
    let (config_extra, config_exclude) = config::default_config_path()
        .map(|p| config::load_config_file(&p))
        .unwrap_or_default();
    let noise_config = config::NoiseConfig {
        config_extra,
        config_exclude,
        cli_extra: vec![],
        no_defaults: false,
    };
    noise_config.resolve()
}

/// Call the appropriate scan/lint function for a tool and convert to `ToolResult`.
fn run_tool_scan(
    binary: &str,
    git: &dyn GitOps,
    directory: &Path,
    noise_patterns: &[String],
    progress: &Progress,
) -> ToolResult {
    match binary {
        "git-worktree-tidy" => scan_worktrees(git, directory, noise_patterns, progress),
        "git-branch-tidy" => scan_branches(git, directory, progress),
        "git-stash-tidy" => scan_stashes(git, directory, progress),
        "git-remote-tidy" => scan_remotes(git, directory, progress),
        "git-tag-tidy" => scan_tags(git, directory, progress),
        "git-repo-tidy" => scan_repos(git, directory, noise_patterns, progress),
        "git-config-tidy" => lint_config(git, directory, progress),
        "git-lfs-tidy" => scan_lfs(git, directory, progress),
        _ => ToolResult {
            name: binary.to_string(),
            item_noun: "unknown".to_string(),
            total: 0,
            counts: BTreeMap::new(),
            error: Some(format!("unknown tool: {binary}")),
        },
    }
}

/// Helper to build a `BTreeMap` from (label, count) pairs, omitting zeros.
fn counts_map(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
    entries
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(label, count)| (label.to_string(), *count))
        .collect()
}

fn scan_worktrees(
    git: &dyn GitOps,
    directory: &Path,
    noise_patterns: &[String],
    progress: &Progress,
) -> ToolResult {
    match git_worktree_tidy::scan::run_scan(
        git,
        directory,
        DEFAULT_BEHIND_THRESHOLD,
        false,
        noise_patterns,
        &[],
        progress,
    ) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("landed", c.landed),
                ("landed-content", c.landed_content),
                ("partial", c.partial),
                ("active", c.active),
                ("local", c.local),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-worktree-tidy".to_string(),
                item_noun: "worktrees".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-worktree-tidy", "worktrees", e),
    }
}

fn scan_branches(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_branch_tidy::scan::run_scan(git, directory, DEFAULT_BEHIND_THRESHOLD, false, progress)
    {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("landed", c.landed),
                ("landed-content", c.landed_content),
                ("partial", c.partial),
                ("active", c.active),
                ("local", c.local),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-branch-tidy".to_string(),
                item_noun: "branches".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-branch-tidy", "branches", e),
    }
}

fn scan_stashes(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_stash_tidy::scan::run_scan(git, directory, DEFAULT_AGE_THRESHOLD, progress) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("committed", c.committed),
                ("orphaned", c.orphaned),
                ("aged", c.aged),
                ("active", c.active),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-stash-tidy".to_string(),
                item_noun: "stashes".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-stash-tidy", "stashes", e),
    }
}

fn scan_remotes(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_remote_tidy::scan::run_scan(git, directory, false, progress) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("unreachable", c.unreachable),
                ("orphaned", c.orphaned),
                ("active", c.active),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-remote-tidy".to_string(),
                item_noun: "remotes".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-remote-tidy", "remotes", e),
    }
}

fn scan_tags(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_tag_tidy::scan::run_scan(git, directory, false, progress) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("stale", c.stale),
                ("local_only", c.local_only),
                ("remote_only", c.remote_only),
                ("synced", c.synced),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-tag-tidy".to_string(),
                item_noun: "tags".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-tag-tidy", "tags", e),
    }
}

fn scan_repos(
    git: &dyn GitOps,
    directory: &Path,
    noise_patterns: &[String],
    progress: &Progress,
) -> ToolResult {
    match git_repo_tidy::scan::run_scan(
        git,
        directory,
        DEFAULT_STALE_DAYS,
        noise_patterns,
        false,
        progress,
    ) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("stale", c.stale),
                ("orphaned", c.orphaned),
                ("active", c.active),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-repo-tidy".to_string(),
                item_noun: "repos".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-repo-tidy", "repos", e),
    }
}

fn lint_config(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_config_tidy::lint::run_lint(git, directory, progress) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("orphaned_branch_config", c.orphaned_branch_config),
                ("alias_shadows_builtin", c.alias_shadows_builtin),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-config-tidy".to_string(),
                item_noun: "config issues".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-config-tidy", "config issues", e),
    }
}

fn scan_lfs(git: &dyn GitOps, directory: &Path, progress: &Progress) -> ToolResult {
    match git_lfs_tidy::scan::run_scan(
        git,
        directory,
        DEFAULT_LFS_SIZE_THRESHOLD,
        DEFAULT_LFS_DEPTH,
        progress,
    ) {
        Ok(result) => {
            let c = &result.counts;
            let counts = counts_map(&[
                ("untracked", c.untracked),
                ("missing", c.missing),
                ("orphaned", c.orphaned),
                ("healthy", c.healthy),
            ]);
            let total: usize = counts.values().sum();
            ToolResult {
                name: "git-lfs-tidy".to_string(),
                item_noun: "LFS files".to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => make_error_result("git-lfs-tidy", "LFS files", e),
    }
}

fn make_error_result(
    name: &str,
    item_noun: &str,
    error: git_tidy_core::error::Error,
) -> ToolResult {
    ToolResult {
        name: name.to_string(),
        item_noun: item_noun.to_string(),
        total: 0,
        counts: BTreeMap::new(),
        error: Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;

    #[test]
    fn counts_map_omits_zeros() {
        let m = counts_map(&[("active", 5), ("landed", 0), ("landed-content", 2)]);
        assert_eq!(m.len(), 2);
        assert_eq!(m["active"], 5);
        assert_eq!(m["landed-content"], 2);
        assert!(!m.contains_key("landed"));
    }

    #[test]
    fn scan_worktrees_with_empty_mock() {
        // MockGit with no data will produce zero worktrees
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = scan_worktrees(&mock, Path::new("/nonexistent"), &[], &p);
        // discover_worktrees will fail on nonexistent dir, giving an error
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_branches_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = scan_branches(&mock, Path::new("/nonexistent"), &p);
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_stashes_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = scan_stashes(&mock, Path::new("/nonexistent"), &p);
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn lint_config_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = lint_config(&mock, Path::new("/nonexistent"), &p);
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_lfs_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = scan_lfs(&mock, Path::new("/nonexistent"), &p);
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn make_error_result_captures_message() {
        let err = git_tidy_core::error::Error::DirectoryNotFound {
            path: "/test".into(),
        };
        let result = make_error_result("git-test-tidy", "items", err);
        assert_eq!(result.name, "git-test-tidy");
        assert_eq!(result.total, 0);
        assert!(
            result
                .error
                .as_ref()
                .unwrap()
                .contains("directory not found")
        );
    }

    #[test]
    fn unknown_tool_returns_error() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan("git-unknown-tidy", &mock, Path::new("/tmp"), &[], &p);
        assert!(result.error.is_some());
        assert!(result.error.as_ref().unwrap().contains("unknown tool"));
    }
}
