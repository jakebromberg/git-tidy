use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;

use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;

use crate::types::{ConfigLintResult, IssueKind};

/// Options controlling config fix behavior.
pub struct FixOptions {
    /// Preview only: print what would be fixed.
    pub dry_run: bool,
    /// Skip confirmation prompts.
    #[allow(dead_code)]
    pub yes: bool,
}

/// A config issue that was successfully fixed.
#[derive(Debug)]
#[allow(dead_code)]
pub struct FixedIssue {
    pub repo: PathBuf,
    pub section: String,
    pub kind: IssueKind,
}

/// A config issue that failed to fix.
#[derive(Debug)]
#[allow(dead_code)]
pub struct FailedFix {
    pub repo: PathBuf,
    pub section: String,
    pub kind: IssueKind,
    pub reason: String,
}

/// Result of a fix operation.
#[derive(Debug)]
#[allow(dead_code)]
pub struct FixResult {
    /// Issues that were fixed (or would be in dry-run).
    pub fixed: Vec<FixedIssue>,
    /// Issues that failed to fix.
    pub failed: Vec<FailedFix>,
    /// Issues that were skipped (not auto-fixable).
    pub skipped: usize,
}

/// Run the fix operation on a lint result.
pub fn run_fix(
    git: &dyn GitOps,
    lint_result: &ConfigLintResult,
    options: &FixOptions,
    out: &mut dyn Write,
) -> Result<FixResult, Error> {
    let mut fixed = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = 0;

    // Deduplicate sections to fix: both .remote and .merge map to the same
    // branch.<name> section, so we only need to remove it once.
    let mut fixed_sections: HashSet<(PathBuf, String)> = HashSet::new();

    for group in &lint_result.repos {
        for issue in &group.issues {
            // Only OrphanedBranchConfig is auto-fixable
            let Some(section) = &issue.section else {
                skipped += 1;
                continue;
            };

            let section_key = (group.repo_path.clone(), section.clone());
            if !fixed_sections.insert(section_key) {
                // Already handled this section
                continue;
            }

            if options.dry_run {
                writeln!(out, "would remove section [{section}] in {}", group.name,)?;
                fixed.push(FixedIssue {
                    repo: group.repo_path.clone(),
                    section: section.clone(),
                    kind: issue.kind,
                });
                continue;
            }

            match git.config_remove_section(&group.repo_path, section) {
                Ok(()) => {
                    writeln!(out, "removed section [{section}] in {}", group.name,)?;
                    fixed.push(FixedIssue {
                        repo: group.repo_path.clone(),
                        section: section.clone(),
                        kind: issue.kind,
                    });
                }
                Err(e) => {
                    writeln!(
                        out,
                        "error: could not remove section [{section}] in {}: {e}",
                        group.name,
                    )?;
                    failed.push(FailedFix {
                        repo: group.repo_path.clone(),
                        section: section.clone(),
                        kind: issue.kind,
                        reason: e.to_string(),
                    });
                }
            }
        }
    }

    Ok(FixResult {
        fixed,
        failed,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;
    use crate::types::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn make_lint_result(issues: Vec<ConfigIssue>) -> ConfigLintResult {
        let mut counts = IssueCounts::default();
        for issue in &issues {
            counts.increment(issue.kind);
        }
        ConfigLintResult {
            repos: vec![ConfigRepoGroup {
                repo_path: repo(),
                name: "my-repo".to_string(),
                issues,
            }],
            total_scanned: 1,
            counts,
            warnings: vec![],
        }
    }

    fn orphaned_issue(branch: &str) -> ConfigIssue {
        ConfigIssue {
            repo_path: repo(),
            kind: IssueKind::OrphanedBranchConfig,
            severity: Severity::Warning,
            key: format!("branch.{branch}.remote"),
            value: "origin".to_string(),
            message: format!("branch '{branch}' no longer exists locally"),
            section: Some(format!("branch.{branch}")),
        }
    }

    fn alias_issue(alias: &str) -> ConfigIssue {
        ConfigIssue {
            repo_path: repo(),
            kind: IssueKind::AliasShadowsBuiltin,
            severity: Severity::Info,
            key: format!("alias.{alias}"),
            value: format!("{alias} --oneline"),
            message: format!("alias '{alias}' shadows built-in git command"),
            section: None,
        }
    }

    fn default_options() -> FixOptions {
        FixOptions {
            dry_run: false,
            yes: false,
        }
    }

    #[test]
    fn fix_removes_orphaned_config() {
        let git = MockGitBuilder::new().build();
        let lint = make_lint_result(vec![orphaned_issue("old-feature")]);
        let mut buf = Vec::new();

        let result = run_fix(&git, &lint, &default_options(), &mut buf).unwrap();

        assert_eq!(result.fixed.len(), 1);
        assert_eq!(result.fixed[0].section, "branch.old-feature");
        assert_eq!(git.config_remove_section_calls().len(), 1);
        assert_eq!(
            git.config_remove_section_calls()[0],
            (repo(), "branch.old-feature".to_string())
        );

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed section [branch.old-feature]"));
    }

    #[test]
    fn fix_skips_non_fixable_issues() {
        let git = MockGitBuilder::new().build();
        let lint = make_lint_result(vec![alias_issue("log")]);
        let mut buf = Vec::new();

        let result = run_fix(&git, &lint, &default_options(), &mut buf).unwrap();

        assert_eq!(result.fixed.len(), 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.config_remove_section_calls().len(), 0);
    }

    #[test]
    fn fix_dry_run_makes_zero_calls() {
        let git = MockGitBuilder::new().build();
        let lint = make_lint_result(vec![orphaned_issue("stale-branch")]);
        let mut buf = Vec::new();
        let options = FixOptions {
            dry_run: true,
            yes: false,
        };

        let result = run_fix(&git, &lint, &options, &mut buf).unwrap();

        assert_eq!(result.fixed.len(), 1);
        assert_eq!(git.config_remove_section_calls().len(), 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("would remove section [branch.stale-branch]"));
    }

    #[test]
    fn fix_handles_removal_failure() {
        let git = MockGitBuilder::new()
            .with_config_remove_section_error(&repo(), "branch.bad", "permission denied")
            .build();
        let lint = make_lint_result(vec![orphaned_issue("bad")]);
        let mut buf = Vec::new();

        let result = run_fix(&git, &lint, &default_options(), &mut buf).unwrap();

        assert_eq!(result.fixed.len(), 0);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].section, "branch.bad");

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("error: could not remove section [branch.bad]"));
    }

    #[test]
    fn fix_mixed_fixable_and_non_fixable() {
        let git = MockGitBuilder::new().build();
        let lint = make_lint_result(vec![orphaned_issue("gone"), alias_issue("log")]);
        let mut buf = Vec::new();

        let result = run_fix(&git, &lint, &default_options(), &mut buf).unwrap();

        assert_eq!(result.fixed.len(), 1);
        assert_eq!(result.skipped, 1);
        assert_eq!(git.config_remove_section_calls().len(), 1);
    }
}
