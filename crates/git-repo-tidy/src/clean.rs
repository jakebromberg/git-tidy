use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use git_tidy_core::error::Error;

use crate::types::{RepoClassification, RepoScanResult};

/// Options controlling repo cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Allow deleting dirty repos.
    pub force: bool,
    /// Only delete stale repos.
    pub stale_only: bool,
    /// Only delete orphaned repos.
    pub orphaned_only: bool,
    /// Delete all non-active repos (stale + orphaned). This is the default.
    #[allow(dead_code)]
    pub all: bool,
}

/// Result of a clean operation.
#[derive(Debug)]
#[allow(dead_code)]
pub struct CleanResult {
    /// Repos that were deleted (or would be in dry-run).
    pub deleted: Vec<DeletedRepo>,
    /// Repos that failed to delete.
    pub failed: Vec<FailedRepo>,
    /// Repos that were skipped.
    pub skipped: usize,
    /// Whether any dirty repos blocked deletion.
    pub dirty_blocked: bool,
}

/// A repo that was successfully deleted.
#[derive(Debug)]
#[allow(dead_code)]
pub struct DeletedRepo {
    pub path: PathBuf,
    pub name: String,
}

/// A repo that failed to be deleted.
#[derive(Debug)]
#[allow(dead_code)]
pub struct FailedRepo {
    pub path: PathBuf,
    pub name: String,
    pub reason: String,
}

/// Determine if a repo should be cleaned based on its classification and options.
fn should_clean(classification: RepoClassification, options: &CleanOptions) -> bool {
    match classification {
        RepoClassification::Active => false,
        RepoClassification::Stale => !options.orphaned_only,
        RepoClassification::Orphaned => !options.stale_only,
    }
}

