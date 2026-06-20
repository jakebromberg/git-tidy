use std::path::{Path, PathBuf};
use std::process::Command;

use git_tidy_core::counts::Counts;
use git_tidy_core::date::days_since_iso_date;
use git_tidy_core::dirty::check_dirty;
use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::scan::parallel_classify;

use crate::types::{RepoClassification, RepoInfo, RepoScanResult};

/// Get disk usage in bytes for a directory using `du -sk`.
///
/// This is a standalone function (not on GitOps) because `du` is not a git operation.
/// Returns 0 on failure.
pub fn disk_usage(path: &Path) -> u64 {
    let output = Command::new("du")
        .args(["-sk", &path.to_string_lossy()])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.split_whitespace()
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .map(|kib| kib * 1024) // KiB to bytes
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Classify a single repository. Returns the classified info plus any non-fatal warnings encountered.
pub fn classify_repo(
    git: &dyn GitOps,
    repo_path: &Path,
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    du_fn: &dyn Fn(&Path) -> u64,
) -> (RepoInfo, Vec<String>) {
    let mut warnings = Vec::new();
    let name = repo_display_name(repo_path);

    // Last commit date and age
    let last_commit_date = git.last_commit_date(repo_path).unwrap_or(None);
    let last_commit_age_days = last_commit_date.as_deref().and_then(days_since_iso_date);

    // Dirty detection
    let dirty_result = check_dirty(git, repo_path, noise_patterns);
    let (is_dirty, dirty_file_count) = match dirty_result {
        Ok(dr) => (!dr.meaningful_files.is_empty(), dr.meaningful_files.len()),
        Err(_) => (false, 0),
    };

    // Branch count
    let branch_count = git
        .list_local_branches(repo_path)
        .map(|b| b.len())
        .unwrap_or(0);

    // Remote detection. Fail closed: if we cannot read the remote list, assume the repo has one and is reachable so it is not classified as Orphaned and rm -rf'd.
    let (remotes, remotes_readable) = match git.list_remotes(repo_path) {
        Ok(r) => (r, true),
        Err(e) => {
            warnings.push(format!(
                "could not list remotes for {}: {e} (treating as having a reachable remote to avoid deletion)",
                repo_path.display()
            ));
            (Vec::new(), false)
        }
    };
    let has_remote = !remotes.is_empty() || !remotes_readable;

    let remote_url = if !remotes.is_empty() {
        git.remote_url(repo_path, &remotes[0]).ok()
    } else {
        None
    };

    let any_reachable = if !remotes_readable {
        // Cannot probe — assume reachable so classification falls through to Active/Stale, not Orphaned.
        true
    } else if has_remote && !offline {
        remotes
            .iter()
            .any(|r| git.ls_remote_check(repo_path, r).unwrap_or(false))
    } else {
        // In offline mode, assume configured remotes are reachable
        has_remote && offline
    };

    // Disk usage
    let disk_usage_bytes = du_fn(repo_path);

    // Classification
    let classification = if !has_remote || !any_reachable {
        RepoClassification::Orphaned
    } else if let Some(age) = last_commit_age_days {
        if age >= stale_days {
            RepoClassification::Stale
        } else {
            RepoClassification::Active
        }
    } else {
        // No commits but has reachable remote -- treat as active (newly cloned?)
        RepoClassification::Active
    };

    let info = RepoInfo {
        path: repo_path.to_path_buf(),
        name,
        classification,
        last_commit_date,
        last_commit_age_days,
        disk_usage_bytes,
        remote_url,
        branch_count,
        has_remote,
        is_dirty,
        dirty_file_count,
    };
    (info, warnings)
}

/// Scan all repos in `directory` and classify them.
#[allow(clippy::too_many_arguments)]
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    verbose: bool,
    repo_filter: &NameFilter,
    progress: &Progress,
) -> Result<RepoScanResult, Error> {
    let repo_paths = discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);
    run_scan_repos(
        git,
        &repo_paths,
        stale_days,
        noise_patterns,
        offline,
        verbose,
        progress,
    )
}

/// Scan a pre-discovered list of repo paths and classify them.
#[allow(clippy::too_many_arguments)]
pub fn run_scan_repos(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    verbose: bool,
    progress: &Progress,
) -> Result<RepoScanResult, Error> {
    run_scan_repos_with_du(
        git,
        repo_paths,
        stale_days,
        noise_patterns,
        offline,
        verbose,
        &disk_usage,
        progress,
    )
}

