use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use git_tidy_core::progress::Progress;

use crate::types::{AuditResult, TOOL_SPECS, ToolResult, ToolSpec};

/// Abstraction for finding and running git-tidy sub-tools.
pub trait ToolRunner {
    /// Check if a binary is available, returning its path.
    fn find_tool(&self, binary: &str) -> Option<PathBuf>;
    /// Run a tool's scan/lint command and return its stdout as a string.
    fn run_tool(
        &self,
        path: &Path,
        scan_cmd: &str,
        directory: &Path,
        verbose: bool,
    ) -> Result<String, String>;
}

/// Real implementation that uses `which` and `std::process::Command`.
pub struct RealToolRunner;

impl ToolRunner for RealToolRunner {
    fn find_tool(&self, binary: &str) -> Option<PathBuf> {
        which::which(binary).ok()
    }

    fn run_tool(
        &self,
        path: &Path,
        scan_cmd: &str,
        directory: &Path,
        verbose: bool,
    ) -> Result<String, String> {
        let mut cmd = Command::new(path);
        if verbose {
            cmd.arg("--verbose");
        }
        let output = cmd
            .arg(scan_cmd)
            .arg("--json")
            .arg(directory)
            .output()
            .map_err(|e| format!("failed to execute {}: {e}", path.display()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "{} exited with {}: {}",
                path.display(),
                output.status,
                stderr.trim()
            ));
        }

        String::from_utf8(output.stdout)
            .map_err(|e| format!("invalid UTF-8 from {}: {e}", path.display()))
    }
}

/// Parse a JSON array from a tool's output and count items by a given field.
pub fn parse_tool_output(
    json_str: &str,
    count_field: &str,
) -> Result<(usize, BTreeMap<String, usize>), String> {
    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("invalid JSON: {e}"))?;

    let array = value
        .as_array()
        .ok_or_else(|| "expected JSON array".to_string())?;

    let mut counts = BTreeMap::new();
    for item in array {
        if let Some(field_value) = item.get(count_field).and_then(|v| v.as_str()) {
            *counts.entry(field_value.to_string()).or_insert(0) += 1;
        }
    }

    Ok((array.len(), counts))
}

/// Check whether a tool name matches a filter entry.
///
/// Accepts both full binary names ("git-branch-tidy") and short names ("branch-tidy",
/// "branch", "worktree").
pub fn matches_filter(binary: &str, filter_entry: &str) -> bool {
    if binary == filter_entry {
        return true;
    }
    // Strip "git-" prefix from binary for short-name matching
    let short = binary.strip_prefix("git-").unwrap_or(binary);
    if short == filter_entry {
        return true;
    }
    // Also allow just the tool word (e.g., "branch" matches "git-branch-tidy")
    let tool_word = short.strip_suffix("-tidy").unwrap_or(short);
    tool_word == filter_entry
}

/// Run an audit across all known git-tidy tools.
pub fn run_audit(
    runner: &dyn ToolRunner,
    directory: &Path,
    tool_filter: Option<&[String]>,
    verbose: bool,
    progress: &Progress,
) -> AuditResult {
    let mut tools_found = Vec::new();
    let mut tools_missing = Vec::new();
    let mut results = Vec::new();

    // Collect matching specs for progress bar length.
    let specs: Vec<_> = TOOL_SPECS
        .iter()
        .filter(|spec| {
            tool_filter
                .map(|f| f.iter().any(|entry| matches_filter(spec.binary, entry)))
                .unwrap_or(true)
        })
        .collect();

    let pb = progress.bar(specs.len() as u64, "Auditing");

    for (idx, spec) in specs.iter().enumerate() {
        pb.set_message(format!(
            "[{}/{}] Scanning {}...",
            idx + 1,
            specs.len(),
            spec.item_noun
        ));

        let Some(path) = runner.find_tool(spec.binary) else {
            tools_missing.push(spec.binary.to_string());
            pb.inc(1);
            continue;
        };

        tools_found.push(spec.binary.to_string());

        let result = match runner.run_tool(&path, spec.scan_command, directory, verbose) {
            Ok(output) => match parse_tool_output(&output, spec.count_field) {
                Ok((total, counts)) => make_tool_result(spec, total, counts, None),
                Err(e) => make_tool_result(spec, 0, BTreeMap::new(), Some(e)),
            },
            Err(e) => make_tool_result(spec, 0, BTreeMap::new(), Some(e)),
        };

        results.push(result);
        pb.inc(1);
    }
    pb.finish_and_clear();

    AuditResult {
        directory: directory.to_path_buf(),
        tools_found,
        tools_missing,
        results,
    }
}

