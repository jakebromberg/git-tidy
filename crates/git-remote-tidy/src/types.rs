use std::path::PathBuf;

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

impl RemoteClassification {
    /// Priority for sorting (lower = more stale).
    pub fn priority(self) -> u8 {
        match self {
            Self::Unreachable => 0,
            Self::Orphaned => 1,
            Self::Active => 2,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
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

/// A group of remotes in the same repo.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteRepoGroup {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Remotes belonging to this repo, sorted by classification priority.
    pub remotes: Vec<RemoteInfo>,
}

/// Summary counts for a remote scan.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RemoteCounts {
    pub unreachable: usize,
    pub orphaned: usize,
    pub active: usize,
}

impl RemoteCounts {
    pub fn increment(&mut self, classification: RemoteClassification) {
        match classification {
            RemoteClassification::Unreachable => self.unreachable += 1,
            RemoteClassification::Orphaned => self.orphaned += 1,
            RemoteClassification::Active => self.active += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.unreachable + self.orphaned + self.active
    }
}

/// Result of a full remote scan.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteScanResult {
    /// Remotes grouped by repo.
    pub repos: Vec<RemoteRepoGroup>,
    /// Total remotes scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: RemoteCounts,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
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
        let mut counts = RemoteCounts::default();
        counts.increment(RemoteClassification::Unreachable);
        counts.increment(RemoteClassification::Orphaned);
        counts.increment(RemoteClassification::Active);
        counts.increment(RemoteClassification::Active);
        assert_eq!(counts.unreachable, 1);
        assert_eq!(counts.orphaned, 1);
        assert_eq!(counts.active, 2);
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