/// Run the clean operation on a scan result.
///
/// `delete_fn` abstracts the actual deletion for testability. In production,
/// pass `std::fs::remove_dir_all`. In tests, pass a tracking mock.
pub fn run_clean(
    scan_result: &RepoScanResult,
    options: &CleanOptions,
    delete_fn: &dyn Fn(&Path) -> io::Result<()>,
    out: &mut dyn Write,
) -> Result<CleanResult, Error> {
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;
    let mut dirty_blocked = false;

    for repo in &scan_result.repos {
        if !should_clean(repo.classification, options) {
            skipped += 1;
            continue;
        }

        // Dirty safety check
        if repo.is_dirty && !options.force {
            writeln!(
                out,
                "warning: skipping dirty repo {} (use --force to delete)",
                repo.name,
            )?;
            skipped += 1;
            dirty_blocked = true;
            continue;
        }

        if options.dry_run {
            writeln!(out, "would delete {}", repo.name)?;
            deleted.push(DeletedRepo {
                path: repo.path.clone(),
                name: repo.name.clone(),
            });
            continue;
        }

        match delete_fn(&repo.path) {
            Ok(()) => {
                writeln!(out, "deleted {}", repo.name)?;
                deleted.push(DeletedRepo {
                    path: repo.path.clone(),
                    name: repo.name.clone(),
                });
            }
            Err(e) => {
                writeln!(out, "error: could not delete {}: {e}", repo.name)?;
                failed.push(FailedRepo {
                    path: repo.path.clone(),
                    name: repo.name.clone(),
                    reason: e.to_string(),
                });
            }
        }
    }

    Ok(CleanResult {
        deleted,
        failed,
        skipped,
        dirty_blocked,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::types::*;

    fn make_repo(name: &str, classification: RepoClassification, is_dirty: bool) -> RepoInfo {
        RepoInfo {
            path: PathBuf::from(format!("/repos/{name}")),
            name: name.to_string(),
            classification,
            last_commit_date: Some("2024-01-15T12:00:00+00:00".to_string()),
            last_commit_age_days: Some(400),
            disk_usage_bytes: 100 * 1024 * 1024,
            remote_url: Some("https://github.com/user/repo.git".to_string()),
            branch_count: 1,
            has_remote: true,
            is_dirty,
            dirty_file_count: if is_dirty { 3 } else { 0 },
        }
    }

    fn make_scan_result(repos: Vec<RepoInfo>) -> RepoScanResult {
        let mut counts = RepoCounts::default();
        let mut reclaimable = 0u64;
        let mut total_disk = 0u64;
        for r in &repos {
            counts.increment(r.classification, r.is_dirty);
            total_disk += r.disk_usage_bytes;
            if matches!(
                r.classification,
                RepoClassification::Stale | RepoClassification::Orphaned
            ) {
                reclaimable += r.disk_usage_bytes;
            }
        }
        RepoScanResult {
            total_scanned: repos.len(),
            repos,
            counts,
            warnings: vec![],
            total_disk_usage_bytes: total_disk,
            reclaimable_bytes: reclaimable,
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            force: false,
            stale_only: false,
            orphaned_only: false,
            all: false,
        }
    }

    fn noop_delete(_path: &Path) -> io::Result<()> {
        Ok(())
    }

    fn failing_delete(_path: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied",
        ))
    }

    #[test]
    fn clean_deletes_stale() {
        let scan = make_scan_result(vec![make_repo("old", RepoClassification::Stale, false)]);
        let mut buf = Vec::new();
        let deleted_paths = RefCell::new(Vec::new());
        let delete_fn = |path: &Path| -> io::Result<()> {
            deleted_paths.borrow_mut().push(path.to_path_buf());
            Ok(())
        };

        let result = run_clean(&scan, &default_options(), &delete_fn, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].name, "old");
        assert_eq!(deleted_paths.borrow().len(), 1);
    }

    #[test]
    fn clean_deletes_orphaned() {
        let scan = make_scan_result(vec![make_repo(
            "orphan",
            RepoClassification::Orphaned,
            false,
        )]);
        let mut buf = Vec::new();

        let result = run_clean(&scan, &default_options(), &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].name, "orphan");
    }

    #[test]
    fn clean_skips_active() {
        let scan = make_scan_result(vec![make_repo("app", RepoClassification::Active, false)]);
        let mut buf = Vec::new();

        let result = run_clean(&scan, &default_options(), &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_skips_dirty_without_force() {
        let scan = make_scan_result(vec![make_repo("old", RepoClassification::Stale, true)]);
        let mut buf = Vec::new();

        let result = run_clean(&scan, &default_options(), &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 0);
        assert_eq!(result.skipped, 1);
        assert!(result.dirty_blocked);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipping dirty repo old"));
        assert!(output.contains("--force"));
    }

    #[test]
    fn clean_deletes_dirty_with_force() {
        let scan = make_scan_result(vec![make_repo("old", RepoClassification::Stale, true)]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&scan, &options, &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 1);
        assert!(!result.dirty_blocked);
    }

    #[test]
    fn clean_stale_only() {
        let scan = make_scan_result(vec![
            make_repo("old", RepoClassification::Stale, false),
            make_repo("orphan", RepoClassification::Orphaned, false),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            stale_only: true,
            ..default_options()
        };

        let result = run_clean(&scan, &options, &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].name, "old");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_orphaned_only() {
        let scan = make_scan_result(vec![
            make_repo("old", RepoClassification::Stale, false),
            make_repo("orphan", RepoClassification::Orphaned, false),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            orphaned_only: true,
            ..default_options()
        };

        let result = run_clean(&scan, &options, &noop_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0].name, "orphan");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_dry_run() {
        let scan = make_scan_result(vec![
            make_repo("old", RepoClassification::Stale, false),
            make_repo("orphan", RepoClassification::Orphaned, false),
        ]);
        let mut buf = Vec::new();
        let deleted_paths = RefCell::new(Vec::new());
        let delete_fn = |path: &Path| -> io::Result<()> {
            deleted_paths.borrow_mut().push(path.to_path_buf());
            Ok(())
        };
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&scan, &options, &delete_fn, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 2);
        // Dry run: no actual deletes
        assert_eq!(deleted_paths.borrow().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would delete old"));
        assert!(output.contains("would delete orphan"));
    }

    #[test]
    fn clean_handles_delete_failure() {
        let scan = make_scan_result(vec![make_repo("old", RepoClassification::Stale, false)]);
        let mut buf = Vec::new();

        let result = run_clean(&scan, &default_options(), &failing_delete, &mut buf).unwrap();

        assert_eq!(result.deleted.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "old");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not delete old"));
    }

    #[test]
    fn clean_mixed_scenario() {
        let scan = make_scan_result(vec![
            make_repo("stale-clean", RepoClassification::Stale, false),
            make_repo("stale-dirty", RepoClassification::Stale, true),
            make_repo("orphan-clean", RepoClassification::Orphaned, false),
            make_repo("active", RepoClassification::Active, false),
        ]);
        let mut buf = Vec::new();

        let result = run_clean(&scan, &default_options(), &noop_delete, &mut buf).unwrap();

        // stale-clean + orphan-clean deleted; stale-dirty skipped (dirty); active skipped
        assert_eq!(result.deleted.len(), 2);
        assert_eq!(result.skipped, 2);
        assert!(result.dirty_blocked);
    }
}