fn make_tool_result(
    spec: &ToolSpec,
    total: usize,
    counts: BTreeMap<String, usize>,
    error: Option<String>,
) -> ToolResult {
    ToolResult {
        name: spec.binary.to_string(),
        item_noun: spec.item_noun.to_string(),
        total,
        counts,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_tool_output tests --

    #[test]
    fn parse_classification_field() {
        let json = r#"[
            {"name": "a", "classification": "landed"},
            {"name": "b", "classification": "active"},
            {"name": "c", "classification": "landed"},
            {"name": "d", "classification": "landed-content"}
        ]"#;

        let (total, counts) = parse_tool_output(json, "classification").unwrap();
        assert_eq!(total, 4);
        assert_eq!(counts["landed"], 2);
        assert_eq!(counts["active"], 1);
        assert_eq!(counts["landed-content"], 1);
    }

    #[test]
    fn parse_kind_field() {
        let json = r#"[
            {"kind": "orphaned_branch_config", "severity": "warning"},
            {"kind": "alias_shadows_builtin", "severity": "error"},
            {"kind": "orphaned_branch_config", "severity": "warning"}
        ]"#;

        let (total, counts) = parse_tool_output(json, "kind").unwrap();
        assert_eq!(total, 3);
        assert_eq!(counts["orphaned_branch_config"], 2);
        assert_eq!(counts["alias_shadows_builtin"], 1);
    }

    #[test]
    fn parse_empty_array() {
        let (total, counts) = parse_tool_output("[]", "classification").unwrap();
        assert_eq!(total, 0);
        assert!(counts.is_empty());
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_tool_output("not json", "classification");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn parse_non_array_json() {
        let result = parse_tool_output(r#"{"key": "value"}"#, "classification");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected JSON array"));
    }

    #[test]
    fn parse_items_missing_field() {
        let json = r#"[
            {"name": "a", "classification": "active"},
            {"name": "b"}
        ]"#;
        let (total, counts) = parse_tool_output(json, "classification").unwrap();
        assert_eq!(total, 2);
        assert_eq!(counts["active"], 1);
        assert_eq!(counts.len(), 1);
    }

    // -- matches_filter tests --

    #[test]
    fn filter_full_name() {
        assert!(matches_filter("git-branch-tidy", "git-branch-tidy"));
    }

    #[test]
    fn filter_short_name() {
        assert!(matches_filter("git-branch-tidy", "branch-tidy"));
    }

    #[test]
    fn filter_tool_word() {
        assert!(matches_filter("git-branch-tidy", "branch"));
        assert!(matches_filter("git-worktree-tidy", "worktree"));
        assert!(matches_filter("git-config-tidy", "config"));
    }

    #[test]
    fn filter_no_match() {
        assert!(!matches_filter("git-branch-tidy", "tag"));
        assert!(!matches_filter("git-branch-tidy", "git-tag-tidy"));
    }

    // -- MockToolRunner and run_audit tests --

    struct MockToolRunner {
        tools: Vec<(String, Result<String, String>)>,
    }

    impl MockToolRunner {
        fn new(tools: Vec<(&str, Result<&str, &str>)>) -> Self {
            Self {
                tools: tools
                    .into_iter()
                    .map(|(name, result)| {
                        (
                            name.to_string(),
                            result.map(|s| s.to_string()).map_err(|s| s.to_string()),
                        )
                    })
                    .collect(),
            }
        }
    }

    impl ToolRunner for MockToolRunner {
        fn find_tool(&self, binary: &str) -> Option<PathBuf> {
            self.tools
                .iter()
                .find(|(name, _)| name == binary)
                .map(|_| PathBuf::from(format!("/usr/local/bin/{binary}")))
        }

        fn run_tool(
            &self,
            path: &Path,
            _scan_cmd: &str,
            _directory: &Path,
            _verbose: bool,
        ) -> Result<String, String> {
            let binary = path.file_name().unwrap().to_str().unwrap();
            self.tools
                .iter()
                .find(|(name, _)| name == binary)
                .map(|(_, result)| result.clone())
                .unwrap_or_else(|| Err("tool not found".to_string()))
        }
    }

    #[test]
    fn audit_all_tools_found() {
        let runner = MockToolRunner::new(vec![
            (
                "git-worktree-tidy",
                Ok(r#"[{"classification":"active"},{"classification":"landed"}]"#),
            ),
            (
                "git-branch-tidy",
                Ok(r#"[{"classification":"landed-content"}]"#),
            ),
            ("git-stash-tidy", Ok("[]")),
            ("git-remote-tidy", Ok("[]")),
            ("git-tag-tidy", Ok("[]")),
            ("git-repo-tidy", Ok("[]")),
            (
                "git-config-tidy",
                Ok(r#"[{"kind":"alias_shadows_builtin"}]"#),
            ),
            ("git-lfs-tidy", Ok("[]")),
        ]);

        let result = run_audit(&runner, Path::new("/dev"), None, false, &Progress::disabled());
        assert_eq!(result.tools_found.len(), 8);
        assert!(result.tools_missing.is_empty());
        assert_eq!(result.results.len(), 8);

        let wt = &result.results[0];
        assert_eq!(wt.name, "git-worktree-tidy");
        assert_eq!(wt.total, 2);
        assert_eq!(wt.counts["active"], 1);
        assert_eq!(wt.counts["landed"], 1);
        assert!(wt.error.is_none());

        let config = &result.results[6];
        assert_eq!(config.name, "git-config-tidy");
        assert_eq!(config.total, 1);
        assert_eq!(config.counts["alias_shadows_builtin"], 1);
    }

    #[test]
    fn audit_some_tools_missing() {
        let runner = MockToolRunner::new(vec![(
            "git-branch-tidy",
            Ok(r#"[{"classification":"active"}]"#),
        )]);

        let result = run_audit(&runner, Path::new("/dev"), None, false, &Progress::disabled());
        assert_eq!(result.tools_found, vec!["git-branch-tidy"]);
        assert!(
            result
                .tools_missing
                .contains(&"git-worktree-tidy".to_string())
        );
        assert!(result.tools_missing.contains(&"git-repo-tidy".to_string()));
        assert_eq!(result.results.len(), 1);
    }

    #[test]
    fn audit_tool_returns_error() {
        let runner = MockToolRunner::new(vec![("git-branch-tidy", Err("process failed"))]);

        let result = run_audit(&runner, Path::new("/dev"), None, false, &Progress::disabled());
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].error.as_deref(), Some("process failed"));
        assert_eq!(result.results[0].total, 0);
    }

    #[test]
    fn audit_tool_returns_invalid_json() {
        let runner = MockToolRunner::new(vec![("git-branch-tidy", Ok("not json"))]);

        let result = run_audit(&runner, Path::new("/dev"), None, false, &Progress::disabled());
        assert_eq!(result.results.len(), 1);
        assert!(result.results[0].error.is_some());
        assert!(
            result.results[0]
                .error
                .as_ref()
                .unwrap()
                .contains("invalid JSON")
        );
    }

    #[test]
    fn audit_with_filter() {
        let runner = MockToolRunner::new(vec![
            ("git-worktree-tidy", Ok("[]")),
            ("git-branch-tidy", Ok(r#"[{"classification":"active"}]"#)),
            ("git-tag-tidy", Ok("[]")),
        ]);

        let filter = vec!["branch".to_string(), "tag".to_string()];
        let result = run_audit(
            &runner,
            Path::new("/dev"),
            Some(&filter),
            false,
            &Progress::disabled(),
        );

        // Only branch and tag should be checked
        assert_eq!(result.tools_found, vec!["git-branch-tidy", "git-tag-tidy"]);
        assert!(result.tools_missing.is_empty());
        assert_eq!(result.results.len(), 2);
    }

    #[test]
    fn audit_no_tools_found() {
        let runner = MockToolRunner::new(vec![]);

        let result = run_audit(&runner, Path::new("/dev"), None, false, &Progress::disabled());
        assert!(result.tools_found.is_empty());
        assert_eq!(result.tools_missing.len(), 8);
        assert!(result.results.is_empty());
    }
}
