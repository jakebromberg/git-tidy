use std::path::PathBuf;

use git_tidy_core::scan::{Classified, ScanResult};
use git_tidy_core::types::{
    Classification, ClassificationLabel, UnmatchedCommit, extract_landed_fields,
};
use serde::Serialize;

/// Information about a single branch (local or remote-only).
#[derive(Debug, Clone, Serialize)]
pub struct BranchInfo {
    /// Path to the repo containing this branch.
    pub repo_path: PathBuf,
    /// Branch name.
    pub name: String,
    /// Default branch of the repo (e.g. "main").
    pub default_branch: String,
    /// Primary classification.
    pub classification: Classification,
    /// Whether the branch has a remote tracking branch on origin.
    pub remote_tracking: bool,
    /// Whether the remote tracking branch was deleted (pruned).
    pub remote_deleted: bool,
    /// Commits ahead of the default branch.
    pub ahead: usize,
    /// Commits behind the default branch.
    pub behind: usize,
    /// Whether the branch is more than the threshold behind.
    pub diverged: bool,
    /// Whether this branch is currently checked out.
    pub is_current: bool,
    /// Whether this branch exists only on the remote (no local counterpart).
    pub remote_only: bool,
}

impl Classified for BranchInfo {
    fn classification_label(&self) -> &str {
        self.classification.label()
    }
}

/// Result of a full branch scan.
///
/// Branches are grouped per repo in `repos` as `RepoGroup<BranchInfo>` (the
/// generic core group type); each group's `items` are sorted by classification
/// priority.
pub type BranchScanResult = ScanResult<BranchInfo>;

impl git_tidy_core::output::IntoJsonItem for BranchInfo {
    type JsonItem = JsonBranch;

    fn to_json_item(&self) -> JsonBranch {
        JsonBranch::from(self)
    }
}

/// Flat JSON representation of a branch.
#[derive(Debug, Serialize)]
pub struct JsonBranch {
    pub repo_path: PathBuf,
    pub name: String,
    pub default_branch: String,
    pub classification: String,
    pub remote_tracking: bool,
    pub remote_deleted: bool,
    pub ahead: usize,
    pub behind: usize,
    pub diverged: bool,
    pub is_current: bool,
    pub remote_only: bool,
    pub landed_ratio: Option<String>,
    pub landed_total: Option<usize>,
    pub unmatched_commits: Vec<UnmatchedCommit>,
}

impl From<&BranchInfo> for JsonBranch {
    fn from(b: &BranchInfo) -> Self {
        let landed = extract_landed_fields(&b.classification);

        JsonBranch {
            repo_path: b.repo_path.clone(),
            name: b.name.clone(),
            default_branch: b.default_branch.clone(),
            classification: b.classification.label().to_string(),
            remote_tracking: b.remote_tracking,
            remote_deleted: b.remote_deleted,
            ahead: b.ahead,
            behind: b.behind,
            diverged: b.diverged,
            is_current: b.is_current,
            remote_only: b.remote_only,
            landed_ratio: landed.ratio,
            landed_total: landed.total,
            unmatched_commits: landed.unmatched,
        }
    }
}
