use std::io::Write;
use std::path::Path;

use crate::types::AuditResult;

/// Display a path with `~` shorthand for the user's home directory.
fn display_path(path: &Path) -> String {
    if let Some(home) = dirs_home(path) {
        return home;
    }
    path.display().to_string()
}

/// Replace the home directory prefix with `~`.
fn dirs_home(path: &Path) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let home_path = Path::new(&home);
    path.strip_prefix(home_path)
        .ok()
        .map(|rest| format!("~/{}", rest.display()))
}

/// Write human-readable audit output.
pub fn write_human(
    out: &mut dyn Write,
    result: &AuditResult,
    verbose: bool,
) -> std::io::Result<()> {
    writeln!(out, "git-tidy audit: {}\n", display_path(&result.directory))?;

    for tool_result in &result.results {
        if let Some(ref err) = tool_result.error {
            writeln!(
                out,
                "  {:<14} error: {err}",
                format!("{}:", tool_result.item_noun),
            )?;
            continue;
        }

        let counts_str = if tool_result.counts.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = tool_result
                .counts
                .iter()
                .map(|(k, v)| format!("{v} {k}"))
                .collect();
            format!(" ({})", parts.join(", "))
        };

        let noun_label = format!("{}:", tool_result.item_noun);
        writeln!(
            out,
            "  {noun_label:<14} {:>3} scanned{counts_str}",
            tool_result.total,
        )?;
    }

    if !result.tools_missing.is_empty() {
        writeln!(out)?;
        writeln!(out, "  not installed: {}", result.tools_missing.join(", "))?;
    }

    if verbose {
        writeln!(out)?;
        for name in &result.tools_found {
            writeln!(out, "  found: {name}")?;
        }
    }

    writeln!(out, "\nRun individual tools for details.")?;

    Ok(())
}

/// Write JSON audit output.
pub fn write_json(out: &mut dyn Write, result: &AuditResult) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(result).map_err(std::io::Error::other)?;
    writeln!(out, "{json}")
}

/// Write porcelain (tab-delimited) audit output.
///
/// Format: `tool_name\titem_noun\ttotal\tcounts_json\terror`
pub fn write_porcelain(out: &mut dyn Write, result: &AuditResult) -> std::io::Result<()> {
    for tool_result in &result.results {
        let counts_json =
            serde_json::to_string(&tool_result.counts).map_err(std::io::Error::other)?;
        let error = tool_result.error.as_deref().unwrap_or("");

        writeln!(
            out,
            "{}\t{}\t{}\t{counts_json}\t{error}",
            tool_result.name, tool_result.item_noun, tool_result.total,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::types::{AuditResult, ToolResult};

    fn make_audit_result() -> AuditResult {
        AuditResult {
            directory: PathBuf::from("/tmp/dev"),
            tools_found: vec![
                "git-worktree-tidy".to_string(),
                "git-branch-tidy".to_string(),
            ],
            tools_missing: vec!["git-repo-tidy".to_string()],
            results: vec![
                ToolResult {
                    name: "git-worktree-tidy".to_string(),
                    item_noun: "worktrees".to_string(),
                    total: 8,
                    counts: BTreeMap::from([
                        ("active".to_string(), 5),
                        ("landed".to_string(), 2),
                        ("merged".to_string(), 1),
                    ]),
                    error: None,
                },
                ToolResult {
                    name: "git-branch-tidy".to_string(),
                    item_noun: "branches".to_string(),
                    total: 3,
                    counts: BTreeMap::from([("active".to_string(), 2), ("merged".to_string(), 1)]),
                    error: None,
                },
            ],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_audit_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("git-tidy audit: /tmp/dev"));
        assert!(output.contains("worktrees:       8 scanned (5 active, 2 landed, 1 merged)"));
        assert!(output.contains("branches:        3 scanned (2 active, 1 merged)"));
        assert!(output.contains("not installed: git-repo-tidy"));
        assert!(output.contains("Run individual tools for details."));
    }

    #[test]
    fn human_output_verbose_shows_found() {
        let result = make_audit_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result, true).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("found: git-worktree-tidy"));
        assert!(output.contains("found: git-branch-tidy"));
    }

    #[test]
    fn human_output_not_verbose_hides_found() {
        let result = make_audit_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(!output.contains("found:"));
    }

    #[test]
    fn human_output_with_error() {
        let result = AuditResult {
            directory: PathBuf::from("/tmp"),
            tools_found: vec!["git-branch-tidy".to_string()],
            tools_missing: vec![],
            results: vec![ToolResult {
                name: "git-branch-tidy".to_string(),
                item_noun: "branches".to_string(),
                total: 0,
                counts: BTreeMap::new(),
                error: Some("process failed".to_string()),
            }],
        };
        let mut buf = Vec::new();
        write_human(&mut buf, &result, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("branches:      error: process failed"));
    }

    #[test]
    fn human_output_no_missing() {
        let mut result = make_audit_result();
        result.tools_missing.clear();
        let mut buf = Vec::new();
        write_human(&mut buf, &result, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(!output.contains("not installed"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_audit_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["directory"], "/tmp/dev");
        assert_eq!(parsed["tools_found"][0], "git-worktree-tidy");
        assert_eq!(parsed["tools_missing"][0], "git-repo-tidy");
        assert_eq!(parsed["results"][0]["total"], 8);
        assert_eq!(parsed["results"][0]["counts"]["active"], 5);
        assert!(parsed["results"][0]["error"].is_null());
    }

    #[test]
    fn porcelain_output_format() {
        let result = make_audit_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0], "git-worktree-tidy");
        assert_eq!(fields[1], "worktrees");
        assert_eq!(fields[2], "8");
        // Counts are sorted (BTreeMap): active, landed, merged
        let counts: serde_json::Value = serde_json::from_str(fields[3]).unwrap();
        assert_eq!(counts["active"], 5);
        assert_eq!(counts["landed"], 2);
        assert_eq!(counts["merged"], 1);
        assert_eq!(fields[4], ""); // no error
    }

    #[test]
    fn porcelain_output_with_error() {
        let result = AuditResult {
            directory: PathBuf::from("/tmp"),
            tools_found: vec!["git-branch-tidy".to_string()],
            tools_missing: vec![],
            results: vec![ToolResult {
                name: "git-branch-tidy".to_string(),
                item_noun: "branches".to_string(),
                total: 0,
                counts: BTreeMap::new(),
                error: Some("failed".to_string()),
            }],
        };
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let fields: Vec<&str> = output.lines().next().unwrap().split('\t').collect();
        assert_eq!(fields[4], "failed");
    }
}
