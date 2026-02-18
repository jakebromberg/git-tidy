use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{CleanResult, FailedItem};

use crate::types::{TagClassification, TagScanResult};

/// Options controlling tag cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be removed.
    pub dry_run: bool,
    /// Skip confirmation prompts.
    #[allow(dead_code)]
    pub yes: bool,
    /// Allow deleting synced tags and bypass release tag warnings.
    pub force: bool,
    /// Only delete stale tags.
    pub stale_only: bool,
    /// Only delete local-only tags.
    pub local_only: bool,
    /// Also delete remote copies when cleaning stale tags.
    pub include_remote: bool,
    /// Delete stale + local_only + remote_only (not synced).
    pub all: bool,
}

/// A tag that was successfully removed.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RemovedTag {
    pub repo: PathBuf,
    pub name: String,
    /// Whether remote copies were also deleted.
    pub remote_deleted: bool,
}

/// Run the clean operation on a scan result.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &TagScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<RemovedTag>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    for group in &scan_result.repos {
        for tag in &group.tags {
            if !should_clean(&tag.classification, options) {
                skipped += 1;
                continue;
            }

            // Release tag protection
            if tag.is_release_tag && !options.force {
                writeln!(
                    out,
                    "warning: skipping release tag {} in {} (use --force to remove)",
                    tag.name, group.name,
                )?;
                skipped += 1;
                continue;
            }

            if options.dry_run {
                let mut action = format!("would delete tag {} in {}", tag.name, group.name);
                if options.include_remote && !tag.remote_names.is_empty() {
                    write!(
                        action,
                        " (and from remotes: {})",
                        tag.remote_names.join(", ")
                    )
                    .unwrap();
                }
                writeln!(out, "{action}")?;
                succeeded.push(RemovedTag {
                    repo: group.repo_path.clone(),
                    name: tag.name.clone(),
                    remote_deleted: false,
                });
                continue;
            }

            // Delete local tag (for local and synced tags)
            if tag.classification != TagClassification::RemoteOnly {
                match git.tag_delete(&group.repo_path, &tag.name) {
                    Ok(()) => {
                        writeln!(out, "deleted tag {} in {}", tag.name, group.name)?;
                    }
                    Err(e) => {
                        writeln!(out, "error: could not delete tag {}: {e}", tag.name,)?;
                        failed.push(FailedItem {
                            repo: group.repo_path.clone(),
                            name: tag.name.clone(),
                            reason: e.to_string(),
                        });
                        continue;
                    }
                }
            }

            // Delete remote copies if --include-remote
            let mut remote_deleted = false;
            if options.include_remote && !tag.remote_names.is_empty() {
                for remote_name in &tag.remote_names {
                    match git.tag_delete_remote(&group.repo_path, remote_name, &tag.name) {
                        Ok(()) => {
                            writeln!(
                                out,
                                "deleted tag {} from remote {remote_name} in {}",
                                tag.name, group.name,
                            )?;
                            remote_deleted = true;
                        }
                        Err(e) => {
                            writeln!(
                                out,
                                "warning: could not delete tag {} from remote {remote_name}: {e}",
                                tag.name,
                            )?;
                        }
                    }
                }
            }

            // For remote-only tags with --all, delete from remotes
            if tag.classification == TagClassification::RemoteOnly {
                for remote_name in &tag.remote_names {
                    match git.tag_delete_remote(&group.repo_path, remote_name, &tag.name) {
                        Ok(()) => {
                            writeln!(
                                out,
                                "deleted tag {} from remote {remote_name} in {}",
                                tag.name, group.name,
                            )?;
                            remote_deleted = true;
                        }
                        Err(e) => {
                            writeln!(
                                out,
                                "error: could not delete tag {} from remote {remote_name}: {e}",
                                tag.name,
                            )?;
                            failed.push(FailedItem {
                                repo: group.repo_path.clone(),
                                name: tag.name.clone(),
                                reason: e.to_string(),
                            });
                        }
                    }
                }
            }

            succeeded.push(RemovedTag {
                repo: group.repo_path.clone(),
                name: tag.name.clone(),
                remote_deleted,
            });
        }
    }

    Ok(CleanResult {
        succeeded,
        failed,
        skipped,
    })
}

