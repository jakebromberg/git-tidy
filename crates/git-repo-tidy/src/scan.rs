use std::path::Path;
use std::process::Command;

use git_tidy_core::date::days_since_iso_date;
use git_tidy_core::dirty::check_dirty;
use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::filter::{NameFilter, filter_paths};
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;

use crate::types::{RepoClassification, RepoCounts, RepoInfo, RepoScanResult};

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

/// Classify a single repository.
pub fn classify_repo(
    git: &dyn GitOps,
    repo_path: &Path,
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    du_fn: &dyn Fn(&Path) -> u64,
) -> RepoInfo {
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

    // Remote detection and reachability
    let remotes = git.list_remotes(repo_path).unwrap_or_default();
    let has_remote = !remotes.is_empty();

    let remote_url = if has_remote {
        git.remote_url(repo_path, &remotes[0]).ok()
    } else {
        None
    };

    let any_reachable = if has_remote && !offline {
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

    RepoInfo {
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
    }
}

/// Scan all repos in `directory` and classify them.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    repo_filter: &NameFilter,
    progress: &Progress,
) -> Result<RepoScanResult, Error> {
    run_scan_with_du(
        git,
        directory,
        stale_days,
        noise_patterns,
        offline,
        repo_filter,
        &disk_usage,
        progress,
    )
}

/// Scan with an injectable disk-usage function (for testing).
#[allow(clippy::too_many_arguments)]
pub fn run_scan_with_du(
    git: &dyn GitOps,
    directory: &Path,
    stale_days: u64,
    noise_patterns: &[String],
    offline: bool,
    repo_filter: &NameFilter,
    du_fn: &dyn Fn(&Path) -> u64,
    progress: &Progress,
) -> Result<RepoScanResult, Error> {
    let repo_paths = discover_repos(directory)?;
    let repo_paths = filter_paths(repo_paths, repo_filter);

    let mut repos = Vec::new();
    let mut counts = RepoCounts::default();
    let warnings = Vec::new();
    let mut total_disk_usage_bytes = 0u64;
    let mut reclaimable_bytes = 0u64;

    let pb = progress.bar(repo_paths.len() as u64, "Scanning repos");
    for repo_path in &repo_paths {
        let info = classify_repo(git, repo_path, stale_days, noise_patterns, offline, du_fn);

        counts.increment(info.classification, info.is_dirty);
        total_disk_usage_bytes += info.disk_usage_bytes;

        if matches!(
            info.classification,
            RepoClassification::Stale | RepoClassification::Orphaned
        ) {
            reclaimable_bytes += info.disk_usage_bytes;
        }

        repos.push(info);
        pb.inc(1);
    }
    pb.finish_and_clear();

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
        warnings,
        total_disk_usage_bytes,
        reclaimable_bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], true, &no_du);

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

        let info = classify_repo(&git, &r, 180, &[], false, &no_du);

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
        let info = classify_repo(&git, &r, 180, &[], false, &du);

        assert_eq!(info.disk_usage_bytes, 142 * 1024 * 1024);
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
        let info = classify_repo(&git, &r, 180, &noise, false, &no_du);

        // .DS_Store is noise, so not dirty
        assert!(!info.is_dirty);
        assert_eq!(info.dirty_file_count, 0);
    }
}
