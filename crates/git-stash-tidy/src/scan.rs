use std::path::Path;

use git_tidy_core::date::days_since_iso_date;
use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::landed::diff_similarity;

use crate::types::{
    StashClassification, StashCounts, StashInfo, StashRepoGroup, StashScanResult,
    parse_stash_branch,
};

/// Classify a single stash entry.
///
/// Priority logic:
/// 1. If the branch exists and `diff_similarity(stash_diff, branch_tip_diff) >= 0.5` -> Committed
/// 2. If the branch doesn't exist locally -> Orphaned
/// 3. If age >= threshold -> Aged
/// 4. Otherwise -> Active
///
/// When the branch name is unparseable, skip committed/orphaned checks; fall through to age check.
#[allow(clippy::collapsible_if)]
pub fn classify_stash(
    git: &dyn GitOps,
    repo: &Path,
    stash_ref: &str,
    message: &str,
    iso_date: &str,
    age_threshold: u64,
) -> StashClassification {
    let age_days = days_since_iso_date(iso_date);
    let branch = parse_stash_branch(message);

    if let Some(ref branch_name) = branch {
        // Check if the branch exists locally
        let branches = git.list_local_branches(repo).unwrap_or_default();
        let branch_exists = branches.iter().any(|b| b == branch_name);

        if branch_exists {
            // Compare stash diff to branch tip diff
            if let Ok(stash_d) = git.stash_diff(repo, stash_ref) {
                if let Ok(tip_hash) = git.rev_parse(repo, branch_name) {
                    if let Ok(tip_d) = git.diff_commit(repo, &tip_hash) {
                        if diff_similarity(&stash_d, &tip_d) >= 0.5 {
                            return StashClassification::Committed;
                        }
                    }
                }
            }
        } else {
            return StashClassification::Orphaned;
        }
    }

    // Age check
    if let Some(days) = age_days {
        if days >= age_threshold {
            return StashClassification::Aged;
        }
    }

    StashClassification::Active
}

