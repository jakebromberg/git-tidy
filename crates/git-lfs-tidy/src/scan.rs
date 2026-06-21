use std::collections::HashSet;
use std::path::{Path, PathBuf};

use git_tidy_core::counts::Counts;
use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;

use crate::types::{LfsClassification, LfsInfo, LfsRepoGroup, LfsScanResult};

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

    // Reject negative and non-finite inputs explicitly. Without this check `--size-threshold -1MB` would compute `-1_000_000.0 as u64 = 0`, silently meaning "flag every blob".
    if !num.is_finite() || num < 0.0 {
        return None;
    }

    let multiplier: f64 = match suffix.as_str() {
        "" | "B" => 1.0,
        "K" | "KB" => 1_000.0,
        "M" | "MB" => 1_000_000.0,
        "G" | "GB" => 1_000_000_000.0,
        "T" | "TB" => 1_000_000_000_000.0,
        _ => return None,
    };

    let bytes = num * multiplier;
    // Reject overflow rather than silently saturating to u64::MAX, so a typo like `99999999999999999999TB` fails parsing instead of pretending to be valid.
    if !bytes.is_finite() || bytes > u64::MAX as f64 {
        return None;
    }
    Some(bytes as u64)
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
    run_scan_repos(git, &repo_paths, size_threshold, depth, verbose, progress)
}

/// Scan the given repo paths for LFS health issues.
pub fn run_scan_repos(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    size_threshold: u64,
    depth: usize,
    verbose: bool,
    progress: &Progress,
) -> Result<LfsScanResult, Error> {
    let lfs_installed = git.lfs_installed().unwrap_or(false);

    let mut warnings = Vec::new();

    if !lfs_installed {
        warnings.push("git-lfs is not installed; skipping LFS-specific checks".to_string());
    }

    let (repos, scan_warnings) = parallel_classify(
        repo_paths,
        |repo_path| {
            let repo_name = repo_display_name(repo_path);
            let mut local_warnings = Vec::new();
            let mut items = Vec::new();
            let mut lfs_paths: HashSet<String> = HashSet::new();
            let mut track_patterns = Vec::new();

            if lfs_installed {
                track_patterns = git.lfs_track_patterns(repo_path).unwrap_or_default();

                match git.lfs_ls_files(repo_path) {
                    Ok(files) => {
                        for (oid, status, path) in files {
                            lfs_paths.insert(path.clone());
                            let classification = if status == '-' {
                                LfsClassification::Missing
                            } else {
                                LfsClassification::Healthy
                            };
                            items.push(LfsInfo {
                                repo_path: repo_path.to_path_buf(),
                                path,
                                classification,
                                oid,
                                size_bytes: None,
                            });
                        }
                    }
                    Err(e) => {
                        local_warnings.push(format!(
                            "could not list LFS files for {}: {e}",
                            repo_path.display()
                        ));
                    }
                }

                match git.lfs_prune_dry_run(repo_path) {
                    Ok((count, bytes)) if count > 0 => {
                        items.push(LfsInfo {
                            repo_path: repo_path.to_path_buf(),
                            path: format!("<{count} orphaned LFS objects>"),
                            classification: LfsClassification::Orphaned,
                            oid: String::new(),
                            size_bytes: Some(bytes),
                        });
                    }
                    Err(e) => {
                        local_warnings.push(format!(
                            "could not check prunable LFS objects for {}: {e}",
                            repo_path.display()
                        ));
                    }
                    _ => {}
                }
            }

            match git.find_large_blobs(repo_path, size_threshold, depth) {
                Ok(blobs) => {
                    for (hash, size, path) in blobs {
                        if lfs_paths.contains(&path) {
                            continue;
                        }
                        items.push(LfsInfo {
                            repo_path: repo_path.to_path_buf(),
                            path,
                            classification: LfsClassification::Untracked,
                            oid: hash,
                            size_bytes: Some(size),
                        });
                    }
                }
                Err(e) => {
                    local_warnings.push(format!(
                        "could not scan large blobs for {}: {e}",
                        repo_path.display()
                    ));
                }
            }

            if verbose && !items.is_empty() {
                eprintln!("{repo_name}: {} LFS items", items.len());
                for item in &items {
                    eprintln!(
                        "  {}: {} ({})",
                        item.path,
                        item.classification.label(),
                        item.size_bytes
                            .map_or("?".to_string(), |s| format!("{s} bytes"))
                    );
                }
            }

            if items.is_empty() {
                return (None, local_warnings);
            }

            items.sort_by(|a, b| {
                a.classification
                    .priority()
                    .cmp(&b.classification.priority())
                    .then_with(|| a.path.cmp(&b.path))
            });

            let group = LfsRepoGroup {
                repo_path: repo_path.to_path_buf(),
                name: repo_name,
                items,
                lfs_available: lfs_installed,
                track_patterns,
            };

            (Some(group), local_warnings)
        },
        "Scanning LFS",
        progress,
    );
    warnings.extend(scan_warnings);

    let mut counts = Counts::default();
    let mut total_scanned = 0;
    for g in &repos {
        for item in &g.items {
            counts.increment(item.classification.label());
        }
        total_scanned += g.items.len();
    }

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

    #[test]
    fn parse_size_rejects_negative() {
        // Regression: `--size-threshold -1MB` previously parsed as `-1_000_000.0 as u64 = 0`, silently meaning "flag every blob". Negatives must be rejected so the CLI surfaces an error.
        assert_eq!(parse_size("-1"), None);
        assert_eq!(parse_size("-1MB"), None);
        assert_eq!(parse_size("-0.5GB"), None);
    }

    #[test]
    fn parse_size_rejects_overflow() {
        // Regression: huge values like "9e30 TB" would previously cast to u64 and saturate to u64::MAX without telling the user. Reject so a typo cannot masquerade as a sane threshold.
        assert!(parse_size("99999999999999999999TB").is_none());
        // f64 NaN/inf paths via large mantissa-exponent combinations:
        assert!(parse_size("1e300TB").is_none());
    }

    // --- run_scan tests ---

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

    #[test]
    fn run_scan_repos_with_mock() {
        use git_tidy_core::progress::Progress;

        let git = MockGitBuilder::new()
            .with_lfs_installed(true)
            .with_lfs_ls_files(
                &repo(),
                vec![("oid1".to_string(), '*', "large.bin".to_string())],
            )
            .with_lfs_track_patterns(&repo(), vec!["*.bin".to_string()])
            .with_lfs_prune_dry_run(&repo(), 0, 0)
            .with_find_large_blobs(&repo(), vec![])
            .build();

        let p = Progress::disabled();
        let result = run_scan_repos(&git, &[repo()], 1_000_000, 1000, false, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.get("healthy"), 1);
    }
}
