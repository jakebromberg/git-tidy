use std::path::PathBuf;

use git_tidy_core::scan::{Classified, ScanResult};
use git_tidy_core::types::ClassificationLabel;
use serde::Serialize;

/// Classification of a remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteClassification {
    /// Remote is unreachable (ls-remote fails or times out).
    Unreachable,
    /// Tracking refs exist but remote is not configured.
    Orphaned,
    /// Remote is reachable and configured.
    Active,
}

impl ClassificationLabel for RemoteClassification {
    fn priority(&self) -> u8 {
        match self {
            Self::Unreachable => 0,
            Self::Orphaned => 1,
            Self::Active => 2,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Orphaned => "orphaned",
            Self::Active => "active",
        }
    }
}

/// Information about a single remote (configured or orphaned).
#[derive(Debug, Clone, Serialize)]
pub struct RemoteInfo {
    /// Path to the repo containing this remote.
    pub repo_path: PathBuf,
    /// Remote name.
    pub name: String,
    /// Classification of this remote.
    pub classification: RemoteClassification,
    /// URL of the remote (None for orphaned remotes).
    pub url: Option<String>,
    /// Number of tracking branches under this remote.
    pub tracking_count: usize,
    /// Whether this is the "origin" remote.
    pub is_origin: bool,
}

impl Classified for RemoteInfo {
    fn classification_label(&self) -> &str {
        self.classification.label()
    }
}

/// Result of a full remote scan.
///
/// Remotes are grouped per repo in `repos` as `RepoGroup<RemoteInfo>` (the
/// generic core group type); each group's `items` are sorted by classification
/// priority.
pub type RemoteScanResult = ScanResult<RemoteInfo>;

impl git_tidy_core::output::IntoJsonItem for RemoteInfo {
    type JsonItem = JsonRemote;

    fn to_json_item(&self) -> JsonRemote {
        JsonRemote::from(self)
    }
}

/// Flat JSON representation of a remote.
#[derive(Debug, Serialize)]
pub struct JsonRemote {
    pub repo_path: PathBuf,
    pub name: String,
    pub classification: String,
    pub url: Option<String>,
    pub tracking_count: usize,
    pub is_origin: bool,
}

impl From<&RemoteInfo> for JsonRemote {
    fn from(r: &RemoteInfo) -> Self {
        JsonRemote {
            repo_path: r.repo_path.clone(),
            name: r.name.clone(),
            classification: r.classification.label().to_string(),
            url: r.url.clone(),
            tracking_count: r.tracking_count,
            is_origin: r.is_origin,
        }
    }
}

#[cfg(test)]
mod tests {
    use git_tidy_core::counts::Counts;

    use super::*;

    #[test]
    fn classification_priority_order() {
        assert!(
            RemoteClassification::Unreachable.priority()
                < RemoteClassification::Orphaned.priority()
        );
        assert!(
            RemoteClassification::Orphaned.priority() < RemoteClassification::Active.priority()
        );
    }

    #[test]
    fn classification_labels() {
        assert_eq!(RemoteClassification::Unreachable.label(), "unreachable");
        assert_eq!(RemoteClassification::Orphaned.label(), "orphaned");
        assert_eq!(RemoteClassification::Active.label(), "active");
    }

    #[test]
    fn counts_increment_and_total() {
        // Also guards against label drift: increments via `classification.label()`.
        let mut counts = Counts::default();
        for c in [
            RemoteClassification::Unreachable,
            RemoteClassification::Orphaned,
            RemoteClassification::Active,
            RemoteClassification::Active,
        ] {
            counts.increment(c.label());
        }
        assert_eq!(counts.get("unreachable"), 1);
        assert_eq!(counts.get("orphaned"), 1);
        assert_eq!(counts.get("active"), 2);
        assert_eq!(counts.total(), 4);
    }

    #[test]
    fn json_remote_from_info() {
        let info = RemoteInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            name: "origin".to_string(),
            classification: RemoteClassification::Active,
            url: Some("https://github.com/user/repo.git".to_string()),
            tracking_count: 5,
            is_origin: true,
        };
        let json = JsonRemote::from(&info);
        assert_eq!(json.name, "origin");
        assert_eq!(json.classification, "active");
        assert_eq!(
            json.url,
            Some("https://github.com/user/repo.git".to_string())
        );
        assert_eq!(json.tracking_count, 5);
        assert!(json.is_origin);
    }

    #[test]
    fn json_remote_orphaned_null_url() {
        let info = RemoteInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            name: "stale".to_string(),
            classification: RemoteClassification::Orphaned,
            url: None,
            tracking_count: 3,
            is_origin: false,
        };
        let json = JsonRemote::from(&info);
        assert_eq!(json.classification, "orphaned");
        assert!(json.url.is_none());
        assert!(!json.is_origin);
    }
}
