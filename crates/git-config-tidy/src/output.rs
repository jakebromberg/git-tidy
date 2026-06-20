use std::io::Write;

use git_tidy_core::output as shared;

use crate::types::{ConfigIssue, ConfigLintResult, JsonConfigIssue};

const HEADER_SEVERITY: &str = "SEVERITY";
const HEADER_KIND: &str = "KIND";
const HEADER_SETTING: &str = "SETTING";
const HEADER_MESSAGE: &str = "MESSAGE";

struct ColumnWidths {
    severity: usize,
    kind: usize,
    setting: usize,
}

fn compute_column_widths(issues: &[ConfigIssue]) -> ColumnWidths {
    let mut max_severity = HEADER_SEVERITY.len();
    let mut max_kind = HEADER_KIND.len();
    let mut max_setting = HEADER_SETTING.len();

    for i in issues {
        max_severity = max_severity.max(i.severity.label().len());
        max_kind = max_kind.max(i.kind.label().len());
        // key=value combined
        max_setting = max_setting.max(i.key.len() + 1 + i.value.len());
    }

    ColumnWidths {
        severity: max_severity,
        kind: max_kind,
        setting: max_setting,
    }
}

fn write_header(out: &mut dyn Write, widths: &ColumnWidths) -> std::io::Result<()> {
    let sw = widths.severity;
    let kw = widths.kind;
    let stw = widths.setting;
    let line = format!(
        "  {HEADER_SEVERITY:<sw$} {HEADER_KIND:<kw$} {HEADER_SETTING:<stw$} {HEADER_MESSAGE}"
    );
    let trimmed = line.trim_end();
    writeln!(out, "{trimmed}")
}

/// Write human-readable lint output.
pub fn write_human(out: &mut dyn Write, result: &ConfigLintResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.issues.len() == 1 {
            "issue"
        } else {
            "issues"
        };
        writeln!(out, "\n{} ({} {noun})", group.name, group.issues.len())?;

        let widths = compute_column_widths(&group.issues);
        write_header(out, &widths)?;

        for issue in &group.issues {
            let setting = format!("{}={}", issue.key, issue.value);

            let sw = widths.severity;
            let kw = widths.kind;
            let stw = widths.setting;
            let line = format!(
                "  {:<sw$} {:<kw$} {setting:<stw$} {}",
                issue.severity.label(),
                issue.kind.label(),
                issue.message,
            );
            let trimmed = line.trim_end();
            writeln!(out, "{trimmed}")?;
        }
    }

    write_lint_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the lint-specific summary line.
fn write_lint_summary(out: &mut dyn Write, result: &ConfigLintResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} repos scanned: {} orphaned branch config, {} alias shadows builtin",
        result.total_scanned,
        c.get("orphaned_branch_config"),
        c.get("alias_shadows_builtin"),
    )
}

/// Write JSON lint output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &ConfigLintResult) -> std::io::Result<()> {
    let all_issues: Vec<JsonConfigIssue> = result
        .repos
        .iter()
        .flat_map(|g| g.issues.iter())
        .map(JsonConfigIssue::from)
        .collect();

    shared::write_json_pretty(out, &all_issues)
}

/// Write porcelain (machine-readable, tab-delimited) lint output.
pub fn write_porcelain(out: &mut dyn Write, result: &ConfigLintResult) -> std::io::Result<()> {
    for group in &result.repos {
        for issue in &group.issues {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                issue.repo_path.display(),
                issue.kind.label(),
                issue.severity.label(),
                issue.key,
                issue.value,
                issue.message,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;

    fn make_lint_result() -> ConfigLintResult {
        ConfigLintResult {
            repos: vec![ConfigRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                issues: vec![
                    ConfigIssue {
                        repo_path: PathBuf::from("/repos/backend"),
                        kind: IssueKind::OrphanedBranchConfig,
                        severity: Severity::Warning,
                        key: "branch.old-feature.remote".to_string(),
                        value: "origin".to_string(),
                        message: "branch 'old-feature' no longer exists locally".to_string(),
                        section: Some("branch.old-feature".to_string()),
                    },
                    ConfigIssue {
                        repo_path: PathBuf::from("/repos/backend"),
                        kind: IssueKind::AliasShadowsBuiltin,
                        severity: Severity::Info,
                        key: "alias.log".to_string(),
                        value: "log --oneline".to_string(),
                        message: "alias 'log' shadows built-in git command".to_string(),
                        section: None,
                    },
                ],
            }],
            total_scanned: 3,
            counts: Counts::from_pairs(&[
                ("orphaned_branch_config", 1),
                ("alias_shadows_builtin", 1),
            ]),
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_lint_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("backend (2 issues)"));
        // Header row
        assert!(output.contains("SEVERITY"));
        assert!(output.contains("KIND"));
        assert!(output.contains("SETTING"));
        assert!(output.contains("MESSAGE"));
        assert!(output.contains("warning"));
        assert!(output.contains("orphaned_branch_config"));
        assert!(output.contains("branch.old-feature.remote=origin"));
        assert!(output.contains("info"));
        assert!(output.contains("alias_shadows_builtin"));
        assert!(output.contains("alias.log=log --oneline"));
        assert!(
            output.contains("3 repos scanned: 1 orphaned branch config, 1 alias shadows builtin")
        );
    }

    #[test]
    fn human_output_with_warnings() {
        let result = ConfigLintResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["could not read config for /repo/bad".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not read config for /repo/bad"));
    }

    #[test]
    fn human_output_single_issue_noun() {
        let result = ConfigLintResult {
            repos: vec![ConfigRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                issues: vec![ConfigIssue {
                    repo_path: PathBuf::from("/repos/test"),
                    kind: IssueKind::OrphanedBranchConfig,
                    severity: Severity::Warning,
                    key: "branch.gone.remote".to_string(),
                    value: "origin".to_string(),
                    message: "branch 'gone' no longer exists locally".to_string(),
                    section: Some("branch.gone".to_string()),
                }],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("orphaned_branch_config", 1)]),
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test (1 issue)"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_lint_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["kind"], "orphaned_branch_config");
        assert_eq!(arr[0]["severity"], "warning");
        assert_eq!(arr[0]["key"], "branch.old-feature.remote");
        assert_eq!(arr[1]["kind"], "alias_shadows_builtin");
        assert_eq!(arr[1]["severity"], "info");
    }

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_lint_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "orphaned_branch_config");
        assert_eq!(fields[2], "warning");
        assert_eq!(fields[3], "branch.old-feature.remote");
        assert_eq!(fields[4], "origin");
        assert!(fields[5].contains("old-feature"));
    }
}
