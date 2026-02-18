use std::collections::HashSet;
use std::path::Path;

use git_tidy_core::discovery::discover_repos;
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;

use crate::types::{
    ConfigIssue, ConfigLintResult, ConfigRepoGroup, IssueCounts, IssueKind,
    parse_branch_from_config_key,
};

/// Lint a single repo for config issues.
///
/// `builtin_commands` is passed in so it can be cached across repos.
pub fn lint_repo(
    git: &dyn GitOps,
    repo_path: &Path,
    builtin_commands: &HashSet<String>,
) -> Result<Vec<ConfigIssue>, Error> {
    let config_entries = git.config_list_local(repo_path)?;
    let local_branches: HashSet<String> = git.list_local_branches(repo_path)?.into_iter().collect();

    let mut issues = Vec::new();

    // Check for orphaned branch config
    let mut seen_orphaned_branches: HashSet<String> = HashSet::new();
    for (key, value) in &config_entries {
        if let Some(branch_name) = parse_branch_from_config_key(key)
            && !local_branches.contains(branch_name)
            && seen_orphaned_branches.insert(branch_name.to_string())
        {
            let kind = IssueKind::OrphanedBranchConfig;
            issues.push(ConfigIssue {
                repo_path: repo_path.to_path_buf(),
                kind,
                severity: kind.severity(),
                key: key.clone(),
                value: value.clone(),
                message: format!("branch '{branch_name}' no longer exists locally"),
                section: Some(format!("branch.{branch_name}")),
            });
        }
    }

    // Check for alias shadows builtin
    for (key, value) in &config_entries {
        if let Some(alias_name) = key.strip_prefix("alias.")
            && builtin_commands.contains(alias_name)
        {
            let kind = IssueKind::AliasShadowsBuiltin;
            issues.push(ConfigIssue {
                repo_path: repo_path.to_path_buf(),
                kind,
                severity: kind.severity(),
                key: key.clone(),
                value: value.clone(),
                message: format!("alias '{alias_name}' shadows built-in git command"),
                section: None,
            });
        }
    }

    // Sort by priority
    issues.sort_by(|a, b| a.kind.priority().cmp(&b.kind.priority()));

    Ok(issues)
}

/// Run a config lint across all repos discovered under `directory`.
pub fn run_lint(
    git: &dyn GitOps,
    directory: &Path,
    progress: &Progress,
) -> Result<ConfigLintResult, Error> {
    let repo_paths = discover_repos(directory)?;

    let mut repos = Vec::new();
    let mut counts = IssueCounts::default();
    let mut warnings = Vec::new();
    let total_scanned = repo_paths.len();

    // Cache builtin commands (global, not per-repo)
    let builtin_commands: HashSet<String> = match git.list_builtin_commands() {
        Ok(cmds) => cmds.into_iter().collect(),
        Err(e) => {
            warnings.push(format!("could not list builtin commands: {e}"));
            HashSet::new()
        }
    };

    let pb = progress.bar(repo_paths.len() as u64, "Linting config");
    for repo_path in &repo_paths {
        let issues = match lint_repo(git, repo_path, &builtin_commands) {
            Ok(issues) => issues,
            Err(e) => {
                warnings.push(format!(
                    "could not lint config for {}: {e}",
                    repo_path.display()
                ));
                continue;
            }
        };

        if issues.is_empty() {
            continue;
        }

        for issue in &issues {
            counts.increment(issue.kind);
        }

        let repo_name = repo_display_name(repo_path);
        repos.push(ConfigRepoGroup {
            repo_path: repo_path.clone(),
            name: repo_name,
            issues,
        });
        pb.inc(1);
    }
    pb.finish_and_clear();

    Ok(ConfigLintResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use git_tidy_core::testutil::MockGitBuilder;

    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from("/repo")
    }

    #[test]
    fn lint_repo_detects_orphaned_branch_config() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![
                    (
                        "branch.old-feature.remote".to_string(),
                        "origin".to_string(),
                    ),
                    (
                        "branch.old-feature.merge".to_string(),
                        "refs/heads/old-feature".to_string(),
                    ),
                ],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins = HashSet::new();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, IssueKind::OrphanedBranchConfig);
        assert_eq!(issues[0].section, Some("branch.old-feature".to_string()));
        assert!(issues[0].message.contains("old-feature"));
    }

    #[test]
    fn lint_repo_skips_existing_branch_config() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![
                    ("branch.main.remote".to_string(), "origin".to_string()),
                    (
                        "branch.main.merge".to_string(),
                        "refs/heads/main".to_string(),
                    ),
                ],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins = HashSet::new();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert!(issues.is_empty());
    }

    #[test]
    fn lint_repo_detects_alias_shadows_builtin() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![("alias.log".to_string(), "log --oneline".to_string())],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins: HashSet<String> = ["log", "commit", "push"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, IssueKind::AliasShadowsBuiltin);
        assert!(issues[0].section.is_none());
        assert!(issues[0].message.contains("log"));
    }

    #[test]
    fn lint_repo_alias_not_shadowing_is_ignored() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![("alias.co".to_string(), "checkout".to_string())],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins: HashSet<String> = ["log", "commit"].iter().map(|s| s.to_string()).collect();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert!(issues.is_empty());
    }

    #[test]
    fn lint_repo_clean_config_no_issues() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![("user.email".to_string(), "test@test.com".to_string())],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins = HashSet::new();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert!(issues.is_empty());
    }

    #[test]
    fn lint_repo_deduplicates_orphaned_branch_entries() {
        // Both .remote and .merge for same branch should produce only 1 issue
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![
                    ("branch.stale.remote".to_string(), "origin".to_string()),
                    (
                        "branch.stale.merge".to_string(),
                        "refs/heads/stale".to_string(),
                    ),
                ],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins = HashSet::new();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn lint_repo_mixed_issues_sorted_by_priority() {
        let git = MockGitBuilder::new()
            .with_config_list_local(
                &repo(),
                vec![
                    ("alias.log".to_string(), "log --oneline".to_string()),
                    ("branch.gone.remote".to_string(), "origin".to_string()),
                ],
            )
            .with_local_branches(&repo(), vec!["main".to_string()])
            .build();

        let builtins: HashSet<String> = ["log"].iter().map(|s| s.to_string()).collect();
        let issues = lint_repo(&git, &repo(), &builtins).unwrap();

        assert_eq!(issues.len(), 2);
        // Orphaned branch config (priority 0) should come before alias (priority 1)
        assert_eq!(issues[0].kind, IssueKind::OrphanedBranchConfig);
        assert_eq!(issues[1].kind, IssueKind::AliasShadowsBuiltin);
    }
}
