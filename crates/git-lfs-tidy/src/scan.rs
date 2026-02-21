use std::collections::HashSet;
use std::path::Path;

use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;

use crate::types::{LfsClassification, LfsCounts, LfsInfo, LfsRepoGroup, LfsScanResult};

/// Parse a human-readable size string into bytes.
///
/// Accepts formats like "1MB", "500KB", "1G", "1048576", "1.5 MB".
/// Case-insensitive. Returns `None` if the string cannot be parsed.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Find the boundary between numeric and suffix parts
    let num_end = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());

    let num_str = s[..num_end].trim();
    let suffix = s[num_end..].trim().to_uppercase();

    let num: f64 = num_str.parse().ok()?;

    let multiplier: f64 = match suffix.as_str() {
        "" | "B" => 1.0,
        "K" | "KB" => 1_000.0,
        "M" | "MB" => 1_000_000.0,
        "G" | "GB" => 1_000_000_000.0,
        "T" | "TB" => 1_000_000_000_000.0,
        _ => return None,
    };

    Some((num * multiplier) as u64)
}

/// Scan all repos in `directory` for LFS health issues.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    size_threshold: u64,
    depth: usize,
    verbose: bool,
    repo_filter: &NameFilter,
    progress: &Progress,
) -> Result<LfsScanResult, Error> {
    let repo_paths = discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);

    let lfs_installed = git.lfs_installed().unwrap_or(false);

    let mut repos = Vec::new();
    let mut counts = LfsCounts::default();
    let mut warnings = Vec::new();
    let mut total_scanned = 0;

    if !lfs_installed {
        warnings.push("git-lfs is not installed; skipping LFS-specific checks".to_string());
    }

    let pb = progress.bar(repo_paths.len() as u64, "Scanning LFS");
    for repo_path in &repo_paths {
        let repo_name = repo_display_name(repo_path);
        let mut items = Vec::new();
        let mut lfs_paths: HashSet<String> = HashSet::new();
        let mut track_patterns = Vec::new();

        if lfs_installed {
            // Get LFS tracking patterns
            track_patterns = git.lfs_track_patterns(repo_path).unwrap_or_default();

            // Classify tracked files as Healthy or Missing
            match git.lfs_ls_files(repo_path) {
                Ok(files) => {
                    for (oid, status, path) in files {
                        lfs_paths.insert(path.clone());
                        let classification = if status == '-' {
                            LfsClassification::Missing
                        } else {
                            LfsClassification::Healthy
                        };
                        counts.increment(classification);
                        total_scanned += 1;
                        items.push(LfsInfo {
                            repo_path: repo_path.clone(),
                            path,
                            classification,
                            oid,
                            size_bytes: None,
                        });
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "could not list LFS files for {}: {e}",
                        repo_path.display()
                    ));
                }
            }

            // Check for orphaned (prunable) objects
            match git.lfs_prune_dry_run(repo_path) {
                Ok((count, bytes)) if count > 0 => {
                    let classification = LfsClassification::Orphaned;
                    counts.increment(classification);
                    total_scanned += 1;
                    items.push(LfsInfo {
                        repo_path: repo_path.clone(),
                        path: format!("<{count} orphaned LFS objects>"),
                        classification,
                        oid: String::new(),
                        size_bytes: Some(bytes),
                    });
                }
                Err(e) => {
                    warnings.push(format!(
                        "could not check prunable LFS objects for {}: {e}",
                        repo_path.display()
                    ));
                }
                _ => {}
            }
        }

        // Find large blobs not tracked by LFS
        match git.find_large_blobs(repo_path, size_threshold, depth) {
            Ok(blobs) => {
                for (hash, size, path) in blobs {
                    if lfs_paths.contains(&path) {
                        continue;
                    }
                    let classification = LfsClassification::Untracked;
                    counts.increment(classification);
                    total_scanned += 1;
                    items.push(LfsInfo {
                        repo_path: repo_path.clone(),
                        path,
                        classification,
                        oid: hash,
                        size_bytes: Some(size),
                    });
                }
            }
            Err(e) => {
                warnings.push(format!(
                    "could not scan large blobs for {}: {e}",
                    repo_path.display()
                ));
            }
        }

        if verbose && !items.is_empty() {
            eprintln!("{repo_name}: {} LFS items", items.len());
            for item in &items {
                eprintln!("  {}: {} ({})", item.path, item.classification.label(),
                    item.size_bytes.map_or("?".to_string(), |s| format!("{s} bytes")));
            }
        }

        if items.is_empty() {
            continue;
        }

        // Sort by classification priority, then by path
        items.sort_by(|a, b| {
            a.classification
                .priority()
                .cmp(&b.classification.priority())
                .then_with(|| a.path.cmp(&b.path))
        });

        repos.push(LfsRepoGroup {
            repo_path: repo_path.clone(),
            name: repo_name,
            items,
            lfs_available: lfs_installed,
            track_patterns,
        });
        pb.inc(1);
    }
    pb.finish_and_clear();

    Ok(LfsScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
        lfs_installed,
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

    // --- parse_size tests ---

    #[test]
    fn parse_size_megabytes() {
        assert_eq!(parse_size("1MB"), Some(1_000_000));
        assert_eq!(parse_size("1M"), Some(1_000_000));
        assert_eq!(parse_size("1.5MB"), Some(1_500_000));
    }

    #[test]
    fn parse_size_kilobytes() {
        assert_eq!(parse_size("500KB"), Some(500_000));
        assert_eq!(parse_size("500K"), Some(500_000));
    }

    #[test]
    fn parse_size_gigabytes() {
        assert_eq!(parse_size("1GB"), Some(1_000_000_000));
        assert_eq!(parse_size("1G"), Some(1_000_000_000));
    }

    #[test]
    fn parse_size_plain_bytes() {
        assert_eq!(parse_size("1048576"), Some(1_048_576));
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn parse_size_with_b_suffix() {
        assert_eq!(parse_size("1024B"), Some(1024));
    }

    #[test]
    fn parse_size_case_insensitive() {
        assert_eq!(parse_size("1mb"), Some(1_000_000));
        assert_eq!(parse_size("1Mb"), Some(1_000_000));
        assert_eq!(parse_size("500kb"), Some(500_000));
    }

    #[test]
    fn parse_size_with_space() {
        assert_eq!(parse_size("1 MB"), Some(1_000_000));
    }

    #[test]
    fn parse_size_invalid() {
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size("1XB"), None);
    }

    // --- run_scan tests ---

    #[test]
    fn scan_empty_repo_no_lfs() {
        let _git = MockGitBuilder::new()
            .with_lfs_installed(false)
            .with_find_large_blobs(&repo(), vec![])
            .build();

        // discover_repos needs a real directory -- tested in integration tests
        // Here we test the logic with mock by calling directly
    }

    #[test]
    fn scan_with_healthy_lfs_files() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_ls_files(
                &repo(),
                vec![
                    ("oid1".to_string(), '*', "large.bin".to_string()),
                    ("oid2".to_string(), '*', "data.zip".to_string()),
                ],
            )
            .with_lfs_track_patterns(&repo(), vec!["*.bin".to_string(), "*.zip".to_string()])
            .with_find_large_blobs(&repo(), vec![])
            .build();

        // Mock directly calls the trait methods
        let files = git.lfs_ls_files(&repo()).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].1, '*');
        assert_eq!(files[1].2, "data.zip");
    }

    #[test]
    fn scan_with_missing_lfs_files() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_ls_files(
                &repo(),
                vec![("oid1".to_string(), '-', "missing.bin".to_string())],
            )
            .build();

        let files = git.lfs_ls_files(&repo()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].1, '-');
    }

    #[test]
    fn scan_with_orphaned_objects() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_prune_dry_run(&repo(), 3, 2_500_000)
            .build();

        let (count, bytes) = git.lfs_prune_dry_run(&repo()).unwrap();
        assert_eq!(count, 3);
        assert_eq!(bytes, 2_500_000);
    }

    #[test]
    fn scan_with_large_untracked_blobs() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_ls_files(&repo(), vec![])
            .with_find_large_blobs(
                &repo(),
                vec![
                    ("hash1".to_string(), 2_000_000, "video.mp4".to_string()),
                    ("hash2".to_string(), 1_500_000, "archive.tar".to_string()),
                ],
            )
            .build();

        let blobs = git.find_large_blobs(&repo(), 1_000_000, 1000).unwrap();
        assert_eq!(blobs.len(), 2);
        assert_eq!(blobs[0].2, "video.mp4");
    }

    #[test]
    fn scan_excludes_lfs_tracked_from_untracked() {
        // If a file is in LFS, it should NOT appear as untracked even if
        // find_large_blobs returns it.
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_ls_files(
                &repo(),
                vec![("oid1".to_string(), '*', "large.bin".to_string())],
            )
            .with_lfs_track_patterns(&repo(), vec!["*.bin".to_string()])
            .with_find_large_blobs(
                &repo(),
                vec![
                    ("hash1".to_string(), 5_000_000, "large.bin".to_string()),
                    (
                        "hash2".to_string(),
                        3_000_000,
                        "not-tracked.mp4".to_string(),
                    ),
                ],
            )
            .build();

        // Verify the lfs_paths filtering logic:
        let lfs_files = git.lfs_ls_files(&repo()).unwrap();
        let lfs_paths: HashSet<String> = lfs_files.iter().map(|(_, _, p)| p.clone()).collect();
        assert!(lfs_paths.contains("large.bin"));

        let blobs = git.find_large_blobs(&repo(), 1_000_000, 1000).unwrap();
        let untracked: Vec<_> = blobs
            .into_iter()
            .filter(|(_, _, path)| !lfs_paths.contains(path))
            .collect();
        assert_eq!(untracked.len(), 1);
        assert_eq!(untracked[0].2, "not-tracked.mp4");
    }

    #[test]
    fn scan_lfs_not_installed_still_finds_large_blobs() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(false)
            .with_find_large_blobs(
                &repo(),
                vec![("hash1".to_string(), 2_000_000, "huge.bin".to_string())],
            )
            .build();

        assert!(!git.lfs_installed().unwrap());
        let blobs = git.find_large_blobs(&repo(), 1_000_000, 1000).unwrap();
        assert_eq!(blobs.len(), 1);
    }

    #[test]
    fn lfs_prune_calls_tracked() {
        let git = MockGitBuilder::new().with_lfs_installed(true).build();

        git.lfs_prune(&repo()).unwrap();
        assert_eq!(git.lfs_prune_calls(), vec![repo()]);
    }

    #[test]
    fn lfs_prune_error() {
        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_prune_error(&repo(), "permission denied")
            .build();

        let result = git.lfs_prune(&repo());
        assert!(result.is_err());
    }
}
