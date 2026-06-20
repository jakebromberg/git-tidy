use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{CleanResult, FailedItem};

use crate::types::{RemoteClassification, RemoteScanResult};

/// Options controlling remote cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Allow removing the origin remote.
    pub force: bool,
    /// Include orphaned remotes (default: unreachable only).
    pub all: bool,
}

/// A remote that was successfully removed.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RemovedRemote {
    pub repo: PathBuf,
    pub name: String,
    /// Number of orphaned refs pruned (0 for configured remotes).
    pub refs_pruned: usize,
}

/// Run the clean operation on a scan result.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &RemoteScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<RemovedRemote>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    for group in &scan_result.repos {
        for remote in &group.remotes {
            if !should_clean(&remote.classification, options) {
                skipped += 1;
                continue;
            }

            // Origin safety check
            if remote.is_origin && !options.force {
                writeln!(
                    out,
                    "warning: skipping origin remote in {} (use --force to remove)",
                    group.name,
                )?;
                skipped += 1;
                continue;
            }

            if options.dry_run {
                let action = if remote.classification == RemoteClassification::Orphaned {
                    "would prune refs for"
                } else {
                    "would remove"
                };
                writeln!(out, "{action} {} in {}", remote.name, group.name)?;
                succeeded.push(RemovedRemote {
                    repo: group.repo_path.clone(),
                    name: remote.name.clone(),
                    refs_pruned: 0,
                });
                continue;
            }

            // Orphaned remotes: prune refs (no config to remove)
            if remote.classification == RemoteClassification::Orphaned {
                match git.prune_remote_refs(&group.repo_path, &remote.name) {
                    Ok(count) => {
                        writeln!(
                            out,
                            "pruned {count} refs for {} in {}",
                            remote.name, group.name,
                        )?;
                        succeeded.push(RemovedRemote {
                            repo: group.repo_path.clone(),
                            name: remote.name.clone(),
                            refs_pruned: count,
                        });
                    }
                    Err(e) => {
                        writeln!(out, "error: could not prune refs for {}: {e}", remote.name,)?;
                        failed.push(FailedItem {
                            repo: group.repo_path.clone(),
                            name: remote.name.clone(),
                            reason: e.to_string(),
                        });
                    }
                }
            } else {
                // Configured remotes: git remote remove
                match git.remote_remove(&group.repo_path, &remote.name) {
                    Ok(()) => {
                        writeln!(out, "removed {} in {}", remote.name, group.name)?;
                        succeeded.push(RemovedRemote {
                            repo: group.repo_path.clone(),
                            name: remote.name.clone(),
                            refs_pruned: 0,
                        });
                    }
                    Err(e) => {
                        writeln!(out, "error: could not remove {}: {e}", remote.name)?;
                        failed.push(FailedItem {
                            repo: group.repo_path.clone(),
                            name: remote.name.clone(),
                            reason: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(CleanResult {
        succeeded,
        failed,
        skipped,
    })
}

/// Determine if a remote should be cleaned based on its classification and options.
fn should_clean(classification: &RemoteClassification, options: &CleanOptions) -> bool {
    if options.all {
        // Unreachable + orphaned
        return *classification != RemoteClassification::Active;
    }

    // Default: unreachable only
    *classification == RemoteClassification::Unreachable
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::counts::Counts;
    use git_tidy_core::testutil::MockGitBuilder;
    use git_tidy_core::types::ClassificationLabel;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(remotes: Vec<RemoteInfo>) -> RemoteScanResult {
        let mut counts = Counts::default();
        for r in &remotes {
            counts.increment(r.classification.label());
        }
        RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                remotes,
            }],
            total_scanned: 0,
            counts,
            warnings: vec![],
        }
    }

    fn remote(name: &str, classification: RemoteClassification, is_origin: bool) -> RemoteInfo {
        RemoteInfo {
            repo_path: repo(),
            name: name.to_string(),
            classification,
            url: Some(format!("https://example.com/{name}.git")),
            tracking_count: 3,
            is_origin,
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            force: false,
            all: false,
        }
    }

    #[test]
    fn clean_removes_unreachable() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote(
            "stale",
            RemoteClassification::Unreachable,
            false,
        )]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "stale");
        assert_eq!(git.remote_remove_calls().len(), 1);
    }

    #[test]
    fn clean_skips_active_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote("origin", RemoteClassification::Active, true)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.remote_remove_calls().len(), 0);
    }

    #[test]
    fn clean_skips_orphaned_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote(
            "orphan",
            RemoteClassification::Orphaned,
            false,
        )]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_all_includes_orphaned() {
        let git = MockGitBuilder::new()
            .with_prune_remote_refs_result(&repo(), "orphan", 3)
            .build();
        let scan = make_scan_result(vec![
            remote("stale", RemoteClassification::Unreachable, false),
            remote("orphan", RemoteClassification::Orphaned, false),
            remote("good", RemoteClassification::Active, false),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        // Unreachable + orphaned removed, active skipped
        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_origin_requires_force() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote(
            "origin",
            RemoteClassification::Unreachable,
            true,
        )]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.remote_remove_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipping origin"));
        assert!(output.contains("--force"));
    }

    #[test]
    fn clean_origin_with_force() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![remote(
            "origin",
            RemoteClassification::Unreachable,
            true,
        )]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.remote_remove_calls().len(), 1);
    }

    #[test]
    fn clean_orphaned_prunes_refs() {
        let git = MockGitBuilder::new()
            .with_prune_remote_refs_result(&repo(), "stale", 5)
            .build();
        let scan = make_scan_result(vec![{
            let mut r = remote("stale", RemoteClassification::Orphaned, false);
            r.url = None;
            r
        }]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].refs_pruned, 5);
        // Should call prune_remote_refs, NOT remote_remove
        assert_eq!(git.prune_remote_refs_calls().len(), 1);
        assert_eq!(git.remote_remove_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("pruned 5 refs"));
    }

    #[test]
    fn clean_dry_run_makes_zero_calls() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            remote("stale", RemoteClassification::Unreachable, false),
            {
                let mut r = remote("orphan", RemoteClassification::Orphaned, false);
                r.url = None;
                r
            },
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.remote_remove_calls().len(), 0);
        assert_eq!(git.prune_remote_refs_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would remove stale"));
        assert!(output.contains("would prune refs for orphan"));
    }

    #[test]
    fn clean_handles_removal_failure() {
        let git = MockGitBuilder::new()
            .with_remote_remove_error(&repo(), "stale", "permission denied")
            .build();
        let scan = make_scan_result(vec![remote(
            "stale",
            RemoteClassification::Unreachable,
            false,
        )]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "stale");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not remove stale"));
    }
}