/// Determine if a tag should be cleaned based on its classification and options.
fn should_clean(classification: &TagClassification, options: &CleanOptions) -> bool {
    if options.stale_only {
        return *classification == TagClassification::Stale;
    }

    if options.local_only {
        return *classification == TagClassification::LocalOnly;
    }

    if options.all {
        // Stale + LocalOnly + RemoteOnly (not Synced, unless --force)
        return if options.force {
            true
        } else {
            *classification != TagClassification::Synced
        };
    }

    // Default: Stale + LocalOnly
    matches!(
        classification,
        TagClassification::Stale | TagClassification::LocalOnly
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(tags: Vec<TagInfo>) -> TagScanResult {
        let mut counts = TagCounts::default();
        for t in &tags {
            counts.increment(&t.classification);
        }
        TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                tags,
            }],
            total_scanned: 0,
            counts,
            warnings: vec![],
        }
    }

    fn tag(name: &str, classification: TagClassification) -> TagInfo {
        TagInfo {
            repo_path: repo(),
            name: name.to_string(),
            classification,
            commit: "abc1234".to_string(),
            is_annotated: false,
            tagger_date: None,
            is_release_tag: false,
            remote_names: vec![],
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            yes: false,
            force: false,
            stale_only: false,
            local_only: false,
            include_remote: false,
            all: false,
        }
    }

    #[test]
    fn clean_deletes_stale() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![tag("old-tag", TagClassification::Stale)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "old-tag");
        assert_eq!(git.tag_delete_calls().len(), 1);
    }

    #[test]
    fn clean_deletes_local_only() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![tag("local-tag", TagClassification::LocalOnly)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "local-tag");
    }

    #[test]
    fn clean_skips_synced_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![tag("v1.0", TagClassification::Synced)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.tag_delete_calls().len(), 0);
    }

    #[test]
    fn clean_skips_remote_only_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![tag("remote-tag", TagClassification::RemoteOnly)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_stale_only_flag() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            tag("stale-tag", TagClassification::Stale),
            tag("local-tag", TagClassification::LocalOnly),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            stale_only: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "stale-tag");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_local_only_flag() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            tag("stale-tag", TagClassification::Stale),
            tag("local-tag", TagClassification::LocalOnly),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            local_only: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].name, "local-tag");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_all_includes_remote_only() {
        let git = MockGitBuilder::new().build();
        let mut remote_tag = tag("remote-tag", TagClassification::RemoteOnly);
        remote_tag.remote_names = vec!["origin".to_string()];

        let scan = make_scan_result(vec![
            tag("stale-tag", TagClassification::Stale),
            tag("local-tag", TagClassification::LocalOnly),
            remote_tag,
            tag("synced-tag", TagClassification::Synced),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        // Stale + local_only + remote_only removed, synced skipped
        assert_eq!(result.succeeded.len(), 3);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_force_deletes_synced() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![tag("v1.0", TagClassification::Synced)]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.tag_delete_calls().len(), 1);
    }

    #[test]
    fn clean_include_remote() {
        let git = MockGitBuilder::new().build();
        let mut stale = tag("old-tag", TagClassification::Stale);
        stale.remote_names = vec!["origin".to_string()];

        let scan = make_scan_result(vec![stale]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            include_remote: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert!(result.succeeded[0].remote_deleted);
        assert_eq!(git.tag_delete_calls().len(), 1);
        assert_eq!(git.tag_delete_remote_calls().len(), 1);
    }

    #[test]
    fn clean_release_tag_protection() {
        let git = MockGitBuilder::new().build();
        let mut release = tag("v1.0.0", TagClassification::Stale);
        release.is_release_tag = true;

        let scan = make_scan_result(vec![release]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.tag_delete_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipping release tag"));
        assert!(output.contains("--force"));
    }

    #[test]
    fn clean_release_tag_with_force() {
        let git = MockGitBuilder::new().build();
        let mut release = tag("v1.0.0", TagClassification::Stale);
        release.is_release_tag = true;

        let scan = make_scan_result(vec![release]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            force: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.tag_delete_calls().len(), 1);
    }

    #[test]
    fn clean_dry_run_makes_zero_calls() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            tag("stale-tag", TagClassification::Stale),
            tag("local-tag", TagClassification::LocalOnly),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.tag_delete_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would delete tag stale-tag"));
        assert!(output.contains("would delete tag local-tag"));
    }

    #[test]
    fn clean_handles_deletion_failure() {
        let git = MockGitBuilder::new()
            .with_tag_delete_error(&repo(), "bad-tag", "permission denied")
            .build();
        let scan = make_scan_result(vec![tag("bad-tag", TagClassification::Stale)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "bad-tag");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not delete tag bad-tag"));
    }
}
