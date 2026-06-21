use std::path::PathBuf;

use git_tidy_core::counts::Counts;
use git_tidy_core::output::{FlatJsonItems, IntoJsonItem};
use serde::Serialize;

/// Classification of a repository by activity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoClassification {
    /// No commits in N months, has reachable remote (safe to re-clone).
    Stale,
    /// No remote, or all remotes unreachable.
    Orphaned,
    /// Recent commits and/or reachable remote.
    Active,
}

impl RepoClassification {
    /// Priority for sorting (lower = more deletable).
    pub fn priority(self) -> u8 {
        match self {
            Self::Stale => 0,
            Self::Orphaned => 1,
            Self::Active => 2,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::Orphaned => "orphaned",
            Self::Active => "active",
        }
    }
}

/// Information about a single repository.
#[derive(Debug, Clone, Serialize)]
pub struct RepoInfo {
    /// Absolute path to the repository.
    pub path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Classification of this repo.
    pub classification: RepoClassification,
    /// ISO 8601 date of the most recent commit, if any.
    pub last_commit_date: Option<String>,
    /// Age of the most recent commit in days, if any.
    pub last_commit_age_days: Option<u64>,
    /// Disk usage in bytes.
    pub disk_usage_bytes: u64,
    /// URL of the first remote, if any.
    pub remote_url: Option<String>,
    /// Number of local branches.
    pub branch_count: usize,
    /// Whether the repo has at least one configured remote.
    pub has_remote: bool,
    /// Whether the repo has uncommitted changes (after noise filtering).
    pub is_dirty: bool,
    /// Number of dirty files (meaningful, after noise filtering).
    pub dirty_file_count: usize,
}

/// Result of a full repo scan.
#[derive(Debug, Clone, Serialize)]
pub struct RepoScanResult {
    /// Flat list of repos (no grouping needed).
    pub repos: Vec<RepoInfo>,
    /// Total repos scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: Counts,
    /// Cross-cutting count: repos with meaningful uncommitted changes
    /// (`is_dirty == true`). Not a classification bucket, so it is tracked
    /// separately from `counts`.
    pub dirty: usize,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
    /// Total disk usage of all scanned repos in bytes.
    pub total_disk_usage_bytes: u64,
    /// Reclaimable disk usage (stale + orphaned repos) in bytes.
    pub reclaimable_bytes: u64,
}

/// Flat JSON representation of a repo.
#[derive(Debug, Serialize)]
pub struct JsonRepo {
    pub path: PathBuf,
    pub name: String,
    pub classification: String,
    pub last_commit_date: Option<String>,
    pub last_commit_age_days: Option<u64>,
    pub disk_usage_bytes: u64,
    pub remote_url: Option<String>,
    pub branch_count: usize,
    pub has_remote: bool,
    pub is_dirty: bool,
    pub dirty_file_count: usize,
}

impl From<&RepoInfo> for JsonRepo {
    fn from(r: &RepoInfo) -> Self {
        JsonRepo {
            path: r.path.clone(),
            name: r.name.clone(),
            classification: r.classification.label().to_string(),
            last_commit_date: r.last_commit_date.clone(),
            last_commit_age_days: r.last_commit_age_days,
            disk_usage_bytes: r.disk_usage_bytes,
            remote_url: r.remote_url.clone(),
            branch_count: r.branch_count,
            has_remote: r.has_remote,
            is_dirty: r.is_dirty,
            dirty_file_count: r.dirty_file_count,
        }
    }
}

impl IntoJsonItem for RepoInfo {
    type JsonItem = JsonRepo;

    fn to_json_item(&self) -> JsonRepo {
        JsonRepo::from(self)
    }
}

/// `RepoScanResult` is bespoke (flat — no `RepoGroup` — and carries result-level
/// disk metrics and the cross-cutting `dirty` count), so it cannot use the
/// blanket `FlatJsonItems for ScanResult<T>` impl in core; it maps its flat
/// `repos` directly via [`IntoJsonItem`].
impl FlatJsonItems for RepoScanResult {
    type JsonItem = JsonRepo;

    fn to_json_items(&self) -> Vec<JsonRepo> {
        self.repos.iter().map(IntoJsonItem::to_json_item).collect()
    }
}

/// Format a byte count as a human-readable disk size.
///
/// Uses binary thresholds but decimal-style labels (KB, MB, GB) for familiarity.
pub fn format_disk_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        let value = bytes as f64 / GB as f64;
        if value >= 10.0 {
            format!("{:.0} GB", value)
        } else {
            format!("{:.1} GB", value)
        }
    } else if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Format the age of the last commit as a human-readable string.
