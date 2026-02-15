use std::path::PathBuf;

use serde::Serialize;

/// Primary staleness classification for a worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    /// Branch tip is an ancestor of the default branch.
    Merged,
    /// All branch commits have matching commits on the default branch.
    Landed { matched: usize, total: usize },
    /// Some but not all branch commits matched.
    LandedPartial {
        matched: usize,
        total: usize,
        unmatched: Vec<UnmatchedCommit>,
    },
    /// Has a remote tracking branch; not merged or landed.
    Active,
    /// No remote tracking branch; not merged or landed.
    Local,
}

impl Classification {
    /// Sort priority: merged=0, landed=1, partial=2, active=3, local=4.
    pub fn priority(&self) -> u8 {
        match self {
            Classification::Merged => 0,
            Classification::Landed { .. } => 1,
            Classification::LandedPartial { .. } => 2,
            Classification::Active => 3,
            Classification::Local => 4,
        }
    }

    /// Short label for display.
    pub fn label(&self) -> &'static str {
        match self {
            Classification::Merged => "merged",
            Classification::Landed { .. } => "landed",
            Classification::LandedPartial { .. } => "partial",
            Classification::Active => "active",
            Classification::Local => "local",
        }
    }
}

/// A branch commit that did not match any commit on the default branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnmatchedCommit {
    pub short_hash: String,
    pub subject: String,
}

/// Orthogonal annotations on a worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Annotations {
    /// The remote tracking branch no longer exists after fetch --prune.
    pub remote_deleted: bool,
    /// Branch is more than --behind-threshold commits behind.
    pub diverged: bool,
    /// Working tree has meaningful uncommitted changes.
    pub dirty: bool,
    /// Number of meaningful dirty files.
    pub dirty_file_count: usize,
}

/// Information about a single linked worktree.
#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
    /// Absolute path to the parent (main) repo.
    pub parent_repo: PathBuf,
    /// Branch name checked out in this worktree (None if detached HEAD).
    pub branch: Option<String>,
    /// Default branch of the parent repo (e.g. "main").
    pub default_branch: String,
    /// Primary classification.
    pub classification: Classification,
    /// Orthogonal annotations.
    pub annotations: Annotations,
    /// Whether the branch has a remote tracking branch on origin.
    pub remote_tracking: bool,
    /// Commits ahead of the default branch.
    pub ahead: usize,
    /// Commits behind the default branch.
    pub behind: usize,
    /// All dirty file paths (before noise filtering).
    pub dirty_files: Vec<String>,
    /// Meaningful dirty file paths (after noise filtering).
    pub meaningful_dirty_files: Vec<String>,
}

/// A group of worktrees sharing the same parent repo.
#[derive(Debug, Clone, Serialize)]
pub struct RepoGroup {
    /// Path to the parent repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Worktrees belonging to this repo, sorted by classification priority.
    pub worktrees: Vec<WorktreeInfo>,
}

/// Summary counts across all scanned worktrees.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanCounts {
    pub merged: usize,
    pub landed: usize,
    pub partial: usize,
    pub active: usize,
    pub local: usize,
}

impl ScanCounts {
    pub fn total(&self) -> usize {
        self.merged + self.landed + self.partial + self.active + self.local
    }

    pub fn increment(&mut self, classification: &Classification) {
        match classification {
            Classification::Merged => self.merged += 1,
            Classification::Landed { .. } => self.landed += 1,
            Classification::LandedPartial { .. } => self.partial += 1,
            Classification::Active => self.active += 1,
            Classification::Local => self.local += 1,
        }
    }
}

/// Flat JSON representation of a worktree matching the spec.
#[derive(Debug, Serialize)]
pub struct JsonWorktree {
    pub path: PathBuf,
    pub parent_repo: PathBuf,
    pub branch: Option<String>,
    pub default_branch: String,
    pub classification: String,
    pub remote_tracking: bool,
    pub remote_deleted: bool,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: bool,
    pub dirty_files: Vec<String>,
    pub meaningful_dirty_files: Vec<String>,
    pub diverged: bool,
    pub landed_ratio: Option<String>,
    pub landed_total: Option<usize>,
    pub unmatched_commits: Vec<UnmatchedCommit>,
}

impl From<&WorktreeInfo> for JsonWorktree {
    fn from(wt: &WorktreeInfo) -> Self {
        let (landed_ratio, landed_total, unmatched_commits) = match &wt.classification {
            Classification::Landed { matched, total } => {
                (Some(format!("{matched}/{total}")), Some(*total), vec![])
            }
            Classification::LandedPartial {
                matched,
                total,
                unmatched,
            } => (
                Some(format!("{matched}/{total}")),
                Some(*total),
                unmatched.clone(),
            ),
            _ => (None, None, vec![]),
        };

        JsonWorktree {
            path: wt.path.clone(),
            parent_repo: wt.parent_repo.clone(),
            branch: wt.branch.clone(),
            default_branch: wt.default_branch.clone(),
            classification: wt.classification.label().to_string(),
            remote_tracking: wt.remote_tracking,
            remote_deleted: wt.annotations.remote_deleted,
            ahead: wt.ahead,
            behind: wt.behind,
            dirty: wt.annotations.dirty,
            dirty_files: wt.dirty_files.clone(),
            meaningful_dirty_files: wt.meaningful_dirty_files.clone(),
            diverged: wt.annotations.diverged,
            landed_ratio,
            landed_total,
            unmatched_commits,
        }
    }
}

/// Result of a full scan operation.
#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    /// Worktrees grouped by parent repo.
    pub repos: Vec<RepoGroup>,
    /// Total worktrees scanned.
    pub total_scanned: usize,
    /// Summary counts by classification.
    pub counts: ScanCounts,
    /// Repos that were skipped (e.g. no default branch).
    pub warnings: Vec<String>,
}
