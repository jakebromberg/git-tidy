use std::path::{Path, PathBuf};

use git_tidy::runner::{ToolRunner, run_audit};
use git_tidy_core::progress::Progress;

/// A test runner that simulates installed tools with canned responses.
struct FakeToolRunner {
    available: Vec<(String, String)>, // (binary, json_output)
}

impl FakeToolRunner {
    fn new(tools: Vec<(&str, &str)>) -> Self {
        Self {
            available: tools
                .into_iter()
                .map(|(name, output)| (name.to_string(), output.to_string()))
                .collect(),
        }
    }
}

impl ToolRunner for FakeToolRunner {
    fn find_tool(&self, binary: &str) -> Option<PathBuf> {
        self.available
            .iter()
            .find(|(name, _)| name == binary)
            .map(|_| PathBuf::from(format!("/fake/bin/{binary}")))
    }

    fn run_tool(&self, path: &Path, _scan_cmd: &str, _directory: &Path) -> Result<String, String> {
        let binary = path.file_name().unwrap().to_str().unwrap();
        self.available
            .iter()
            .find(|(name, _)| name == binary)
            .map(|(_, output)| Ok(output.clone()))
            .unwrap_or(Err("not found".to_string()))
    }
}

#[test]
fn full_audit_multiple_tools() {
    let runner = FakeToolRunner::new(vec![
        (
            "git-worktree-tidy",
            r#"[
                {"classification":"active","name":"a"},
                {"classification":"landed","name":"b"},
                {"classification":"active","name":"c"}
            ]"#,
        ),
        (
            "git-branch-tidy",
            r#"[
                {"classification":"landed-content","name":"feature"},
                {"classification":"active","name":"main"}
            ]"#,
        ),
        (
            "git-config-tidy",
            r#"[
                {"kind":"orphaned_branch_config","severity":"warning"},
                {"kind":"alias_shadows_builtin","severity":"error"}
            ]"#,
        ),
    ]);

    let result = run_audit(&runner, Path::new("/tmp/test"), None, &Progress::disabled());

    // Check found/missing
    assert!(
        result
            .tools_found
            .contains(&"git-worktree-tidy".to_string())
    );
    assert!(result.tools_found.contains(&"git-branch-tidy".to_string()));
    assert!(result.tools_found.contains(&"git-config-tidy".to_string()));
    assert!(result.tools_missing.contains(&"git-stash-tidy".to_string()));
    assert!(result.tools_missing.contains(&"git-repo-tidy".to_string()));

    // Worktree results
    let wt = result
        .results
        .iter()
        .find(|r| r.name == "git-worktree-tidy")
        .unwrap();
    assert_eq!(wt.total, 3);
    assert_eq!(wt.counts["active"], 2);
    assert_eq!(wt.counts["landed"], 1);
    assert!(wt.error.is_none());

    // Config results (uses "kind" field)
    let cfg = result
        .results
        .iter()
        .find(|r| r.name == "git-config-tidy")
        .unwrap();
    assert_eq!(cfg.total, 2);
    assert_eq!(cfg.counts["orphaned_branch_config"], 1);
    assert_eq!(cfg.counts["alias_shadows_builtin"], 1);
}

#[test]
fn full_audit_json_roundtrip() {
    let runner = FakeToolRunner::new(vec![(
        "git-branch-tidy",
        r#"[{"classification":"active"}]"#,
    )]);

    let result = run_audit(&runner, Path::new("/tmp/test"), None, &Progress::disabled());

    // Serialize to JSON and parse back
    let mut buf = Vec::new();
    git_tidy::output::write_json(&mut buf, &result).unwrap();
    let json_str = String::from_utf8(buf).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(parsed["directory"], "/tmp/test");
    assert!(!parsed["results"].as_array().unwrap().is_empty());
}

#[test]
fn full_audit_porcelain_roundtrip() {
    let runner = FakeToolRunner::new(vec![(
        "git-branch-tidy",
        r#"[{"classification":"active"},{"classification":"landed"}]"#,
    )]);

    let result = run_audit(&runner, Path::new("/tmp"), None, &Progress::disabled());

    let mut buf = Vec::new();
    git_tidy::output::write_porcelain(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();

    // Should have exactly 1 line (one tool result)
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 1);

    let fields: Vec<&str> = lines[0].split('\t').collect();
    assert_eq!(fields[0], "git-branch-tidy");
    assert_eq!(fields[1], "branches");
    assert_eq!(fields[2], "2");
}

#[test]
fn full_audit_human_output_end_to_end() {
    let runner = FakeToolRunner::new(vec![(
        "git-worktree-tidy",
        r#"[{"classification":"active"},{"classification":"active"},{"classification":"landed"}]"#,
    )]);

    let result = run_audit(&runner, Path::new("/tmp/dev"), None, &Progress::disabled());

    let mut buf = Vec::new();
    git_tidy::output::write_human(&mut buf, &result, false).unwrap();
    let output = String::from_utf8(buf).unwrap();

    assert!(output.contains("git-tidy audit:"));
    assert!(output.contains("worktrees:"));
    assert!(output.contains("3 scanned"));
    assert!(output.contains("2 active"));
    assert!(output.contains("1 landed"));
    assert!(output.contains("not installed:"));
    assert!(output.contains("Run individual tools for details."));
}

#[test]
fn audit_filter_limits_tools() {
    let runner = FakeToolRunner::new(vec![
        ("git-worktree-tidy", "[]"),
        ("git-branch-tidy", r#"[{"classification":"active"}]"#),
        ("git-tag-tidy", "[]"),
    ]);

    let filter = vec!["branch".to_string()];
    let result = run_audit(
        &runner,
        Path::new("/tmp"),
        Some(&filter),
        &Progress::disabled(),
    );

    assert_eq!(result.tools_found, vec!["git-branch-tidy"]);
    assert!(result.tools_missing.is_empty());
    assert_eq!(result.results.len(), 1);
    assert_eq!(result.results[0].name, "git-branch-tidy");
}