pub fn format_last_commit_age(days: Option<u64>) -> String {
    match days {
        None => "no commits".to_string(),
        Some(0) => "today".to_string(),
        Some(1) => "1 day ago".to_string(),
        Some(d) => format!("{d} days ago"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_priority_order() {
        assert!(RepoClassification::Stale.priority() < RepoClassification::Orphaned.priority());
        assert!(RepoClassification::Orphaned.priority() < RepoClassification::Active.priority());
    }

    #[test]
    fn classification_labels() {
        assert_eq!(RepoClassification::Stale.label(), "stale");
        assert_eq!(RepoClassification::Orphaned.label(), "orphaned");
        assert_eq!(RepoClassification::Active.label(), "active");
    }

    #[test]
    fn counts_increment_and_total() {
        // Also guards against label drift: increments via `classification.label()`.
        // The cross-cutting `dirty` count now lives on `RepoScanResult`, not in
        // `Counts`; it is exercised by the scan tests.
        let mut counts = Counts::default();
        for c in [
            RepoClassification::Stale,
            RepoClassification::Orphaned,
            RepoClassification::Active,
            RepoClassification::Active,
        ] {
            counts.increment(c.label());
        }
        assert_eq!(counts.get("stale"), 1);
        assert_eq!(counts.get("orphaned"), 1);
        assert_eq!(counts.get("active"), 2);
        assert_eq!(counts.total(), 4);
    }

    #[test]
    fn format_disk_size_bytes() {
        assert_eq!(format_disk_size(500), "500 B");
    }

    #[test]
    fn format_disk_size_kilobytes() {
        assert_eq!(format_disk_size(1024), "1 KB");
        assert_eq!(format_disk_size(2048), "2 KB");
    }

    #[test]
    fn format_disk_size_megabytes() {
        assert_eq!(format_disk_size(1024 * 1024), "1 MB");
        assert_eq!(format_disk_size(142 * 1024 * 1024), "142 MB");
    }

    #[test]
    fn format_disk_size_gigabytes() {
        assert_eq!(format_disk_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_disk_size(12 * 1024 * 1024 * 1024), "12 GB");
    }

    #[test]
    fn format_disk_size_fractional_gb() {
        // 1.5 GB
        let bytes = 1024 * 1024 * 1024 + 512 * 1024 * 1024;
        assert_eq!(format_disk_size(bytes), "1.5 GB");
    }

    #[test]
    fn format_last_commit_age_today() {
        assert_eq!(format_last_commit_age(Some(0)), "today");
    }

    #[test]
    fn format_last_commit_age_one_day() {
        assert_eq!(format_last_commit_age(Some(1)), "1 day ago");
    }

    #[test]
    fn format_last_commit_age_many_days() {
        assert_eq!(format_last_commit_age(Some(549)), "549 days ago");
    }

    #[test]
    fn format_last_commit_age_none() {
        assert_eq!(format_last_commit_age(None), "no commits");
    }

    #[test]
    fn json_repo_from_info() {
        let info = RepoInfo {
            path: PathBuf::from("/repos/my-project"),
            name: "my-project".to_string(),
            classification: RepoClassification::Stale,
            last_commit_date: Some("2024-01-15T12:00:00+00:00".to_string()),
            last_commit_age_days: Some(549),
            disk_usage_bytes: 142 * 1024 * 1024,
            remote_url: Some("https://github.com/user/repo.git".to_string()),
            branch_count: 3,
            has_remote: true,
            is_dirty: false,
            dirty_file_count: 0,
        };
        let json = JsonRepo::from(&info);
        assert_eq!(json.name, "my-project");
        assert_eq!(json.classification, "stale");
        assert_eq!(json.last_commit_age_days, Some(549));
        assert!(!json.is_dirty);
    }

    #[test]
    fn json_repo_orphaned_no_remote() {
        let info = RepoInfo {
            path: PathBuf::from("/repos/orphan"),
            name: "orphan".to_string(),
            classification: RepoClassification::Orphaned,
            last_commit_date: Some("2023-06-01T12:00:00+00:00".to_string()),
            last_commit_age_days: Some(900),
            disk_usage_bytes: 89 * 1024 * 1024,
            remote_url: None,
            branch_count: 1,
            has_remote: false,
            is_dirty: true,
            dirty_file_count: 3,
        };
        let json = JsonRepo::from(&info);
        assert_eq!(json.classification, "orphaned");
        assert!(json.remote_url.is_none());
        assert!(json.is_dirty);
        assert_eq!(json.dirty_file_count, 3);
    }
}
