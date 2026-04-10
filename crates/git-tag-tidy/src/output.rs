use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::ClassificationLabel;

use crate::types::{TagInfo, TagScanResult};

const HEADER_STATUS: &str = "STATUS";
const HEADER_NAME: &str = "NAME";
const HEADER_COMMIT: &str = "COMMIT";
const HEADER_KIND: &str = "KIND";
const HEADER_DATE: &str = "DATE";

struct ColumnWidths {
    status: usize,
    name: usize,
    commit: usize,
    kind: usize,
    date: usize,
    has_date: bool,
}

fn commit_short(commit: &str) -> &str {
    if commit.len() >= 7 {
        &commit[..7]
    } else {
        commit
    }
}

fn compute_column_widths(tags: &[TagInfo]) -> ColumnWidths {
    let mut max_status = HEADER_STATUS.len();
    let mut max_name = HEADER_NAME.len();
    let mut max_commit = HEADER_COMMIT.len();
    let mut max_kind = HEADER_KIND.len();
    let mut max_date = HEADER_DATE.len();
    let mut has_date = false;

    for t in tags {
        max_status = max_status.max(t.classification.label().len());
        max_name = max_name.max(t.name.len());
        max_commit = max_commit.max(commit_short(&t.commit).len());
        let kind = if t.is_annotated {
            "annotated"
        } else {
            "lightweight"
        };
        max_kind = max_kind.max(kind.len());
        if let Some(ref d) = t.tagger_date {
            has_date = true;
            max_date = max_date.max(d.len());
        }
    }

    ColumnWidths {
        status: max_status,
        name: max_name,
        commit: max_commit,
        kind: max_kind,
        date: max_date,
        has_date,
    }
}

fn write_header(out: &mut dyn Write, widths: &ColumnWidths) -> std::io::Result<()> {
    let sw = widths.status;
    let nw = widths.name;
    let cw = widths.commit;
    let kw = widths.kind;
    let mut line = format!(
        "  {HEADER_STATUS:<sw$} {HEADER_NAME:<nw$} {HEADER_COMMIT:<cw$} {HEADER_KIND:<kw$}"
    );
    if widths.has_date {
        line.push_str(&format!(" {HEADER_DATE}"));
    }
    let trimmed = line.trim_end();
    writeln!(out, "{trimmed}")
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.tags.len() == 1 { "tag" } else { "tags" };
        writeln!(out, "\n{} ({} {noun})", group.name, group.tags.len())?;

        let widths = compute_column_widths(&group.tags);
        write_header(out, &widths)?;

        for tag in &group.tags {
            let label = tag.classification.label();
            let cs = commit_short(&tag.commit);
            let kind = if tag.is_annotated {
                "annotated"
            } else {
                "lightweight"
            };
            let date = tag.tagger_date.as_deref().unwrap_or("");

            let sw = widths.status;
            let nw = widths.name;
            let cw = widths.commit;
            let kw = widths.kind;
            let mut line = format!("  {label:<sw$} {:<nw$} {cs:<cw$} {kind:<kw$}", tag.name);
            if widths.has_date {
                let dw = widths.date;
                line.push_str(&format!(" {date:<dw$}"));
            }
            let trimmed = line.trim_end();
            writeln!(out, "{trimmed}")?;
        }
    }

    write_tag_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the tag-specific summary line.
fn write_tag_summary(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} tags scanned: {} stale, {} local_only, {} remote_only, {} synced",
        result.total_scanned, c.stale, c.local_only, c.remote_only, c.synced,
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &TagScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for tag in &group.tags {
            let repo = tag.repo_path.display();
            let remotes = tag.remote_names.join(",");

            writeln!(
                out,
                "{repo}\t{}\t{}\t{}\t{}\t{}\t{}\t{remotes}",
                tag.name,
                tag.classification.label(),
                tag.commit,
                tag.is_annotated,
                tag.tagger_date.as_deref().unwrap_or(""),
                tag.is_release_tag,
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

    fn make_scan_result() -> TagScanResult {
        TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                tags: vec![
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "old-experiment".to_string(),
                        classification: TagClassification::Stale,
                        commit: "abc1234def5678".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "feature-v2-wip".to_string(),
                        classification: TagClassification::LocalOnly,
                        commit: "def5678abc1234".to_string(),
                        is_annotated: false,
                        tagger_date: None,
                        is_release_tag: false,
                        remote_names: vec![],
                    },
                    TagInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "v1.0.0".to_string(),
                        classification: TagClassification::Synced,
                        commit: "789abcdef1234".to_string(),
                        is_annotated: true,
                        tagger_date: Some("2024-06-15T10:00:00+00:00".to_string()),
                        is_release_tag: true,
                        remote_names: vec!["origin".to_string()],
                    },
                ],
            }],
            total_scanned: 3,
            counts: TagCounts {
                stale: 1,
                local_only: 1,
                remote_only: 0,
                synced: 1,
            },
            warnings: vec![],
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("backend (3 tags)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("NAME"));
        assert!(output.contains("COMMIT"));
        assert!(output.contains("KIND"));
        assert!(output.contains("stale"));
        assert!(output.contains("local_only"));
        assert!(output.contains("synced"));
        assert!(output.contains("old-experiment"));
        assert!(output.contains("v1.0.0"));
        assert!(output.contains("annotated"));
        assert!(output.contains("lightweight"));
        assert!(output.contains("3 tags scanned: 1 stale, 1 local_only, 0 remote_only, 1 synced"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = TagScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: TagCounts::default(),
            warnings: vec!["could not list tags for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list tags for /repo/Foo"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["classification"], "stale");
        assert_eq!(arr[0]["name"], "old-experiment");
        assert!(!arr[0]["is_annotated"].as_bool().unwrap());
        assert_eq!(arr[2]["classification"], "synced");
        assert_eq!(arr[2]["name"], "v1.0.0");
        assert!(arr[2]["is_annotated"].as_bool().unwrap());
        assert!(arr[2]["is_release_tag"].as_bool().unwrap());
    }

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "old-experiment");
        assert_eq!(fields[2], "stale");
        assert_eq!(fields[3], "abc1234def5678");
        assert_eq!(fields[4], "false");
        assert_eq!(fields[5], ""); // no date
        assert_eq!(fields[6], "false");
        assert_eq!(fields[7], ""); // no remotes
    }

    #[test]
    fn human_output_single_tag_noun() {
        let result = TagScanResult {
            repos: vec![TagRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                tags: vec![TagInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    name: "v1.0".to_string(),
                    classification: TagClassification::Synced,
                    commit: "abc1234".to_string(),
                    is_annotated: false,
                    tagger_date: None,
                    is_release_tag: true,
                    remote_names: vec![],
                }],
            }],
            total_scanned: 1,
            counts: TagCounts {
                synced: 1,
                ..Default::default()
            },
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test (1 tag)"));
    }
}
