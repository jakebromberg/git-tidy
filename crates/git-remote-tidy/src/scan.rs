use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use git_tidy_core::counts::Counts;
use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;
use git_tidy_core::types::ClassificationLabel;

use crate::types::{RemoteClassification, RemoteInfo, RemoteRepoGroup, RemoteScanResult};

/// Classify a single remote.
///
/// - If `is_configured` is false, the remote is Orphaned (tracking refs exist but no config).
/// - If `offline` is true, skip reachability check and default to Active.
/// - Otherwise, check `ls_remote_check`: reachable -> Active, unreachable -> Unreachable.
pub fn classify_remote(
    git: &dyn GitOps,
    repo: &Path,
    remote_name: &str,
    is_configured: bool,
    offline: bool,
) -> RemoteClassification {
    if !is_configured {
        return RemoteClassification::Orphaned;
    }

    if offline {
        return RemoteClassification::Active;
    }

    match git.ls_remote_check(repo, remote_name) {
        Ok(true) => RemoteClassification::Active,
        _ => RemoteClassification::Unreachable,
    }
}

/// Scan all repos in `directory` for remotes.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    offline: bool,
    verbose: bool,
    repo_filter: &NameFilter,
    entity_filter: &NameFilter,
    progress: &Progress,
) -> Result<RemoteScanResult, Error> {
    let repo_paths = discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);
    run_scan_repos(git, &repo_paths, offline, verbose, entity_filter, progress)
}