/// Scan a pre-discovered list of repo paths with an injectable disk-usage function.
#[allow(clippy::too_many_arguments)]
pub fn run_scan_repos_with_du(
    git: &dyn GitOps,
    repo_paths: &[PathBuf],
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    verbose: bool,
    du_fn: &(dyn Fn(&Path) -> u64 + Sync),
    progress: &Progress,
) -> Result<RepoScanResult, Error> {
    let (mut repos, warnings) = parallel_classify(
        repo_paths,
        |repo_path| {
            let (info, classify_warnings) =
                classify_repo(git, repo_path, stale_days, noise_patterns, offline, du_fn);
            if verbose {
                eprintln!(
                    "{}: {} (age={}d, remote={}, dirty={})",
                    info.name,
                    info.classification.label(),
                    info.last_commit_age_days
                        .map_or("?".to_string(), |d| d.to_string()),
                    info.has_remote,
                    info.is_dirty,
                );
            }
            (Some(info), classify_warnings)
        },
        "Scanning repos",
        progress,
    );

    // Post-processing: compute counts and disk usage totals.
    let mut counts = Counts::default();
    // `dirty` is a cross-cutting count (repos with meaningful uncommitted changes),
    // not a classification bucket, so it lives on the result rather than in `Counts`.
    let mut dirty = 0usize;
    let mut total_disk_usage_bytes = 0u64;
    let mut reclaimable_bytes = 0u64;
    for info in &repos {
        counts.increment(info.classification.label());
        if info.is_dirty {
            dirty += 1;
        }
        total_disk_usage_bytes += info.disk_usage_bytes;
        if matches!(
            info.classification,
            RepoClassification::Stale | RepoClassification::Orphaned
        ) {
            reclaimable_bytes += info.disk_usage_bytes;
        }
    }

    // Sort by classification priority (stale first, then orphaned, then active)
    repos.sort_by(|a, b| {
        a.classification
            .priority()
            .cmp(&b.classification.priority())
            .then_with(|| a.name.cmp(&b.name))
    });

    let total_scanned = repos.len();

    Ok(RepoScanResult {
        repos,
        total_scanned,
        counts,
        dirty,
        warnings,
        total_disk_usage_bytes,
        reclaimable_bytes,
    })
}

