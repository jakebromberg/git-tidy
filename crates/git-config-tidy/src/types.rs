use std::path::PathBuf;

use serde::Serialize;

/// Severity of a config issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Likely a problem that should be fixed.
    Warning,
    /// Informational finding, no action needed.
    Info,
}

impl Severity {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// Kind of config issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// `branch.foo.remote` / `branch.foo.merge` exist but branch `foo` does not.
    OrphanedBranchConfig,
    /// `alias.X` shadows a built-in git command.
    AliasShadowsBuiltin,
}

impl IssueKind {
    /// Priority for sorting (lower = more important).
    pub fn priority(self) -> u8 {
        match self {
            Self::OrphanedBranchConfig => 0,
            Self::AliasShadowsBuiltin => 1,
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::OrphanedBranchConfig => "orphaned_branch_config",
            Self::AliasShadowsBuiltin => "alias_shadows_builtin",
        }
    }

    /// Severity for this kind of issue.
    pub fn severity(self) -> Severity {
        match self {
            Self::OrphanedBranchConfig => Severity::Warning,
            Self::AliasShadowsBuiltin => Severity::Info,
        }
    }
}

/// A single config issue found in a repo.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigIssue {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Kind of issue.
    pub kind: IssueKind,
    /// Severity of the issue.
    pub severity: Severity,
    /// The config key (e.g., `branch.foo.remote`).
    pub key: String,
    /// The config value.
    pub value: String,
    /// Human-readable description.
    pub message: String,
    /// The config section to remove for fixing (e.g., `branch.foo`).
    /// Only set for fixable issues.
    pub section: Option<String>,
}

/// A group of config issues in the same repo.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigRepoGroup {
    /// Path to the repo.
    pub repo_path: PathBuf,
    /// Display name (directory basename).
    pub name: String,
    /// Issues in this repo, sorted by priority.
    pub issues: Vec<ConfigIssue>,
}

/// Summary counts by issue kind.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IssueCounts {
    pub orphaned_branch_config: usize,
    pub alias_shadows_builtin: usize,
}

impl IssueCounts {
    pub fn increment(&mut self, kind: IssueKind) {
        match kind {
            IssueKind::OrphanedBranchConfig => self.orphaned_branch_config += 1,
            IssueKind::AliasShadowsBuiltin => self.alias_shadows_builtin += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.orphaned_branch_config + self.alias_shadows_builtin
    }
}

/// Result of a full config lint.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigLintResult {
    /// Issues grouped by repo.
    pub repos: Vec<ConfigRepoGroup>,
    /// Total repos scanned.
    pub total_scanned: usize,
    /// Summary counts by kind.
    pub counts: IssueCounts,
    /// Warnings encountered during scanning.
    pub warnings: Vec<String>,
}

/// Flat JSON representation of a config issue.
#[derive(Debug, Serialize)]
pub struct JsonConfigIssue {
    pub repo_path: PathBuf,
    pub kind: String,
    pub severity: String,
    pub key: String,
    pub value: String,
    pub message: String,
}

impl From<&ConfigIssue> for JsonConfigIssue {
    fn from(issue: &ConfigIssue) -> Self {
        JsonConfigIssue {
            repo_path: issue.repo_path.clone(),
            kind: issue.kind.label().to_string(),
            severity: issue.severity.label().to_string(),
            key: issue.key.clone(),
            value: issue.value.clone(),
            message: issue.message.clone(),
        }
    }
}

/// Extract the branch name from a config key like `branch.<name>.remote` or `branch.<name>.merge`.
///
/// Uses `rsplit_once('.')` after stripping `branch.` to handle branch names containing dots
/// (e.g., `branch.fix.remote-bug.remote` -> `fix.remote-bug`).
pub fn parse_branch_from_config_key(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("branch.")?;
    let (branch_name, suffix) = rest.rsplit_once('.')?;
    if suffix == "remote" || suffix == "merge" {
        Some(branch_name)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_labels() {
        assert_eq!(Severity::Warning.label(), "warning");
        assert_eq!(Severity::Info.label(), "info");
    }

    #[test]
    fn issue_kind_priority_order() {
        assert!(
            IssueKind::OrphanedBranchConfig.priority() < IssueKind::AliasShadowsBuiltin.priority()
        );
    }

    #[test]
    fn issue_kind_labels() {
        assert_eq!(
            IssueKind::OrphanedBranchConfig.label(),
            "orphaned_branch_config"
        );
        assert_eq!(
            IssueKind::AliasShadowsBuiltin.label(),
            "alias_shadows_builtin"
        );
    }

    #[test]
    fn issue_kind_severity() {
        assert_eq!(
            IssueKind::OrphanedBranchConfig.severity(),
            Severity::Warning
        );
        assert_eq!(IssueKind::AliasShadowsBuiltin.severity(), Severity::Info);
    }

    #[test]
    fn counts_increment_and_total() {
        let mut counts = IssueCounts::default();
        counts.increment(IssueKind::OrphanedBranchConfig);
        counts.increment(IssueKind::OrphanedBranchConfig);
        counts.increment(IssueKind::AliasShadowsBuiltin);
        assert_eq!(counts.orphaned_branch_config, 2);
        assert_eq!(counts.alias_shadows_builtin, 1);
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn json_config_issue_from_issue() {
        let issue = ConfigIssue {
            repo_path: PathBuf::from("/repos/my-repo"),
            kind: IssueKind::OrphanedBranchConfig,
            severity: Severity::Warning,
            key: "branch.old-feature.remote".to_string(),
            value: "origin".to_string(),
            message: "branch 'old-feature' no longer exists locally".to_string(),
            section: Some("branch.old-feature".to_string()),
        };
        let json = JsonConfigIssue::from(&issue);
        assert_eq!(json.kind, "orphaned_branch_config");
        assert_eq!(json.severity, "warning");
        assert_eq!(json.key, "branch.old-feature.remote");
        assert_eq!(json.value, "origin");
    }

    #[test]
    fn parse_branch_simple() {
        assert_eq!(
            parse_branch_from_config_key("branch.feature.remote"),
            Some("feature")
        );
        assert_eq!(
            parse_branch_from_config_key("branch.feature.merge"),
            Some("feature")
        );
    }

    #[test]
    fn parse_branch_with_slash() {
        assert_eq!(
            parse_branch_from_config_key("branch.feature/login.remote"),
            Some("feature/login")
        );
    }

    #[test]
    fn parse_branch_with_dot() {
        assert_eq!(
            parse_branch_from_config_key("branch.fix.remote-bug.remote"),
            Some("fix.remote-bug")
        );
    }

    #[test]
    fn parse_branch_rejects_non_branch_keys() {
        assert_eq!(parse_branch_from_config_key("user.email"), None);
        assert_eq!(parse_branch_from_config_key("alias.co"), None);
        assert_eq!(parse_branch_from_config_key("branch.feature.vmerge"), None);
    }

    #[test]
    fn parse_branch_rejects_unknown_suffix() {
        assert_eq!(parse_branch_from_config_key("branch.feature.rebase"), None);
    }
}
