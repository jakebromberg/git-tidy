use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use git_tidy_core::caching::CachingGitOps;
use git_tidy_core::config;
use git_tidy_core::counts::Counts;
use git_tidy_core::discovery;
use git_tidy_core::filter::NameFilter;
use git_tidy_core::git::GitOps;
use git_tidy_core::gix_ops::GixGitOps;
use git_tidy_core::progress::Progress;
use indicatif::{ProgressBar, ProgressStyle};

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

/// Format a completed spinner message from a `ToolResult`.
///
/// - Success with counts: `"✓ branches: 12 scanned (3 landed, 9 active)"`
/// - Success with zero: `"✓ LFS files: 0 scanned"`
/// - Error: `"✗ branches: error: process failed"`
fn format_spinner_done(result: &ToolResult) -> String {
    if let Some(ref err) = result.error {
        return format!("✗ {}: error: {err}", result.item_noun);
    }

    let counts_str = if result.counts.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = result
            .counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect();
        format!(" ({})", parts.join(", "))
    };

    format!(
        "✓ {}: {} scanned{counts_str}",
        result.item_noun, result.total,
    )
}

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

    // Collect tools whose filter matches, then partition by binary presence on
    // $PATH so the AuditResult shape matches subprocess mode end-to-end:
    // tools_missing names tools we did not run, and `results` excludes them.
    // Without this skip, downstream consumers seeing "git-foo" in
    // tools_missing would still find a `results` entry for it in inprocess
    // mode but not in subprocess mode.
    let matched_specs: Vec<_> = TOOL_SPECS
        .iter()
        .filter(|spec| {
            tool_filter
                .map(|f| f.iter().any(|entry| matches_filter(spec.binary, entry)))
                .unwrap_or(true)
        })
        .collect();

    let mut specs: Vec<&ToolSpec> = Vec::new();
    let mut tools_found: Vec<String> = Vec::new();
    let mut tools_missing: Vec<String> = Vec::new();
    for spec in matched_specs {
        if which::which(spec.binary).is_ok() {
            tools_found.push(spec.binary.to_string());
            specs.push(spec);
        } else {
            tools_missing.push(spec.binary.to_string());
        }
    }

    // Create per-tool spinners under a MultiProgress container.
    let mp = progress.multi();
    let spinners: Vec<ProgressBar> = specs
        .iter()
        .map(|spec| match mp.as_ref() {
            Some(mp) => {
                let pb = mp.add(ProgressBar::new_spinner());
                pb.set_style(ProgressStyle::with_template("{spinner:.cyan} {msg}").unwrap());
                pb.set_message(format!("Scanning {}...", spec.item_noun));
                pb.enable_steady_tick(Duration::from_millis(100));
                pb
            }
            None => ProgressBar::hidden(),
        })
        .collect();

    // Per-tool forwarding progress: sub-tool bars appear beneath each spinner.
    // When progress is disabled, falls back to hidden bars.
    let sub_progresses: Vec<Progress> = spinners
        .iter()
        .map(|spinner| match mp.as_ref() {
            Some(mp) => Progress::forwarding(mp, spinner),
            None => Progress::disabled(),
        })
        .collect();

    // Run all tool scans concurrently, each returning (index, result).
    let mut indexed_results: Vec<(usize, ToolResult)> = thread::scope(|s| {
        let handles: Vec<_> = specs
            .iter()
            .enumerate()
            .map(|(idx, spec)| {
                let spinners = &spinners;
                let caching = &caching;
                let repo_paths = &repo_paths;
                let noise_patterns = &noise_patterns;
                let sub_progresses = &sub_progresses;
                s.spawn(move || {
                    let result = run_tool_scan(
                        spec,
                        caching,
                        directory,
                        repo_paths,
                        noise_patterns,
                        verbose,
                        &sub_progresses[idx],
                    );
                    spinners[idx].finish_with_message(format_spinner_done(&result));
                    (idx, result)
                })
            })
            .collect();

        // Panic isolation: if a single tool scan panics, synthesize an error
        // ToolResult for it rather than crashing the entire audit. The other
        // tools' results are still useful, and the user can see exactly which
        // tool failed.
        handles
            .into_iter()
            .enumerate()
            .map(|(idx, h)| match h.join() {
                Ok(r) => r,
                Err(panic) => {
                    let msg = panic_message(panic.as_ref());
                    (
                        idx,
                        ToolResult {
                            name: specs[idx].binary.to_string(),
                            item_noun: specs[idx].item_noun.to_string(),
                            total: 0,
                            counts: BTreeMap::new(),
                            error: Some(format!("panicked: {msg}")),
                        },
                    )
                }
            })
            .collect()
    });

    // Restore original TOOL_SPECS ordering.
    indexed_results.sort_by_key(|(idx, _)| *idx);
    let results = indexed_results.into_iter().map(|(_, r)| r).collect();

    AuditResult {
        directory: directory.to_path_buf(),
        tools_found,
        tools_missing,
        results,
    }
}

/// Best-effort extraction of a panic message from `JoinHandle::join`'s
/// `Box<dyn Any + Send>` payload. `panic!("literal")` produces a `&'static str`
/// payload; `panic!(format!(...))` produces a `String`. Anything else falls
/// back to a generic label.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
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

