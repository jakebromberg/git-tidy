//! Shared output helpers used by all git-tidy binary crates.

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::types::{Classification, ScanCounts};

/// Write the summary line: "N {item_noun} scanned: X landed, Y content, ..."
pub fn write_summary_line(
    out: &mut dyn Write,
    total: usize,
    counts: &ScanCounts,
    item_noun: &str,
) -> std::io::Result<()> {
    writeln!(
        out,
        "\n{total} {item_noun} scanned: {} landed, {} content, {} partial, {} active, {} local",
        counts.landed, counts.landed_content, counts.partial, counts.active, counts.local,
    )
}

/// Write warnings with the "warning: " prefix.
pub fn write_warnings(out: &mut dyn Write, warnings: &[String]) -> std::io::Result<()> {
    for warning in warnings {
        writeln!(out, "warning: {warning}")?;
    }
    Ok(())
}

/// Format ahead/behind as "+N/-M". Returns empty string when both are 0.
pub fn format_ahead_behind(ahead: usize, behind: usize) -> String {
    if ahead > 0 || behind > 0 {
        format!("+{}/-{}", ahead, behind)
    } else {
        String::new()
    }
}

/// Format a comma-separated annotation list from string slices.
/// Returns empty string when the list is empty.
pub fn format_annotations(annotations: &[&str]) -> String {
    annotations.join(", ")
}

/// Extract a display name from a repo path (last path component, or full path as fallback).
pub fn repo_display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Trait for scan results that can be flattened into JSON items.
pub trait FlatJsonItems {
    /// The JSON-serializable item type.
    type JsonItem: Serialize;

    /// Flatten repo groups into a vec of JSON items.
    fn to_json_items(&self) -> Vec<Self::JsonItem>;
}

/// Flatten a scan result into JSON items and write as pretty-printed JSON.
pub fn write_json_flat<T: FlatJsonItems>(out: &mut dyn Write, result: &T) -> std::io::Result<()> {
    let items = result.to_json_items();
    write_json_pretty(out, &items)
}

/// Serialize a value as pretty-printed JSON and write to output.
pub fn write_json_pretty(out: &mut dyn Write, value: &impl Serialize) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    writeln!(out, "{json}")
}

/// Format the landed ratio for display. Returns empty string for non-landed classifications.
pub fn format_landed_ratio(classification: &Classification) -> String {
    match classification {
        Classification::LandedByContent { matched, total } => format!("{matched}/{total}"),
        Classification::LandedPartial { matched, total, .. } => format!("{matched}/{total}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_line_format() {
        let counts = ScanCounts {
            landed: 3,
            landed_content: 1,
            partial: 0,
            active: 2,
            local: 1,
        };
        let mut buf = Vec::new();
        write_summary_line(&mut buf, 7, &counts, "branches").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(
            output,
            "\n7 branches scanned: 3 landed, 1 content, 0 partial, 2 active, 1 local\n"
        );
    }

    #[test]
    fn summary_line_worktrees() {
        let counts = ScanCounts {
            landed: 1,
            ..Default::default()
        };
        let mut buf = Vec::new();
        write_summary_line(&mut buf, 1, &counts, "worktrees").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("1 worktrees scanned"));
    }

    #[test]
    fn warnings_output() {
        let mut buf = Vec::new();
        write_warnings(&mut buf, &["fetch failed for /repo".to_string()]).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "warning: fetch failed for /repo\n");
    }

    #[test]
    fn warnings_empty() {
        let mut buf = Vec::new();
        write_warnings(&mut buf, &[]).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn ahead_behind_nonzero() {
        assert_eq!(format_ahead_behind(3, 5), "+3/-5");
    }

    #[test]
    fn ahead_behind_zero() {
        assert_eq!(format_ahead_behind(0, 0), "");
    }

    #[test]
    fn annotations_basic() {
        assert_eq!(
            format_annotations(&["diverged", "remote deleted"]),
            "diverged, remote deleted"
        );
    }

    #[test]
    fn annotations_empty() {
        assert_eq!(format_annotations(&[]), "");
    }

    #[test]
    fn landed_ratio_by_content() {
        assert_eq!(
            format_landed_ratio(&Classification::LandedByContent {
                matched: 3,
                total: 3
            }),
            "3/3"
        );
    }

    #[test]
    fn landed_ratio_partial() {
        assert_eq!(
            format_landed_ratio(&Classification::LandedPartial {
                matched: 2,
                total: 5,
                unmatched: vec![],
            }),
            "2/5"
        );
    }

    #[test]
    fn landed_ratio_other() {
        assert_eq!(format_landed_ratio(&Classification::Active), "");
    }

    #[test]
    fn repo_display_name_normal() {
        use std::path::PathBuf;
        assert_eq!(
            repo_display_name(&PathBuf::from("/repos/my-project")),
            "my-project"
        );
    }

    #[test]
    fn write_json_pretty_basic() {
        let data = vec!["hello", "world"];
        let mut buf = Vec::new();
        write_json_pretty(&mut buf, &data).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed, serde_json::json!(["hello", "world"]));
    }

    #[test]
    fn repo_display_name_root_path() {
        use std::path::PathBuf;
        let path = PathBuf::from("/");
        // Root path has no file_name, should fall back to display
        assert_eq!(repo_display_name(&path), "/");
    }
}
