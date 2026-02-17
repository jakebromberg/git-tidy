use std::path::PathBuf;

use serde::Serialize;

/// Classification of a stash entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StashClassification {
    /// Stash diff matches a branch tip (content already committed).
    Committed,
    /// Branch from stash message no longer exists locally.
    Orphaned,
    /// Older than the configured age threshold.
    Aged,
    /// None of the above -- still relevant.
    Active,
}

impl StashClassification {
    /// Priority for sorting (lower = more stale).
    pub fn priority(self) -> u8 {
        match self {
            Self::Committed => 0,
            Self::Orphaned => 1,
            Self::Aged => 2,
            Self::Active => 3,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Orphaned => "orphaned",
            Self::Aged => "aged",
            Self::Active => "active",
        }
    }
}

/// Information about a single stash entry.
#[derive(Debug, Clone, Serialize)]
pub struct StashInfo {
    /// Path to the repo containing this stash.
    pub repo_path: PathBuf,
    /// Stash reference (e.g. "stash@{0}").
    pub stash_ref: String,
    /// Classification of this stash.
    pub classification: StashClassification,
    /// Branch name extracted from stash message, if parseable.
    pub branch: Option<String>,
    /// Age in days since the stash was created.
    pub age_days: Option<u64>,
    /// Full stash message.
    pub message: String,
}

/// A group of stashes in the same repo.
#[derive(Debug, Clone, Serialize)]
pub struct StashRepoGroup {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Stashes belonging to this repo, sorted by classification priority.
    pub stashes: Vec<StashInfo>,
}

/// Summary counts for a stash scan.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StashCounts {
    pub committed: usize,
    pub orphaned: usize,
    pub aged: usize,
    pub active: usize,
}

impl StashCounts {
    pub fn increment(&mut self, classification: StashClassification) {
        match classification {
            StashClassification::Committed => self.committed += 1,
            StashClassification::Orphaned => self.orphaned += 1,
            StashClassification::Aged => self.aged += 1,
            StashClassification::Active => self.active += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.committed + self.orphaned + self.aged + self.active
    }
}

/// Result of a full stash scan.
#[derive(Debug, Clone, Serialize)]
pub struct StashScanResult {
    /// Stashes grouped by repo.
    pub repos: Vec<StashRepoGroup>,
    /// Total stashes scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: StashCounts,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
}

/// Flat JSON representation of a stash entry.
#[derive(Debug, Serialize)]
pub struct JsonStash {
    pub repo_path: PathBuf,
    pub stash_ref: String,
    pub classification: String,
    pub branch: Option<String>,
    pub age_days: Option<u64>,
    pub message: String,
}

impl From<&StashInfo> for JsonStash {
    fn from(s: &StashInfo) -> Self {
        JsonStash {
            repo_path: s.repo_path.clone(),
            stash_ref: s.stash_ref.clone(),
            classification: s.classification.label().to_string(),
            branch: s.branch.clone(),
            age_days: s.age_days,
            message: s.message.clone(),
        }
    }
}

/// Parse the branch name from a stash message.
///
/// Stash messages typically look like:
/// - "WIP on <branch>: <hash> <subject>"
/// - "On <branch>: <message>"
pub fn parse_stash_branch(message: &str) -> Option<String> {
    // Try "WIP on <branch>: ..." or "On <branch>: ..."
    let rest = message
        .strip_prefix("WIP on ")
        .or_else(|| message.strip_prefix("On "))?;

    let branch = rest.split(':').next()?;
    let branch = branch.trim();
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_branch_wip_format() {
        assert_eq!(
            parse_stash_branch("WIP on feature-x: abc1234 Add login"),
            Some("feature-x".to_string())
        );
    }

    #[test]
    fn parse_branch_on_format() {
        assert_eq!(
            parse_stash_branch("On main: temp changes"),
            Some("main".to_string())
        );
    }

    #[test]
    fn parse_branch_unknown_format() {
        assert_eq!(parse_stash_branch("some random message"), None);
    }

    #[test]
    fn parse_branch_empty() {
        assert_eq!(parse_stash_branch(""), None);
    }

    #[test]
    fn classification_priority_order() {
        assert!(
            StashClassification::Committed.priority() < StashClassification::Orphaned.priority()
        );
        assert!(StashClassification::Orphaned.priority() < StashClassification::Aged.priority());
        assert!(StashClassification::Aged.priority() < StashClassification::Active.priority());
    }

    #[test]
    fn counts_increment_and_total() {
        let mut counts = StashCounts::default();
        counts.increment(StashClassification::Committed);
        counts.increment(StashClassification::Orphaned);
        counts.increment(StashClassification::Active);
        assert_eq!(counts.committed, 1);
        assert_eq!(counts.orphaned, 1);
        assert_eq!(counts.active, 1);
        assert_eq!(counts.total(), 3);
    }
}