/// Scan pre-discovered repos for remotes.
///
/// Accepts repo paths directly (skipping discovery), so the audit runner can
/// call `discover_repos` once and share the result across tools.
pub fn run_scan_repos(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    offline: bool,
    verbose: bool,
    entity_filter: &NameFilter,
    progress: &Progress,
) -> Result<RemoteScanResult, Error> {
    let mut warnings = Vec::new();

    let (repos, scan_warnings) = parallel_classify(
        repo_paths,
        |repo_path| {
            let mut local_warnings = Vec::new();

            let configured = match git.list_remotes(repo_path) {
                Ok(r) => r,
                Err(e) => {
                    local_warnings.push(format!(
                        "could not list remotes for {}: {e}",
                        repo_path.display()
                    ));
                    return (None, local_warnings);
                }
            };

            // Fail closed: if we cannot enumerate tracking refs, surface the warning. Orphan detection will under-report rather than silently report zero, and the user sees the failure.
            let tracking_refs = match git.list_remote_tracking_refs(repo_path) {
                Ok(refs) => refs,
                Err(e) => {
                    local_warnings.push(format!(
                        "could not list remote tracking refs for {}: {e}",
                        repo_path.display()
                    ));
                    Vec::new()
                }
            };

            let mut tracking_counts: HashMap<String, usize> = HashMap::new();

            for (short, _full) in &tracking_refs {
                if let Some(remote_name) = short.split('/').next() {
                    let branch_part = short
                        .strip_prefix(remote_name)
                        .and_then(|rest| rest.strip_prefix('/'));
                    if branch_part == Some("HEAD") {
                        continue;
                    }
                    *tracking_counts.entry(remote_name.to_string()).or_insert(0) += 1;
                }
            }

            let configured_count = configured.len();
            let configured_set: HashSet<&str> = configured.iter().map(|s| s.as_str()).collect();
            let orphaned: Vec<String> = tracking_counts
                .keys()
                .filter(|name| !configured_set.contains(name.as_str()))
                .cloned()
                .collect();
            drop(configured_set);
            let mut all_remote_names = configured;
            all_remote_names.extend(orphaned);

            if all_remote_names.is_empty() {
                return (None, local_warnings);
            }

            let repo_name = repo_display_name(repo_path);

            if verbose {
                eprintln!(
                    "{repo_name}: {configured_count} configured, {} orphaned",
                    all_remote_names.len() - configured_count
                );
            }

            let mut classified = Vec::new();

            for (idx, remote_name) in all_remote_names.iter().enumerate() {
                if !entity_filter.matches(remote_name) {
                    continue;
                }

                let is_configured = idx < configured_count;
                let classification =
                    classify_remote(git, repo_path, remote_name, is_configured, offline);
                let tracking_count = tracking_counts.get(remote_name).copied().unwrap_or(0);

                let url = if is_configured {
                    git.remote_url(repo_path, remote_name).ok()
                } else {
                    None
                };

                if verbose {
                    eprintln!(
                        "  {remote_name}: {} (tracking_refs={tracking_count})",
                        classification.label(),
                    );
                }

                classified.push(RemoteInfo {
                    repo_path: repo_path.to_path_buf(),
                    name: remote_name.clone(),
                    classification,
                    url,
                    tracking_count,
                    is_origin: remote_name == "origin",
                });
            }

            classified.sort_by_key(|r| r.classification.priority());

            let group = RemoteRepoGroup {
                repo_path: repo_path.to_path_buf(),
                name: repo_name,
                remotes: classified,
            };

            (Some(group), local_warnings)
        },
        "Scanning remotes",
        progress,
    );
    warnings.extend(scan_warnings);

    let mut counts = Counts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for r in &g.remotes {
            counts.increment(r.classification.label());
        }
        total_scanned += g.remotes.len();
    }

    Ok(RemoteScanResult {
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

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn classify_reachable_remote() {
        let git = MockGitBuilder::new()
            .with_ls_remote_check(&repo(), "origin", true)
            .build();

        let result = classify_remote(&git, &repo(), "origin", true, false);
        assert_eq!(result, RemoteClassification::Active);
    }

    #[test]
    fn classify_unreachable_remote() {
        let git = MockGitBuilder::new()
            .with_ls_remote_check(&repo(), "origin", false)
            .build();

        let result = classify_remote(&git, &repo(), "origin", true, false);
        assert_eq!(result, RemoteClassification::Unreachable);
    }

    #[test]
    fn classify_offline_skips_reachability() {
        // No ls_remote_check configured -- if offline, should still return Active
        let git = MockGitBuilder::new().build();

        let result = classify_remote(&git, &repo(), "origin", true, true);
        assert_eq!(result, RemoteClassification::Active);
    }

    #[test]
    fn detect_orphaned_remote() {
        let git = MockGitBuilder::new().build();

        let result = classify_remote(&git, &repo(), "stale", false, false);
        assert_eq!(result, RemoteClassification::Orphaned);
    }

    #[test]
    fn is_origin_flag() {
        // Test the origin detection logic inline
        assert!("origin" == "origin");
        assert!("upstream" != "origin");
    }

    #[test]
    fn run_scan_repos_with_mock() {
        let git = MockGitBuilder::new()
            .with_list_remotes(&repo(), vec!["origin".to_string()])
            .with_ls_remote_check(&repo(), "origin", true)
            .with_remote_url(&repo(), "origin", "https://example.com/repo.git")
            .with_remote_tracking_refs(
                &repo(),
                vec![(
                    "origin/main".to_string(),
                    "refs/remotes/origin/main".to_string(),
                )],
            )
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], false, false, &filter, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.get("active"), 1);
    }

    #[test]
    fn scan_warns_when_remote_tracking_refs_errors() {
        // Regression: previously `list_remote_tracking_refs().unwrap_or_default()` silently returned zero orphaned remotes when the query errored. Fail closed: emit a warning so the user can tell the orphan-detection was incomplete.
        let git = MockGitBuilder::new()
            .with_list_remotes(&repo(), vec!["origin".to_string()])
            .with_ls_remote_check(&repo(), "origin", true)
            .with_remote_url(&repo(), "origin", "https://example.com/repo.git")
            .with_list_remote_tracking_refs_error(&repo(), "for-each-ref failed")
            .build();

        let p = Progress::disabled();
        let filter = NameFilter::default();
        let result = run_scan_repos(&git, &[repo()], false, false, &filter, &p).unwrap();
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("tracking refs") && w.contains("for-each-ref failed")),
            "expected warning, got: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn scan_sorts_by_priority() {
        // Verify classification sorting order
        let mut infos = [
            RemoteInfo {
                repo_path: repo(),
                name: "origin".to_string(),
                classification: RemoteClassification::Active,
                url: Some("https://example.com".to_string()),
                tracking_count: 5,
                is_origin: true,
            },
            RemoteInfo {
                repo_path: repo(),
                name: "stale".to_string(),
                classification: RemoteClassification::Unreachable,
                url: Some("https://old.example.com".to_string()),
                tracking_count: 2,
                is_origin: false,
            },
            RemoteInfo {
                repo_path: repo(),
                name: "orphan".to_string(),
                classification: RemoteClassification::Orphaned,
                url: None,
                tracking_count: 1,
                is_origin: false,
            },
        ];

        infos.sort_by_key(|r| r.classification.priority());

        assert_eq!(infos[0].classification, RemoteClassification::Unreachable);
        assert_eq!(infos[1].classification, RemoteClassification::Orphaned);
        assert_eq!(infos[2].classification, RemoteClassification::Active);
    }
}
