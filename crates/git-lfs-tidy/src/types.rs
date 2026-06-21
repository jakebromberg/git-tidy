use std::path::PathBuf;

use git_tidy_core::counts::Counts;
use git_tidy_core::output::{FlatJsonItems, IntoJsonItem};
use serde::Serialize;

/// Classification of an LFS item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LfsClassification {
    /// Large blob not tracked by LFS.
    Untracked,
    /// LFS pointer exists but object missing locally.
    Missing,
    /// Prunable LFS objects (no branch refs).
    Orphaned,
    /// Properly tracked and present.
    Healthy,
}

impl LfsClassification {
    /// Priority for sorting (lower = more actionable).
    pub fn priority(self) -> u8 {
        match self {
            Self::Untracked => 0,
            Self::Missing => 1,
            Self::Orphaned => 2,
            Self::Healthy => 3,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Untracked => "untracked",
            Self::Missing => "missing",
            Self::Orphaned => "orphaned",
            Self::Healthy => "healthy",
        }
    }
}

/// Information about a single LFS item.
#[derive(Debug, Clone, Serialize)]
pub struct LfsInfo {
    /// Path to the repo containing this item.
    pub repo_path: PathBuf,
    /// File path (or synthetic description for orphaned).
    pub path: String,
    /// Classification of this item.
    pub classification: LfsClassification,
    /// LFS OID or blob hash.
    pub oid: String,
    /// Size in bytes, if known.
    pub size_bytes: Option<u64>,
}

/// A group of LFS items in the same repo.
#[derive(Debug, Clone, Serialize)]
pub struct LfsRepoGroup {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// LFS items belonging to this repo, sorted by classification priority.
    pub items: Vec<LfsInfo>,
    /// Whether git-lfs is available for this repo.
    pub lfs_available: bool,
    /// Current LFS tracking patterns.
    pub track_patterns: Vec<String>,
}

/// Result of a full LFS scan.
#[derive(Debug, Clone, Serialize)]
pub struct LfsScanResult {
    /// LFS items grouped by repo.
    pub repos: Vec<LfsRepoGroup>,
    /// Total items scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: Counts,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
    /// Whether git-lfs is installed globally.
    pub lfs_installed: bool,
}

/// Flat JSON representation of an LFS item.
#[derive(Debug, Serialize)]
pub struct JsonLfsItem {
    pub repo_path: PathBuf,
    pub path: String,
    pub classification: String,
    pub oid: String,
    pub size_bytes: Option<u64>,
}

impl From<&LfsInfo> for JsonLfsItem {
    fn from(info: &LfsInfo) -> Self {
        JsonLfsItem {
            repo_path: info.repo_path.clone(),
            path: info.path.clone(),
            classification: info.classification.label().to_string(),
            oid: info.oid.clone(),
            size_bytes: info.size_bytes,
        }
    }
}

impl IntoJsonItem for LfsInfo {
    type JsonItem = JsonLfsItem;

    fn to_json_item(&self) -> JsonLfsItem {
        JsonLfsItem::from(self)
    }
}

/// `LfsScanResult` is bespoke (it carries the result-level `lfs_installed`
/// flag), so it cannot use the blanket `FlatJsonItems for ScanResult<T>` impl in
/// core; it flattens its groups' items the same way, via [`IntoJsonItem`].
impl FlatJsonItems for LfsScanResult {
    type JsonItem = JsonLfsItem;

    fn to_json_items(&self) -> Vec<JsonLfsItem> {
        self.repos
            .iter()
            .flat_map(|g| g.items.iter())
            .map(IntoJsonItem::to_json_item)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_priority_order() {
        assert!(LfsClassification::Untracked.priority() < LfsClassification::Missing.priority());
        assert!(LfsClassification::Missing.priority() < LfsClassification::Orphaned.priority());
        assert!(LfsClassification::Orphaned.priority() < LfsClassification::Healthy.priority());
    }

    #[test]
    fn classification_labels() {
        assert_eq!(LfsClassification::Untracked.label(), "untracked");
        assert_eq!(LfsClassification::Missing.label(), "missing");
        assert_eq!(LfsClassification::Orphaned.label(), "orphaned");
        assert_eq!(LfsClassification::Healthy.label(), "healthy");
    }

    #[test]
    fn counts_increment_and_total() {
        // Also guards against label drift: increments via `classification.label()`.
        let mut counts = Counts::default();
        for c in [
            LfsClassification::Untracked,
            LfsClassification::Missing,
            LfsClassification::Orphaned,
            LfsClassification::Healthy,
            LfsClassification::Healthy,
        ] {
            counts.increment(c.label());
        }
        assert_eq!(counts.get("untracked"), 1);
        assert_eq!(counts.get("missing"), 1);
        assert_eq!(counts.get("orphaned"), 1);
        assert_eq!(counts.get("healthy"), 2);
        assert_eq!(counts.total(), 5);
    }

    #[test]
    fn json_lfs_item_from_info() {
        let info = LfsInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            path: "large-file.bin".to_string(),
            classification: LfsClassification::Untracked,
            oid: "abc1234".to_string(),
            size_bytes: Some(1_048_576),
        };
        let json = JsonLfsItem::from(&info);
        assert_eq!(json.path, "large-file.bin");
        assert_eq!(json.classification, "untracked");
        assert_eq!(json.oid, "abc1234");
        assert_eq!(json.size_bytes, Some(1_048_576));
    }

    #[test]
    fn json_lfs_item_missing_size() {
        let info = LfsInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            path: "tracked.bin".to_string(),
            classification: LfsClassification::Healthy,
            oid: "def5678".to_string(),
            size_bytes: None,
        };
        let json = JsonLfsItem::from(&info);
        assert_eq!(json.classification, "healthy");
        assert!(json.size_bytes.is_none());
    }
}
