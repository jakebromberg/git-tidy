use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::clean::{Decision, Outcome, run_clean as core_run_clean};
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::types::{CleanResult, FailedItem};

use crate::types::{StashClassification, StashInfo, StashScanResult};

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
/// index renumbering side effects. Because the shared [`core_run_clean`] loop
/// preserves input order, each group's stashes are pre-sorted by descending
/// stash index *before* the call (see [`order_group_items`]); the loop then
/// drops them high-index-first while [`should_clean`] (as the `decide` filter)
/// counts the rest as skipped.
pub fn run_clean(
    git: &dyn GitOps,
    scan_result: &StashScanResult,
    options: &CleanOptions,
    out: &mut dyn Write,
) -> Result<CleanResult<DroppedStash>, Error> {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    // One `core_run_clean` call per group keeps each repo's output contiguous
    // and lets the per-group descending-index pre-sort do the ordering work.
    for group in &scan_result.repos {
        let ordered = order_group_items(&group.items, options);

        let result = core_run_clean(
            ordered,
            |stash| {
                if should_clean(&stash.classification, options) {
                    Decision::Clean
                } else {
                    Decision::Skip
                }
            },
            |stash, out| {
                let stash_ref = stash.stash_ref.as_str();

                if options.dry_run {
                    writeln!(out, "would drop {} in {}", stash_ref, group.name)?;
                    return Ok(Outcome::Cleaned(DroppedStash {
                        repo: group.repo_path.clone(),
                        stash_ref: stash_ref.to_string(),
                    }));
                }

                match git.stash_drop(&group.repo_path, stash_ref) {
                    Ok(()) => {
                        writeln!(out, "dropped {} in {}", stash_ref, group.name)?;
                        Ok(Outcome::Cleaned(DroppedStash {
                            repo: group.repo_path.clone(),
                            stash_ref: stash_ref.to_string(),
                        }))
                    }
                    Err(e) => {
                        writeln!(out, "error: could not drop {}: {e}", stash_ref)?;
                        Ok(Outcome::Failed(FailedItem {
                            repo: group.repo_path.clone(),
                            name: stash_ref.to_string(),
                            reason: e.to_string(),
                        }))
                    }
                }
            },
            out,
        )?;

        succeeded.extend(result.succeeded);
        failed.extend(result.failed);
        skipped += result.skipped;
    }

    Ok(CleanResult {
        succeeded,
        failed,
        skipped,
    })
}

/// Order one group's stashes for the shared clean loop.
///
/// The shared loop preserves input order, so the descending-index drop order is
/// established here. Admitted, drop-eligible stashes are those that both pass
/// [`should_clean`] and carry a parseable `stash@{N}` index; they are sorted by
/// descending index so that dropping a higher index never renumbers a lower one
/// still pending. Stashes rejected by [`should_clean`] are retained so the loop
/// still counts them as skipped (their relative order is irrelevant). Stashes
/// that would be cleaned but whose ref does not parse are dropped from the
/// list entirely — matching the original loop, which silently ignored them
/// (never counting them as succeeded, failed, or skipped).
fn order_group_items<'a>(items: &'a [StashInfo], options: &CleanOptions) -> Vec<&'a StashInfo> {
    let mut ordered: Vec<&StashInfo> = items
        .iter()
        .filter(|stash| {
            // Keep skip candidates regardless of parseability; for admitted
            // stashes, keep only those with a parseable index.
            !should_clean(&stash.classification, options)
                || parse_stash_index(&stash.stash_ref).is_some()
        })
        .collect();

    // Descending stash index. Skip candidates (no admitted index needed) sort by
    // their own parsed index or 0; their position only affects skip ordering,
    // which the result does not observe.
    ordered
        .sort_by_key(|stash| std::cmp::Reverse(parse_stash_index(&stash.stash_ref).unwrap_or(0)));

    ordered
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