/// Scan all repos in `directory` for stash entries.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    age_threshold: u64,
) -> Result<StashScanResult, Error> {
    let repo_paths = discover_repos(directory)?;

    let mut repos = Vec::new();
    let mut counts = StashCounts::default();
    let mut warnings = Vec::new();
    let mut total_scanned = 0;

    for repo_path in &repo_paths {
        let stashes = match git.list_stashes(repo_path) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!(
                    "could not list stashes for {}: {e}",
                    repo_path.display()
                ));
                continue;
            }
        };

        if stashes.is_empty() {
            continue;
        }

        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_path.display().to_string());

        let mut classified = Vec::new();

        for (stash_ref, message, iso_date) in &stashes {
            let classification =
                classify_stash(git, repo_path, stash_ref, message, iso_date, age_threshold);
            let age_days = days_since_iso_date(iso_date);
            let branch = parse_stash_branch(message);

            counts.increment(classification);
            total_scanned += 1;

            classified.push(StashInfo {
                repo_path: repo_path.clone(),
                stash_ref: stash_ref.clone(),
                classification,
                branch,
                age_days,
                message: message.clone(),
            });
        }

        // Sort by classification priority
        classified.sort_by_key(|s| s.classification.priority());

        repos.push(StashRepoGroup {
            repo_path: repo_path.clone(),
            name: repo_name,
            stashes: classified,
        });
    }

    Ok(StashScanResult {
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
    fn classify_committed_stash() {
        // Stash diff matches branch tip -> Committed
        let diff = "+line 1\n+line 2\n";
        let git = MockGitBuilder::new()
            .with_local_branches(&repo(), vec!["feature-x".to_string()])
            .with_stash_diff(&repo(), "stash@{0}", diff)
            .with_rev_parse(&repo(), "feature-x", "abc123")
            .with_diff_commit(&repo(), "abc123", diff)
            .build();

        let result = classify_stash(
            &git,
            &repo(),
            "stash@{0}",
            "WIP on feature-x: abc1234 Add login",
            "2025-01-01T12:00:00+00:00",
            90,
        );
        assert_eq!(result, StashClassification::Committed);
    }

    #[test]
    fn classify_orphaned_stash() {
        // Branch doesn't exist locally -> Orphaned
        let git = MockGitBuilder::new()
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let result = classify_stash(
            &git,
            &repo(),
            "stash@{1}",
            "WIP on deleted-branch: def5678 Fix UI",
            "2025-01-01T12:00:00+00:00",
            90,
        );
        assert_eq!(result, StashClassification::Orphaned);
    }

    #[test]
    fn classify_aged_stash() {
        // Branch exists but diff doesn't match, and stash is old -> Aged
        let git = MockGitBuilder::new()
            .with_local_branches(&repo(), vec!["feature-y".to_string()])
            .with_stash_diff(&repo(), "stash@{0}", "+stash line\n")
            .with_rev_parse(&repo(), "feature-y", "def456")
            .with_diff_commit(&repo(), "def456", "+completely different\n")
            .build();

        let result = classify_stash(
            &git,
            &repo(),
            "stash@{0}",
            "WIP on feature-y: def456 Some work",
            "2020-01-01T12:00:00+00:00", // very old
            90,
        );
        assert_eq!(result, StashClassification::Aged);
    }

    #[test]
    fn classify_active_stash() {
        // Branch exists, diff doesn't match, and stash is recent -> Active
        let git = MockGitBuilder::new()
            .with_local_branches(&repo(), vec!["main".to_string()])
            .with_stash_diff(&repo(), "stash@{0}", "+stash line\n")
            .with_rev_parse(&repo(), "main", "ghi789")
            .with_diff_commit(&repo(), "ghi789", "+branch line\n")
            .build();

        // Use today's date to ensure it's recent
        let now = {
            use std::time::SystemTime;
            let secs = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let days = secs / 86400;
            // Approximate today's date
            format!(
                "{}T12:00:00+00:00",
                format_date_from_epoch_days(days as i64)
            )
        };

        let result = classify_stash(
            &git,
            &repo(),
            "stash@{0}",
            "WIP on main: ghi9012 Temp changes",
            &now,
            90,
        );
        assert_eq!(result, StashClassification::Active);
    }

    #[test]
    fn classify_unparseable_message_recent() {
        // Unparseable message, recent -> Active
        let git = MockGitBuilder::new().build();

        let now = {
            use std::time::SystemTime;
            let secs = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let days = secs / 86400;
            format!(
                "{}T12:00:00+00:00",
                format_date_from_epoch_days(days as i64)
            )
        };

        let result = classify_stash(&git, &repo(), "stash@{0}", "some random message", &now, 90);
        assert_eq!(result, StashClassification::Active);
    }

    #[test]
    fn classify_unparseable_message_aged() {
        // Unparseable message, old -> Aged
        let git = MockGitBuilder::new().build();

        let result = classify_stash(
            &git,
            &repo(),
            "stash@{0}",
            "some random message",
            "2020-01-01T12:00:00+00:00",
            90,
        );
        assert_eq!(result, StashClassification::Aged);
    }

    #[test]
    fn scan_groups_by_repo() {
        // We can't call run_scan with mock paths (discover_repos needs real dirs),
        // but we can verify the grouping/sorting logic by checking classify outputs.
        // run_scan is tested via integration tests with real repos.

        // Instead, verify classification sorting order
        let mut infos = [
            StashInfo {
                repo_path: repo(),
                stash_ref: "stash@{0}".to_string(),
                classification: StashClassification::Active,
                branch: Some("main".to_string()),
                age_days: Some(1),
                message: "WIP on main: abc".to_string(),
            },
            StashInfo {
                repo_path: repo(),
                stash_ref: "stash@{1}".to_string(),
                classification: StashClassification::Committed,
                branch: Some("feature".to_string()),
                age_days: Some(5),
                message: "WIP on feature: def".to_string(),
            },
            StashInfo {
                repo_path: repo(),
                stash_ref: "stash@{2}".to_string(),
                classification: StashClassification::Orphaned,
                branch: None,
                age_days: Some(30),
                message: "WIP on gone: ghi".to_string(),
            },
        ];

        infos.sort_by_key(|s: &StashInfo| s.classification.priority());

        assert_eq!(infos[0].classification, StashClassification::Committed);
        assert_eq!(infos[1].classification, StashClassification::Orphaned);
        assert_eq!(infos[2].classification, StashClassification::Active);
    }

    /// Helper to format an epoch day count as YYYY-MM-DD.
    fn format_date_from_epoch_days(days: i64) -> String {
        // Simple Hinnant civil_from_days
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    }
}
