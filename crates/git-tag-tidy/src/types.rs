use std::path::PathBuf;

use git_tidy_core::types::ClassificationLabel;
use serde::Serialize;

/// Classification of a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagClassification {
    /// Tag points at a commit not reachable from any branch.
    Stale,
    /// Tag exists locally but not on any configured remote.
    LocalOnly,
    /// Tag exists on remote but not locally.
    RemoteOnly,
    /// Tag exists both locally and on remote, commit is reachable.
    Synced,
}

impl ClassificationLabel for TagClassification {
    fn priority(&self) -> u8 {
        match self {
            Self::Stale => 0,
            Self::LocalOnly => 1,
            Self::RemoteOnly => 2,
            Self::Synced => 3,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::LocalOnly => "local_only",
            Self::RemoteOnly => "remote_only",
            Self::Synced => "synced",
        }
    }
}

/// Information about a single tag.
#[derive(Debug, Clone, Serialize)]
pub struct TagInfo {
    /// Path to the repo containing this tag.
    pub repo_path: PathBuf,
    /// Tag name.
    pub name: String,
    /// Classification of this tag.
    pub classification: TagClassification,
    /// Commit SHA the tag points at (empty for remote-only if unknown).
    pub commit: String,
    /// Whether this is an annotated tag.
    pub is_annotated: bool,
    /// ISO 8601 tagger/creator date, if available.
    pub tagger_date: Option<String>,
    /// Whether this tag matches a release version pattern (e.g., v1.0.0).
    pub is_release_tag: bool,
    /// Remotes that have this tag.
    pub remote_names: Vec<String>,
}

/// A group of tags in the same repo.
#[derive(Debug, Clone, Serialize)]
pub struct TagRepoGroup {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Tags belonging to this repo, sorted by classification priority.
    pub tags: Vec<TagInfo>,
}

git_tidy_core::define_counts!(TagCounts, TagClassification, {
    TagClassification::Stale => stale,
    TagClassification::LocalOnly => local_only,
    TagClassification::RemoteOnly => remote_only,
    TagClassification::Synced => synced,
});

/// Result of a full tag scan.
#[derive(Debug, Clone, Serialize)]
pub struct TagScanResult {
    /// Tags grouped by repo.
    pub repos: Vec<TagRepoGroup>,
    /// Total tags scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: TagCounts,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
}

impl git_tidy_core::output::FlatJsonItems for TagScanResult {
    type JsonItem = JsonTag;

    fn to_json_items(&self) -> Vec<JsonTag> {
        self.repos
            .iter()
            .flat_map(|g| g.tags.iter())
            .map(JsonTag::from)
            .collect()
    }
}

/// Flat JSON representation of a tag.
#[derive(Debug, Serialize)]
pub struct JsonTag {
    pub repo_path: PathBuf,
    pub name: String,
    pub classification: String,
    pub commit: String,
    pub is_annotated: bool,
    pub tagger_date: Option<String>,
    pub is_release_tag: bool,
    pub remote_names: Vec<String>,
}

impl From<&TagInfo> for JsonTag {
    fn from(t: &TagInfo) -> Self {
        JsonTag {
            repo_path: t.repo_path.clone(),
            name: t.name.clone(),
            classification: t.classification.label().to_string(),
            commit: t.commit.clone(),
            is_annotated: t.is_annotated,
            tagger_date: t.tagger_date.clone(),
            is_release_tag: t.is_release_tag,
            remote_names: t.remote_names.clone(),
        }
    }
}

/// Check if a tag name looks like a release version.
///
/// Matches: `v1.0`, `v1.0.0`, `V1.0`, `1.2.3`, `v1.0.0-rc1`.
/// Does not match: `version-bump`, `v-next`.
pub fn is_release_tag_name(name: &str) -> bool {
    let rest = name
        .strip_prefix('v')
        .or_else(|| name.strip_prefix('V'))
        .unwrap_or(name);
    rest.starts_with(|c: char| c.is_ascii_digit()) && rest.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_priority_order() {
        assert!(TagClassification::Stale.priority() < TagClassification::LocalOnly.priority());
        assert!(TagClassification::LocalOnly.priority() < TagClassification::RemoteOnly.priority());
        assert!(TagClassification::RemoteOnly.priority() < TagClassification::Synced.priority());
    }

    #[test]
    fn classification_labels() {
        assert_eq!(TagClassification::Stale.label(), "stale");
        assert_eq!(TagClassification::LocalOnly.label(), "local_only");
        assert_eq!(TagClassification::RemoteOnly.label(), "remote_only");
        assert_eq!(TagClassification::Synced.label(), "synced");
    }

    #[test]
    fn counts_increment_and_total() {
        let mut counts = TagCounts::default();
        counts.increment(&TagClassification::Stale);
        counts.increment(&TagClassification::LocalOnly);
        counts.increment(&TagClassification::RemoteOnly);
        counts.increment(&TagClassification::Synced);
        counts.increment(&TagClassification::Synced);
        assert_eq!(counts.stale, 1);
        assert_eq!(counts.local_only, 1);
        assert_eq!(counts.remote_only, 1);
        assert_eq!(counts.synced, 2);
        assert_eq!(counts.total(), 5);
    }

    #[test]
    fn json_tag_from_info() {
        let info = TagInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            name: "v1.0.0".to_string(),
            classification: TagClassification::Synced,
            commit: "abc1234".to_string(),
            is_annotated: true,
            tagger_date: Some("2024-06-15T10:00:00+00:00".to_string()),
            is_release_tag: true,
            remote_names: vec!["origin".to_string()],
        };
        let json = JsonTag::from(&info);
        assert_eq!(json.name, "v1.0.0");
        assert_eq!(json.classification, "synced");
        assert_eq!(json.commit, "abc1234");
        assert!(json.is_annotated);
        assert!(json.is_release_tag);
        assert_eq!(json.remote_names, vec!["origin"]);
    }

    #[test]
    fn json_tag_remote_only_empty_commit() {
        let info = TagInfo {
            repo_path: PathBuf::from("/repos/my-repo"),
            name: "v2.0.0".to_string(),
            classification: TagClassification::RemoteOnly,
            commit: "def5678".to_string(),
            is_annotated: false,
            tagger_date: None,
            is_release_tag: true,
            remote_names: vec!["origin".to_string()],
        };
        let json = JsonTag::from(&info);
        assert_eq!(json.classification, "remote_only");
        assert!(!json.is_annotated);
        assert!(json.tagger_date.is_none());
    }

    #[test]
    fn is_release_tag_name_v_prefix() {
        assert!(is_release_tag_name("v1.0"));
        assert!(is_release_tag_name("v1.0.0"));
        assert!(is_release_tag_name("v1.0.0-rc1"));
        assert!(is_release_tag_name("V1.0"));
    }

    #[test]
    fn is_release_tag_name_no_prefix() {
        assert!(is_release_tag_name("1.2.3"));
        assert!(is_release_tag_name("0.1.0"));
    }

    #[test]
    fn is_release_tag_name_rejects_non_release() {
        assert!(!is_release_tag_name("version-bump"));
        assert!(!is_release_tag_name("v-next"));
        assert!(!is_release_tag_name("feature-v2"));
        assert!(!is_release_tag_name("old-experiment"));
        assert!(!is_release_tag_name(""));
    }
}
