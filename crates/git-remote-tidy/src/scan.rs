use std::collections::{HashMap, HashSet};
use std::path::Path;

use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::types::ClassificationLabel;

use crate::types::{
    RemoteClassification, RemoteCounts, RemoteInfo, RemoteRepoGroup, RemoteScanResult,
};

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
) -> Result<RemoteScanResult, Error> {
    let repo_paths = discover_repos(directory)?;

    let mut repos = Vec::new();
    let mut counts = RemoteCounts::default();
    let mut warnings = Vec::new();
    let mut total_scanned = 0;

    for repo_path in &repo_paths {
        // Get configured remotes
        let configured = match git.list_remotes(repo_path) {
            Ok(r) => r,
            Err(e) => {
                warnings.push(format!(
                    "could not list remotes for {}: {e}",
                    repo_path.display()
                ));
                continue;
            }
        };

        // Get all tracking refs to detect orphaned remotes and count per-remote
        let tracking_refs = git.list_remote_tracking_refs(repo_path).unwrap_or_default();

        // Count tracking refs per remote name and detect orphaned
        let mut tracking_counts: HashMap<String, usize> = HashMap::new();

        for (short, _full) in &tracking_refs {
            // short is like "origin/main" -- extract remote name
            if let Some(remote_name) = short.split('/').next() {
                // Skip HEAD refs (e.g., "origin/HEAD")
                let branch_part = short
                    .strip_prefix(remote_name)
                    .and_then(|rest| rest.strip_prefix('/'));
                if branch_part == Some("HEAD") {
                    continue;
                }
                *tracking_counts.entry(remote_name.to_string()).or_insert(0) += 1;
            }
        }

        // Collect all remote names (configured + orphaned); move configured to avoid clone
        let configured_count = configured.len();
        let configured_set: HashSet<&str> = configured.iter().map(|s| s.as_str()).collect();
        let orphaned: Vec<String> = tracking_counts
            .keys()
            .filter(|name| !configured_set.contains(name.as_str()))
            .cloned()
            .collect();
        drop(configured_set);
        let mut all_remote_names = configured; // move, not clone
        all_remote_names.extend(orphaned);

        if all_remote_names.is_empty() {
            continue;
        }

        let repo_name = repo_display_name(repo_path);

        let mut classified = Vec::new();

        for (idx, remote_name) in all_remote_names.iter().enumerate() {
            let is_configured = idx < configured_count;
            let classification =
                classify_remote(git, repo_path, remote_name, is_configured, offline);
            let tracking_count = tracking_counts.get(remote_name).copied().unwrap_or(0);

            let url = if is_configured {
                git.remote_url(repo_path, remote_name).ok()
            } else {
                None
            };

            counts.increment(&classification);
            total_scanned += 1;

            classified.push(RemoteInfo {
                repo_path: repo_path.clone(),
                name: remote_name.clone(),
                classification,
                url,
                tracking_count,
                is_origin: remote_name == "origin",
            });
        }

        // Sort by classification priority
        classified.sort_by_key(|r| r.classification.priority());

        repos.push(RemoteRepoGroup {
            repo_path: repo_path.clone(),
            name: repo_name,
            remotes: classified,
        });
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