/// Convert a tool scan/lint `Result` into a `ToolResult`, reading the result's
/// generic `Counts`. Handles the Ok→counts and Err→error paths uniformly.
///
/// Every scan-shaped tool now exposes its summary as `git_tidy_core::counts::Counts`,
/// keyed by the same classification labels the audit emits as JSON keys, so each
/// `get_counts` accessor is simply `|r| &r.counts`. `Counts` only ever holds
/// non-zero buckets, so no `> 0` filtering is needed.
fn scan_to_result<T>(
    scan_result: Result<T, git_tidy_core::error::Error>,
    spec: &ToolSpec,
    get_counts: impl FnOnce(&T) -> &Counts,
) -> ToolResult {
    match scan_result {
        Ok(r) => {
            let counts: BTreeMap<String, usize> = get_counts(&r)
                .iter()
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
            |r| &r.counts,
        ),
        "git-branch-tidy" => scan_to_result(
            git_branch_tidy::scan::run_scan_repos(
                git,
                repo_paths,
                DEFAULT_BEHIND_THRESHOLD,
                verbose,
                &filter,
                false, // exclude remote-only branches from audit
                progress,
            ),
            spec,
            |r| &r.counts,
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
            |r| &r.counts,
        ),
        "git-remote-tidy" => scan_to_result(
            git_remote_tidy::scan::run_scan_repos(
                git, repo_paths, false, verbose, &filter, progress,
            ),
            spec,
            |r| &r.counts,
        ),
        "git-tag-tidy" => scan_to_result(
            git_tag_tidy::scan::run_scan_repos(git, repo_paths, false, verbose, &filter, progress),
            spec,
            |r| &r.counts,
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
            |r| &r.counts,
        ),
        "git-config-tidy" => scan_to_result(
            git_config_tidy::lint::run_lint_repos(git, repo_paths, verbose, progress),
            spec,
            |r| &r.counts,
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
            |r| &r.counts,
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
        // The `("landed", 0)` pair is dropped by `from_pairs` (it never increments),
        // so the bucket is absent from `Counts` and therefore from the audit map —
        // exercising the zero-omission contract end to end through `scan_to_result`.
        let counts = Counts::from_pairs(&[("active", 5), ("landed", 0), ("stale", 2)]);
        let ok: Result<Counts, git_tidy_core::error::Error> = Ok(counts);
        let spec = spec_for("git-branch-tidy");
        let result = scan_to_result(ok, spec, |c| c);
        assert_eq!(result.counts.len(), 2);
        assert_eq!(result.counts["active"], 5);
        assert_eq!(result.counts["stale"], 2);
        assert!(!result.counts.contains_key("landed"));
        assert_eq!(result.total, 7);
        assert!(result.error.is_none());
    }

    #[test]
    fn scan_to_result_captures_error() {
        let err: Result<Counts, _> = Err(git_tidy_core::error::Error::DirectoryNotFound {
            path: "/test".into(),
        });
        let spec = spec_for("git-branch-tidy");
        let result = scan_to_result(err, spec, |c| c);
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
    fn parallel_tool_scans_preserve_order() {
        let mock = MockGitBuilder::new().build();
        let p = Progress::disabled();
        let specs: Vec<_> = ["git-branch-tidy", "git-stash-tidy"]
            .iter()
            .map(|name| spec_for(name))
            .collect();

        let mut indexed: Vec<(usize, ToolResult)> = std::thread::scope(|s| {
            let handles: Vec<_> = specs
                .iter()
                .enumerate()
                .map(|(idx, spec)| {
                    let mock = &mock;
                    let p = &p;
                    s.spawn(move || {
                        let result = run_tool_scan(
                            spec,
                            mock,
                            Path::new("/nonexistent"),
                            &[],
                            &[],
                            false,
                            p,
                        );
                        (idx, result)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        indexed.sort_by_key(|(idx, _)| *idx);
        let results: Vec<_> = indexed.into_iter().map(|(_, r)| r).collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "git-branch-tidy");
        assert_eq!(results[1].name, "git-stash-tidy");
    }

    #[test]
    fn spinner_done_with_counts() {
        let result = ToolResult {
            name: "git-branch-tidy".to_string(),
            item_noun: "branches".to_string(),
            total: 12,
            counts: BTreeMap::from([("active".to_string(), 9), ("landed".to_string(), 3)]),
            error: None,
        };
        assert_eq!(
            format_spinner_done(&result),
            "✓ branches: 12 scanned (9 active, 3 landed)"
        );
    }

    #[test]
    fn spinner_done_zero_counts() {
        let result = ToolResult {
            name: "git-lfs-tidy".to_string(),
            item_noun: "LFS files".to_string(),
            total: 0,
            counts: BTreeMap::new(),
            error: None,
        };
        assert_eq!(format_spinner_done(&result), "✓ LFS files: 0 scanned");
    }

    #[test]
    fn spinner_done_error() {
        let result = ToolResult {
            name: "git-branch-tidy".to_string(),
            item_noun: "branches".to_string(),
            total: 0,
            counts: BTreeMap::new(),
            error: Some("process failed".to_string()),
        };
        assert_eq!(
            format_spinner_done(&result),
            "✗ branches: error: process failed"
        );
    }

    #[test]
    fn spinner_done_single_count() {
        let result = ToolResult {
            name: "git-remote-tidy".to_string(),
            item_noun: "remotes".to_string(),
            total: 1,
            counts: BTreeMap::from([("unreachable".to_string(), 1)]),
            error: None,
        };
        assert_eq!(
            format_spinner_done(&result),
            "✓ remotes: 1 scanned (1 unreachable)"
        );
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

    // -- panic_message tests --

    #[test]
    fn panic_message_handles_static_str_panic() {
        let result = std::thread::spawn(|| panic!("boom")).join();
        let payload = result.unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "boom");
    }

    #[test]
    fn panic_message_handles_string_panic() {
        let result = std::thread::spawn(|| panic!("{}", "dynamic".to_string())).join();
        let payload = result.unwrap_err();
        assert_eq!(panic_message(payload.as_ref()), "dynamic");
    }

    #[test]
    fn panic_message_handles_unknown_payload() {
        // Construct a non-string panic payload directly.
        let payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(panic_message(payload.as_ref()), "non-string panic payload");
    }
}
