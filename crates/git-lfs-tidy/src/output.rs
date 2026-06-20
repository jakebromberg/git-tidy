use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};

use crate::types::{JsonLfsItem, LfsClassification, LfsInfo, LfsScanResult};

/// Format bytes into a human-readable string.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{bytes} B")
    }
}

fn oid_short(oid: &str) -> &str {
    if oid.len() >= 7 { &oid[..7] } else { oid }
}

impl TidyItem for LfsInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("PATH"),
        ColumnSpec::left("SIZE"),
        ColumnSpec::left("OID"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        // SIZE always visible: Some("") opts out of the auto-hide rule when
        // every item has an unknown size.
        let size_cell: Cell = match self.size_bytes {
            Some(b) => Cow::Owned(format_bytes(b)),
            None => Cow::Borrowed(""),
        };
        vec![
            Some(Cow::Borrowed(self.classification.label())),
            Some(Cow::Owned(self.path.clone())),
            Some(size_cell),
            Some(Cow::Owned(oid_short(&self.oid).to_string())),
        ]
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        // Porcelain SIZE is raw u64 bytes (not human-formatted), and OID is
        // the full hash (not shortened). These differ from the human row on
        // purpose so downstream tooling sees stable, parseable values.
        let size = self.size_bytes.map(|b| b.to_string()).unwrap_or_default();
        vec![
            Cow::Owned(self.repo_path.display().to_string()),
            Cow::Owned(self.path.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(self.oid.clone()),
            Cow::Owned(size),
        ]
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &LfsScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.items.len() == 1 {
            "item"
        } else {
            "items"
        };
        writeln!(out, "\n{} ({} {noun})", group.name, group.items.len())?;

        if !group.track_patterns.is_empty() {
            writeln!(out, "  LFS patterns: {}", group.track_patterns.join(", "))?;
        }

        shared::format_table(out, &group.items)?;

        let has_untracked = group
            .items
            .iter()
            .any(|i| i.classification == LfsClassification::Untracked);
        let has_missing = group
            .items
            .iter()
            .any(|i| i.classification == LfsClassification::Missing);

        if has_untracked {
            writeln!(
                out,
                "  hint: use `git lfs migrate` or `git-filter-repo` to track large files with LFS"
            )?;
        }
        if has_missing {
            writeln!(
                out,
                "  hint: use `git lfs fetch --all` to download missing LFS objects"
            )?;
        }
    }

    write_lfs_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the LFS-specific summary line.
fn write_lfs_summary(out: &mut dyn Write, result: &LfsScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} items scanned: {} untracked, {} missing, {} orphaned, {} healthy",
        result.total_scanned,
        c.get("untracked"),
        c.get("missing"),
        c.get("orphaned"),
        c.get("healthy"),
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &LfsScanResult) -> std::io::Result<()> {
    let all_items: Vec<JsonLfsItem> = result
        .repos
        .iter()
        .flat_map(|g| g.items.iter())
        .map(JsonLfsItem::from)
        .collect();

    shared::write_json_pretty(out, &all_items)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &LfsScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        shared::format_porcelain(out, &group.items)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;

    // --- format_bytes tests ---

    #[test]
    fn format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(999), "999 B");
    }

    #[test]
    fn format_bytes_kilobytes() {
        assert_eq!(format_bytes(1_000), "1.0 KB");
        assert_eq!(format_bytes(1_500), "1.5 KB");
        assert_eq!(format_bytes(999_999), "1000.0 KB");
    }

    #[test]
    fn format_bytes_megabytes() {
        assert_eq!(format_bytes(1_000_000), "1.0 MB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_bytes(52_400_000), "52.4 MB");
    }

    #[test]
    fn format_bytes_gigabytes() {
        assert_eq!(format_bytes(1_000_000_000), "1.0 GB");
        assert_eq!(format_bytes(2_500_000_000), "2.5 GB");
    }

    // --- write_human tests ---

    fn make_scan_result() -> LfsScanResult {
        LfsScanResult {
            repos: vec![LfsRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                items: vec![
                    LfsInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        path: "video.mp4".to_string(),
                        classification: LfsClassification::Untracked,
                        oid: "abc1234def5678".to_string(),
                        size_bytes: Some(52_400_000),
                    },
                    LfsInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        path: "missing.bin".to_string(),
                        classification: LfsClassification::Missing,
                        oid: "def5678abc1234".to_string(),
                        size_bytes: None,
                    },
                    LfsInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        path: "tracked.zip".to_string(),
                        classification: LfsClassification::Healthy,
                        oid: "789abcdef1234".to_string(),
                        size_bytes: Some(1_200_000),
                    },
                ],
                lfs_available: true,
                track_patterns: vec!["*.bin".to_string(), "*.zip".to_string()],
            }],
            total_scanned: 3,
            counts: Counts::from_pairs(&[("untracked", 1), ("missing", 1), ("healthy", 1)]),
            warnings: vec![],
            lfs_installed: true,
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("backend (3 items)"));
        assert!(output.contains("LFS patterns: *.bin, *.zip"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("PATH"));
        assert!(output.contains("SIZE"));
        assert!(output.contains("OID"));
        assert!(output.contains("untracked"));
        assert!(output.contains("missing"));
        assert!(output.contains("healthy"));
        assert!(output.contains("video.mp4"));
        assert!(output.contains("52.4 MB"));
        assert!(output.contains("hint: use `git lfs migrate`"));
        assert!(output.contains("hint: use `git lfs fetch --all`"));
        assert!(output.contains("3 items scanned: 1 untracked, 1 missing, 0 orphaned, 1 healthy"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = LfsScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: Counts::default(),
            warnings: vec!["git-lfs is not installed".to_string()],
            lfs_installed: false,
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: git-lfs is not installed"));
    }

    #[test]
    fn human_output_single_item_noun() {
        let result = LfsScanResult {
            repos: vec![LfsRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                items: vec![LfsInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    path: "file.bin".to_string(),
                    classification: LfsClassification::Healthy,
                    oid: "abc1234".to_string(),
                    size_bytes: None,
                }],
                lfs_available: true,
                track_patterns: vec![],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("healthy", 1)]),
            warnings: vec![],
            lfs_installed: true,
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test (1 item)"));
    }

    #[test]
    fn human_output_size_column_visible_when_all_sizes_none() {
        // The SIZE column must render even when every row has size_bytes = None,
        // because LfsInfo::row() returns Some(Cow::Borrowed("")) — opting out of
        // format_table's auto-hide rule for all-None columns.
        let result = LfsScanResult {
            repos: vec![LfsRepoGroup {
                repo_path: PathBuf::from("/repos/r"),
                name: "r".to_string(),
                items: vec![LfsInfo {
                    repo_path: PathBuf::from("/repos/r"),
                    path: "a.bin".to_string(),
                    classification: LfsClassification::Missing,
                    oid: "1234567abc".to_string(),
                    size_bytes: None,
                }],
                lfs_available: true,
                track_patterns: vec![],
            }],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("missing", 1)]),
            warnings: vec![],
            lfs_installed: true,
        };
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("SIZE"), "SIZE header missing: {output}");
    }

    // --- write_json tests ---

    #[test]
    fn json_output_valid() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["classification"], "untracked");
        assert_eq!(arr[0]["path"], "video.mp4");
        assert_eq!(arr[0]["size_bytes"], 52_400_000);
        assert_eq!(arr[2]["classification"], "healthy");
    }

    // --- write_porcelain tests ---

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "video.mp4");
        assert_eq!(fields[2], "untracked");
        assert_eq!(fields[3], "abc1234def5678");
        assert_eq!(fields[4], "52400000");
    }

    // --- TidyItem data-shape tests ---

    fn lfs_info(
        path: &str,
        classification: LfsClassification,
        oid: &str,
        size_bytes: Option<u64>,
    ) -> LfsInfo {
        LfsInfo {
            repo_path: PathBuf::from("/repos/backend"),
            path: path.to_string(),
            classification,
            oid: oid.to_string(),
            size_bytes,
        }
    }

    #[test]
    fn tidyitem_row_shape_lfs() {
        let row = lfs_info(
            "video.mp4",
            LfsClassification::Untracked,
            "abc1234def5678",
            Some(52_400_000),
        )
        .row();
        assert_eq!(row.len(), 4);
        assert!(row.iter().all(|c| c.is_some()));
        assert_eq!(row[0].as_deref(), Some("untracked"));
        assert_eq!(row[1].as_deref(), Some("video.mp4"));
        assert_eq!(row[2].as_deref(), Some("52.4 MB"));
        assert_eq!(row[3].as_deref(), Some("abc1234"));
    }

    #[test]
    fn tidyitem_row_size_none_renders_empty_some_not_none() {
        // SIZE must stay visible even when size_bytes is None — implemented by
        // returning Some(Cow::Borrowed("")) rather than None.
        let row = lfs_info("a.bin", LfsClassification::Missing, "abc1234", None).row();
        assert_eq!(row[2].as_deref(), Some(""));
    }

    #[test]
    fn tidyitem_porcelain_lfs_raw_bytes_and_full_oid() {
        let fields = lfs_info(
            "video.mp4",
            LfsClassification::Untracked,
            "abc1234def5678",
            Some(52_400_000),
        )
        .porcelain_fields();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0].as_ref(), "/repos/backend");
        assert_eq!(fields[1].as_ref(), "video.mp4");
        assert_eq!(fields[2].as_ref(), "untracked");
        // Full OID, not shortened.
        assert_eq!(fields[3].as_ref(), "abc1234def5678");
        // Raw u64 bytes, not "52.4 MB".
        assert_eq!(fields[4].as_ref(), "52400000");
    }

    #[test]
    fn tidyitem_porcelain_size_none_is_empty_field() {
        let fields =
            lfs_info("a.bin", LfsClassification::Missing, "abc1234", None).porcelain_fields();
        assert_eq!(fields[4].as_ref(), "");
    }
}
