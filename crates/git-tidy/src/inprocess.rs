use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use git_tidy_core::caching::CachingGitOps;
use git_tidy_core::config;
use git_tidy_core::discovery;
use git_tidy_core::filter::NameFilter;
use git_tidy_core::git::GitOps;
use git_tidy_core::gix_ops::GixGitOps;
use git_tidy_core::progress::Progress;

use crate::runner::matches_filter;
use crate::types::{AuditResult, TOOL_SPECS, ToolResult, ToolSpec};

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
    verbose: bool,
    progress: &Progress,
) -> AuditResult {
    let git = GixGitOps;
    let caching = CachingGitOps::new(&git);

    // Resolve noise patterns from config file (no CLI overrides in audit mode).
    let noise_patterns = resolve_noise_patterns();

    // Discover repos once and share across all tools.
    let repo_paths = discovery::discover_repos(directory).unwrap_or_default();

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
            spec,
            &caching,
            directory,
            &repo_paths,
            &noise_patterns,
            verbose,
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

/// Convert a tool scan/lint `Result` into a `ToolResult`, extracting counts
/// via a closure. Handles the Ok→counts and Err→error paths uniformly.
fn scan_to_result<T>(
    scan_result: Result<T, git_tidy_core::error::Error>,
    spec: &ToolSpec,
    extract_counts: impl FnOnce(&T) -> Vec<(&str, usize)>,
) -> ToolResult {
    match scan_result {
        Ok(r) => {
            let counts: BTreeMap<String, usize> = extract_counts(&r)
                .into_iter()
                .filter(|(_, count)| *count > 0)
                .map(|(label, count)| (label.to_string(), count))
                .collect();
            let total: usize = counts.values().sum();
            ToolResult {
                name: spec.binary.to_string(),
                item_noun: spec.item_noun.to_string(),
                total,
                counts,
                error: None,
            }
        }
        Err(e) => ToolResult {
            name: spec.binary.to_string(),
            item_noun: spec.item_noun.to_string(),
            total: 0,
            counts: BTreeMap::new(),
            error: Some(e.to_string()),
        },
    }
}

/// Call the appropriate scan/lint function for a tool and convert to `ToolResult`.
///
/// Most tools use `run_scan_repos` / `run_lint_repos` with the shared `repo_paths`
/// (discovered once). Worktree-tidy uses its own discovery, so it still receives
/// `directory` and calls `run_scan` directly.
fn run_tool_scan(
    spec: &ToolSpec,
    git: &dyn GitOps,
    directory: &Path,
    repo_paths: &[PathBuf],
    noise_patterns: &[String],
    verbose: bool,
    progress: &Progress,
) -> ToolResult {
    let filter = NameFilter::default();

    match spec.binary {
        "git-worktree-tidy" => scan_to_result(
            git_worktree_tidy::scan::run_scan(
                git,
                directory,
                DEFAULT_BEHIND_THRESHOLD,
                verbose,
                noise_patterns,
                &filter,
                &filter,
                progress,
            ),
            spec,
            |r| {
                vec![
                    ("landed", r.counts.landed),
                    ("landed-content", r.counts.landed_content),
                    ("partial", r.counts.partial),
                    ("active", r.counts.active),
                    ("local", r.counts.local),
                ]
            },
        ),
        "git-branch-tidy" => scan_to_result(
            git_branch_tidy::scan::run_scan_repos(
                git,
                repo_paths,
                DEFAULT_BEHIND_THRESHOLD,
                verbose,
                &filter,
                progress,
            ),
            spec,
            |r| {
                vec![
                    ("landed", r.counts.landed),
                    ("landed-content", r.counts.landed_content),
                    ("partial", r.counts.partial),
                    ("active", r.counts.active),
                    ("local", r.counts.local),
                ]
            },
        ),
        "git-stash-tidy" => scan_to_result(
            git_stash_tidy::scan::run_scan_repos(
                git,
                repo_paths,
                DEFAULT_AGE_THRESHOLD,
                verbose,
                &filter,
                progress,
            ),
            spec,
            |r| {
                vec![
                    ("committed", r.counts.committed),
                    ("orphaned", r.counts.orphaned),
                    ("aged", r.counts.aged),
                    ("active", r.counts.active),
                ]
            },
        ),
        "git-remote-tidy" => scan_to_result(
            git_remote_tidy::scan::run_scan_repos(
                git, repo_paths, false, verbose, &filter, progress,
            ),
            spec,
            |r| {
                vec![
                    ("unreachable", r.counts.unreachable),
                    ("orphaned", r.counts.orphaned),
                    ("active", r.counts.active),
                ]
            },
        ),
        "git-tag-tidy" => scan_to_result(
            git_tag_tidy::scan::run_scan_repos(
                git, repo_paths, false, verbose, &filter, progress,
            ),
            spec,
            |r| {
                vec![
                    ("stale", r.counts.stale),
                    ("local_only", r.counts.local_only),
                    ("remote_only", r.counts.remote_only),
                    ("synced", r.counts.synced),
                ]
            },
        ),
        "git-repo-tidy" => scan_to_result(
            git_repo_tidy::scan::run_scan_repos(
                git,
                repo_paths,
                DEFAULT_STALE_DAYS,
                noise_patterns,
                false,
                verbose,
                progress,
            ),
            spec,
            |r| {
                vec![
                    ("stale", r.counts.stale),
                    ("orphaned", r.counts.orphaned),
                    ("active", r.counts.active),
                ]
            },
        ),
        "git-config-tidy" => scan_to_result(
            git_config_tidy::lint::run_lint_repos(git, repo_paths, verbose, progress),
            spec,
            |r| {
                vec![
                    ("orphaned_branch_config", r.counts.orphaned_branch_config),
                    ("alias_shadows_builtin", r.counts.alias_shadows_builtin),
                ]
            },
        ),
        "git-lfs-tidy" => scan_to_result(
            git_lfs_tidy::scan::run_scan_repos(
                git,
                repo_paths,
                DEFAULT_LFS_SIZE_THRESHOLD,
                DEFAULT_LFS_DEPTH,
                verbose,
                progress,
            ),
            spec,
            |r| {
                vec![
                    ("untracked", r.counts.untracked),
                    ("missing", r.counts.missing),
                    ("orphaned", r.counts.orphaned),
                    ("healthy", r.counts.healthy),
                ]
            },
        ),
        _ => ToolResult {
            name: spec.binary.to_string(),
            item_noun: spec.item_noun.to_string(),
            total: 0,
            counts: BTreeMap::new(),
            error: Some(format!("unknown tool: {}", spec.binary)),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;

    fn spec_for(binary: &str) -> &'static ToolSpec {
        TOOL_SPECS.iter().find(|s| s.binary == binary).unwrap()
    }

    #[test]
    fn scan_to_result_omits_zero_counts() {
        let ok: Result<Vec<(&str, usize)>, git_tidy_core::error::Error> =
            Ok(vec![("active", 5), ("landed", 0), ("stale", 2)]);
        let spec = spec_for("git-branch-tidy");
        let result = scan_to_result(ok, spec, |entries| entries.clone());
        assert_eq!(result.counts.len(), 2);
        assert_eq!(result.counts["active"], 5);
        assert_eq!(result.counts["stale"], 2);
        assert!(!result.counts.contains_key("landed"));
        assert_eq!(result.total, 7);
        assert!(result.error.is_none());
    }

    #[test]
    fn scan_to_result_captures_error() {
        let err: Result<(), _> = Err(git_tidy_core::error::Error::DirectoryNotFound {
            path: "/test".into(),
        });
        let spec = spec_for("git-branch-tidy");
        let result = scan_to_result(err, spec, |_| vec![]);
        assert_eq!(result.name, "git-branch-tidy");
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
    fn scan_worktrees_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan(
            spec_for("git-worktree-tidy"),
            &mock,
            Path::new("/nonexistent"),
            &[],
            &[],
            false,
            &p,
        );
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_branches_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan(
            spec_for("git-branch-tidy"),
            &mock,
            Path::new("/nonexistent"),
            &[],
            &[],
            false,
            &p,
        );
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_stashes_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan(
            spec_for("git-stash-tidy"),
            &mock,
            Path::new("/nonexistent"),
            &[],
            &[],
            false,
            &p,
        );
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn lint_config_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan(
            spec_for("git-config-tidy"),
            &mock,
            Path::new("/nonexistent"),
            &[],
            &[],
            false,
            &p,
        );
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn scan_lfs_with_empty_mock() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let result = run_tool_scan(
            spec_for("git-lfs-tidy"),
            &mock,
            Path::new("/nonexistent"),
            &[],
            &[],
            false,
            &p,
        );
        assert!(result.error.is_some() || result.total == 0);
    }

    #[test]
    fn unknown_tool_returns_error() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let unknown = ToolSpec {
            binary: "git-unknown-tidy",
            item_noun: "unknowns",
            scan_command: "scan",
            count_field: "classification",
            aliases: &[],
        };
        let result = run_tool_scan(&unknown, &mock, Path::new("/tmp"), &[], &[], false, &p);
        assert!(result.error.is_some());
        assert!(result.error.as_ref().unwrap().contains("unknown tool"));
    }
}
