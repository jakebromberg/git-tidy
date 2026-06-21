use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{CleanResult, FailedItem};

use crate::types::{StashClassification, StashScanResult};

/// Options controlling stash cleanup behavior.
pub struct CleanOptions {
    /// Preview only: print what would be dropped.
    pub dry_run: bool,
    /// Only target committed stashes.
    pub committed_only: bool,
    /// Only target aged stashes.
    pub aged_only: bool,
    /// Include all stashes except active.
    pub all: bool,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct DroppedStash {
    pub repo: PathBuf,
    pub stash_ref: String,
}

/// Run the clean operation on a scan result.
///
/// Key design: drops stashes in descending index order per repo to prevent
/// index renumbering side effects.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &StashScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<DroppedStash>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    for group in &scan_result.repos {
        // Collect stashes to drop, along with their parsed indices
        let mut to_drop: Vec<(usize, &str, &StashClassification)> = Vec::new();

        for stash in &group.items {
            if !should_clean(&stash.classification, options) {
                skipped += 1;
                continue;
            }

            if let Some(idx) = parse_stash_index(&stash.stash_ref) {
                to_drop.push((idx, &stash.stash_ref, &stash.classification));
            }
        }

        // Sort by descending index to prevent renumbering
        to_drop.sort_by_key(|b| std::cmp::Reverse(b.0));

        for (_, stash_ref, _) in &to_drop {
            if options.dry_run {
                writeln!(out, "would drop {} in {}", stash_ref, group.name)?;
                succeeded.push(DroppedStash {
                    repo: group.repo_path.clone(),
                    stash_ref: stash_ref.to_string(),
                });
                continue;
            }

            match git.stash_drop(&group.repo_path, stash_ref) {
                Ok(()) => {
                    writeln!(out, "dropped {} in {}", stash_ref, group.name)?;
                    succeeded.push(DroppedStash {
                        repo: group.repo_path.clone(),
                        stash_ref: stash_ref.to_string(),
                    });
                }
                Err(e) => {
                    writeln!(out, "error: could not drop {}: {e}", stash_ref)?;
                    failed.push(FailedItem {
                        repo: group.repo_path.clone(),
                        name: stash_ref.to_string(),
                        reason: e.to_string(),
                    });
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

/// Determine if a stash should be cleaned based on its classification and options.
fn should_clean(classification: &StashClassification, options: &CleanOptions) -> bool {
    if options.all {
        // All except active
        return *classification != StashClassification::Active;
    }

    if options.committed_only {
        return *classification == StashClassification::Committed;
    }

    if options.aged_only {
        return *classification == StashClassification::Aged;
    }

    // Default: committed + orphaned
    matches!(
        classification,
        StashClassification::Committed | StashClassification::Orphaned
    )
}

/// Parse the numeric index from a stash ref like "stash@{2}".
fn parse_stash_index(stash_ref: &str) -> Option<usize> {
    let start = stash_ref.find('{')? + 1;
    let end = stash_ref.find('}')?;
    stash_ref[start..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::counts::Counts;
    use git_tidy_core::scan::RepoGroup;
    use git_tidy_core::testutil::MockGitBuilder;
    use git_tidy_core::types::ClassificationLabel;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_scan_result(stashes: Vec<StashInfo>) -> StashScanResult {
        let mut counts = Counts::default();
        for s in &stashes {
            counts.increment(s.classification.label());
        }
        StashScanResult {
            repos: vec![RepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                items: stashes,
            }],
            total_scanned: 0,
            counts,
            warnings: vec![],
        }
    }

    fn stash(stash_ref: &str, classification: StashClassification) -> StashInfo {
        StashInfo {
            repo_path: repo(),
            stash_ref: stash_ref.to_string(),
            classification,
            branch: Some("main".to_string()),
            age_days: Some(10),
            message: format!("WIP on main: {stash_ref}"),
        }
    }

    fn default_options() -> CleanOptions {
        CleanOptions {
            dry_run: false,
            committed_only: false,
            aged_only: false,
            all: false,
        }
    }

    #[test]
    fn clean_drops_committed_stashes() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![stash("stash@{0}", StashClassification::Committed)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].stash_ref, "stash@{0}");
        assert_eq!(git.stash_drop_calls().len(), 1);
    }

    #[test]
    fn clean_drops_orphaned_stashes_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![stash("stash@{0}", StashClassification::Orphaned)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(git.stash_drop_calls().len(), 1);
    }

    #[test]
    fn clean_skips_active_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![stash("stash@{0}", StashClassification::Active)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.stash_drop_calls().len(), 0);
    }

    #[test]
    fn clean_skips_aged_by_default() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![stash("stash@{0}", StashClassification::Aged)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_drops_in_descending_order() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            stash("stash@{0}", StashClassification::Committed),
            stash("stash@{1}", StashClassification::Orphaned),
            stash("stash@{2}", StashClassification::Committed),
        ]);
        let mut buf = Vec::new();

