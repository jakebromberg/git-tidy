use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

/// Metadata for a known git-tidy sub-tool.
pub struct ToolSpec {
    /// Binary name (e.g., "git-worktree-tidy").
    pub binary: &'static str,
    /// Noun describing items scanned (e.g., "worktrees").
    pub item_noun: &'static str,
    /// Subcommand to run for scanning (e.g., "scan" or "lint").
    pub scan_command: &'static str,
    /// JSON field to count by (e.g., "classification" or "kind").
    pub count_field: &'static str,
    /// CLI aliases for dispatch (e.g., `["worktrees", "worktree"]`).
    pub aliases: &'static [&'static str],
}

/// All known git-tidy sub-tools.
pub static TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        binary: "git-worktree-tidy",
        item_noun: "worktrees",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["worktrees", "worktree"],
    },
    ToolSpec {
        binary: "git-branch-tidy",
        item_noun: "branches",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["branches", "branch"],
    },
    ToolSpec {
        binary: "git-stash-tidy",
        item_noun: "stashes",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["stashes", "stash"],
    },
    ToolSpec {
        binary: "git-remote-tidy",
        item_noun: "remotes",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["remotes", "remote"],
    },
    ToolSpec {
        binary: "git-tag-tidy",
        item_noun: "tags",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["tags", "tag"],
    },
    ToolSpec {
        binary: "git-repo-tidy",
        item_noun: "repos",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["repos", "repo"],
    },
    ToolSpec {
        binary: "git-config-tidy",
        item_noun: "config issues",
        scan_command: "lint",
        count_field: "kind",
        aliases: &["config"],
    },
    ToolSpec {
        binary: "git-lfs-tidy",
        item_noun: "LFS files",
        scan_command: "scan",
        count_field: "classification",
        aliases: &["lfs"],
    },
];

/// Result from running a single tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    /// Binary name (e.g., "git-worktree-tidy").
    pub name: String,
    /// Noun describing items (e.g., "worktrees").
    pub item_noun: String,
    /// Total items found.
    pub total: usize,
    /// Counts by classification/kind (sorted by key).
    pub counts: BTreeMap<String, usize>,
    /// Error message if the tool failed.
    pub error: Option<String>,
}

/// Consolidated result of an audit run.
#[derive(Debug, Clone, Serialize)]
pub struct AuditResult {
    /// Directory that was scanned.
    pub directory: PathBuf,
    /// Names of installed tools.
    pub tools_found: Vec<String>,
    /// Names of tools not found on PATH.
    pub tools_missing: Vec<String>,
    /// Per-tool results.
    pub results: Vec<ToolResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_specs_count() {
        assert_eq!(TOOL_SPECS.len(), 8);
    }

    #[test]
    fn tool_specs_binary_names() {
        let names: Vec<&str> = TOOL_SPECS.iter().map(|s| s.binary).collect();
        assert_eq!(
            names,
            [
                "git-worktree-tidy",
                "git-branch-tidy",
                "git-stash-tidy",
                "git-remote-tidy",
                "git-tag-tidy",
                "git-repo-tidy",
                "git-config-tidy",
                "git-lfs-tidy",
            ]
        );
    }

    #[test]
    fn tool_specs_aliases_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for spec in TOOL_SPECS {
            for alias in spec.aliases {
                assert!(
                    seen.insert(*alias),
                    "duplicate alias {alias:?} in {}",
                    spec.binary
                );
            }
        }
    }

    #[test]
    fn tool_specs_all_have_aliases() {
        for spec in TOOL_SPECS {
            assert!(!spec.aliases.is_empty(), "{} has no aliases", spec.binary);
        }
    }

    #[test]
    fn tool_specs_aliases_dont_collide_with_commands() {
        let reserved = ["audit", "completions", "help"];
        for spec in TOOL_SPECS {
            for alias in spec.aliases {
                assert!(
                    !reserved.contains(alias),
                    "alias {alias:?} in {} collides with reserved command",
                    spec.binary
                );
                assert!(
                    !alias.starts_with('-'),
                    "alias {alias:?} in {} starts with a dash",
                    spec.binary
                );
            }
        }
    }

    #[test]
    fn tool_specs_config_uses_lint_and_kind() {
        let config = TOOL_SPECS
            .iter()
            .find(|s| s.binary == "git-config-tidy")
            .unwrap();
        assert_eq!(config.scan_command, "lint");
        assert_eq!(config.count_field, "kind");
    }

    #[test]
    fn tool_specs_others_use_scan_and_classification() {
        for spec in TOOL_SPECS.iter().filter(|s| s.binary != "git-config-tidy") {
            assert_eq!(
                spec.scan_command, "scan",
                "expected scan for {}",
                spec.binary
            );
            assert_eq!(
                spec.count_field, "classification",
                "expected classification for {}",
                spec.binary
            );
        }
    }

    #[test]
    fn tool_result_serializes() {
        let result = ToolResult {
            name: "git-branch-tidy".to_string(),
            item_noun: "branches".to_string(),
            total: 5,
            counts: BTreeMap::from([("active".to_string(), 3), ("landed".to_string(), 2)]),
            error: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["total"], 5);
        assert_eq!(json["counts"]["active"], 3);
        assert!(json["error"].is_null());
    }

    #[test]
    fn audit_result_serializes() {
        let result = AuditResult {
            directory: PathBuf::from("/tmp/dev"),
            tools_found: vec!["git-branch-tidy".to_string()],
            tools_missing: vec!["git-repo-tidy".to_string()],
            results: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["directory"], "/tmp/dev");
        assert_eq!(json["tools_found"][0], "git-branch-tidy");
        assert_eq!(json["tools_missing"][0], "git-repo-tidy");
    }
}