#[cfg(test)]
mod tests {
    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repos/my-project")
    }

    fn no_du(_path: &Path) -> u64 {
        0
    }

    fn fixed_du(bytes: u64) -> impl Fn(&Path) -> u64 {
        move |_| bytes
    }

    fn today_iso() -> String {
        // Use a date that's definitely today
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let days = now / 86400;
        // civil_from_days equivalent (simplified)
        let z = days as i64 + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}T12:00:00+00:00")
    }

    fn old_iso() -> String {
        "2020-01-01T12:00:00+00:00".to_string()
    }

    #[test]
    fn classify_active_repo() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Active);
        assert_eq!(info.name, "my-project");
        assert!(info.has_remote);
        assert!(!info.is_dirty);
        assert_eq!(info.branch_count, 1);
    }

    #[test]
    fn classify_stale_repo() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&old_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Stale);
    }

    #[test]
    fn classify_orphaned_no_remote() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec![])
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Orphaned);
        assert!(!info.has_remote);
    }

    #[test]
    fn classify_orphaned_unreachable_remote() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", false)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Orphaned);
        assert!(info.has_remote); // has remote config, just not reachable
    }

    #[test]
    fn classify_dirty_stale() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&old_iso()))
            .with_status_porcelain(
                &r,
                vec![" M src/main.rs".to_string(), "?? new.txt".to_string()],
            )
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Stale);
        assert!(info.is_dirty);
        assert_eq!(info.dirty_file_count, 2);
    }

    #[test]
    fn classify_dirty_orphaned() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![" M README.md".to_string()])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec![])
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Orphaned);
        assert!(info.is_dirty);
        assert_eq!(info.dirty_file_count, 1);
    }

    #[test]
    fn classify_dirty_active() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec!["?? draft.txt".to_string()])
            .with_local_branches(&r, vec!["main".to_string(), "feature".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_eq!(info.classification, RepoClassification::Active);
        assert!(info.is_dirty);
        assert_eq!(info.branch_count, 2);
    }

    #[test]
    fn classify_offline_mode_assumes_reachable() {
        let r = repo();
        // No ls_remote_check configured -- offline mode should still yield Active
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], true, &no_du);

        assert_eq!(info.classification, RepoClassification::Active);
    }

    #[test]
    fn classify_empty_repo_with_remote() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, None)
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec![])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        // Empty repo with reachable remote -> active (newly cloned)
        assert_eq!(info.classification, RepoClassification::Active);
        assert!(info.last_commit_date.is_none());
    }

    #[test]
    fn classify_uses_du_fn() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let du = fixed_du(142 * 1024 * 1024);
        let (info, _warnings) = classify_repo(&git, &r, 180, &[], false, &du);

        assert_eq!(info.disk_usage_bytes, 142 * 1024 * 1024);
    }

    #[test]
    fn run_scan_repos_with_du_mock() {
        use git_tidy_core::progress::Progress;

        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec![])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let p = Progress::disabled();
        let result =
            run_scan_repos_with_du(&git, &[r], 180, &[], false, false, &no_du, &p).unwrap();
        assert_eq!(result.total_scanned, 1);
        assert_eq!(result.counts.get("active"), 1);
    }

    #[test]
    fn run_scan_repos_parallel_multiple_repos() {
        use git_tidy_core::progress::Progress;

        let active = PathBuf::from("/repos/active-project");
        let stale = PathBuf::from("/repos/stale-project");
        let orphaned = PathBuf::from("/repos/orphaned-project");

        let git = MockGitBuilder::new()
            // Active repo
            .with_last_commit_date(&active, Some(&today_iso()))
            .with_status_porcelain(&active, vec![])
            .with_local_branches(&active, vec!["main".to_string()])
            .with_list_remotes(&active, vec!["origin".to_string()])
            .with_remote_url(&active, "origin", "https://github.com/user/active.git")
            .with_ls_remote_check(&active, "origin", true)
            // Stale repo
            .with_last_commit_date(&stale, Some(&old_iso()))
            .with_status_porcelain(&stale, vec![])
            .with_local_branches(&stale, vec!["main".to_string()])
            .with_list_remotes(&stale, vec!["origin".to_string()])
            .with_remote_url(&stale, "origin", "https://github.com/user/stale.git")
            .with_ls_remote_check(&stale, "origin", true)
            // Orphaned repo (no remotes)
            .with_last_commit_date(&orphaned, Some(&today_iso()))
            .with_status_porcelain(&orphaned, vec![])
            .with_local_branches(&orphaned, vec!["main".to_string()])
            .with_list_remotes(&orphaned, vec![])
            .build();

        let p = Progress::disabled();
        let du = fixed_du(1024);
        let result = run_scan_repos_with_du(
            &git,
            &[active, stale, orphaned],
            180,
            &[],
            false,
            false,
            &du,
            &p,
        )
        .unwrap();

        assert_eq!(result.total_scanned, 3);
        assert_eq!(result.counts.get("active"), 1);
        assert_eq!(result.counts.get("stale"), 1);
        assert_eq!(result.counts.get("orphaned"), 1);
        assert_eq!(result.total_disk_usage_bytes, 3 * 1024);
        assert_eq!(result.reclaimable_bytes, 2 * 1024); // stale + orphaned
    }

    #[test]
    fn classify_noise_patterns_filter_dirty() {
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_status_porcelain(&r, vec!["?? .DS_Store".to_string()])
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes(&r, vec!["origin".to_string()])
            .with_remote_url(&r, "origin", "https://github.com/user/repo.git")
            .with_ls_remote_check(&r, "origin", true)
            .build();

        let noise = vec![".DS_Store".to_string()];
        let (info, _warnings) = classify_repo(&git, &r, 180, &noise, false, &no_du);

        // .DS_Store is noise, so not dirty
        assert!(!info.is_dirty);
        assert_eq!(info.dirty_file_count, 0);
    }

    #[test]
    fn classify_list_remotes_error_does_not_classify_as_orphaned() {
        // Regression: `list_remotes().unwrap_or_default()` previously made an unreadable .git/config look like "no remotes" → Orphaned → eligible for rm -rf with --all -y -f. Failing closed: classify as Active and surface a warning.
        let r = repo();
        let git = MockGitBuilder::new()
            .with_last_commit_date(&r, Some(&today_iso()))
            .with_local_branches(&r, vec!["main".to_string()])
            .with_list_remotes_error(&r, "config unreadable")
            .build();

        let (info, warnings) = classify_repo(&git, &r, 180, &[], false, &no_du);

        assert_ne!(
            info.classification,
            RepoClassification::Orphaned,
            "repo with unreadable remote config must not be classified Orphaned",
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("list remotes") && w.contains("config unreadable")),
            "expected warning, got: {warnings:?}",
        );
    }
}