        run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        let calls = git.stash_drop_calls();
        assert_eq!(calls.len(), 3);
        // Must be descending order: stash@{2}, stash@{1}, stash@{0}
        assert_eq!(calls[0].1, "stash@{2}");
        assert_eq!(calls[1].1, "stash@{1}");
        assert_eq!(calls[2].1, "stash@{0}");
    }

    #[test]
    fn clean_dry_run_makes_zero_drop_calls() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            stash("stash@{0}", StashClassification::Committed),
            stash("stash@{1}", StashClassification::Orphaned),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            dry_run: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(git.stash_drop_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would drop stash@{0}"));
        assert!(output.contains("would drop stash@{1}"));
    }

    #[test]
    fn clean_committed_only_skips_orphaned() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            stash("stash@{0}", StashClassification::Committed),
            stash("stash@{1}", StashClassification::Orphaned),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            committed_only: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].stash_ref, "stash@{0}");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_aged_only() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            stash("stash@{0}", StashClassification::Committed),
            stash("stash@{1}", StashClassification::Aged),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            aged_only: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].stash_ref, "stash@{1}");
        assert_eq!(result.skipped, 1);
    }

    #[test]
    fn clean_all_includes_aged() {
        let git = MockGitBuilder::new().build();
        let scan = make_scan_result(vec![
            stash("stash@{0}", StashClassification::Committed),
            stash("stash@{1}", StashClassification::Orphaned),
            stash("stash@{2}", StashClassification::Aged),
            stash("stash@{3}", StashClassification::Active),
        ]);
        let mut buf = Vec::new();
        let options = CleanOptions {
            all: true,
            ..default_options()
        };

        let result = run_clean(&git, &scan, &options, &mut buf).unwrap();

        // All except active
        assert_eq!(result.succeeded.len(), 3);
        assert_eq!(result.skipped, 1); // active skipped
    }

    #[test]
    fn clean_handles_drop_failure() {
        let git = MockGitBuilder::new()
            .with_stash_drop_error(&repo(), "stash@{0}", "stash entry not found")
            .build();
        let scan = make_scan_result(vec![stash("stash@{0}", StashClassification::Committed)]);
        let mut buf = Vec::new();

        let result = run_clean(&git, &scan, &default_options(), &mut buf).unwrap();

        assert_eq!(result.succeeded.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].name, "stash@{0}");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not drop stash@{0}"));
    }

    #[test]
    fn parse_stash_index_valid() {
        assert_eq!(parse_stash_index("stash@{0}"), Some(0));
        assert_eq!(parse_stash_index("stash@{42}"), Some(42));
    }

    #[test]
    fn parse_stash_index_invalid() {
        assert_eq!(parse_stash_index("invalid"), None);
        assert_eq!(parse_stash_index("stash@{abc}"), None);
    }
}
