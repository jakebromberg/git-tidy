use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::ClassificationLabel;

use crate::types::{RemoteInfo, RemoteScanResult};

const HEADER_STATUS: &str = "STATUS";
const HEADER_NAME: &str = "NAME";
const HEADER_URL: &str = "URL";
const HEADER_TRACKING: &str = "TRACKING";

struct ColumnWidths {
    status: usize,
    name: usize,
    url: usize,
    tracking: usize,
    has_tracking: bool,
}

fn format_tracking(remote: &RemoteInfo) -> String {
    if remote.tracking_count > 0 {
        let branch_noun = if remote.tracking_count == 1 {
            "tracking branch"
        } else {
            "tracking branches"
        };
        format!("({} {branch_noun})", remote.tracking_count)
    } else {
        String::new()
    }
}

fn compute_column_widths(remotes: &[RemoteInfo]) -> ColumnWidths {
    let mut max_status = HEADER_STATUS.len();
    let mut max_name = HEADER_NAME.len();
    let mut max_url = HEADER_URL.len();
    let mut max_tracking = HEADER_TRACKING.len();
    let mut has_tracking = false;

    for r in remotes {
        max_status = max_status.max(r.classification.label().len());
        max_name = max_name.max(r.name.len());
        max_url = max_url.max(r.url.as_deref().unwrap_or("").len());
        let tracking = format_tracking(r);
        if !tracking.is_empty() {
            has_tracking = true;
            max_tracking = max_tracking.max(tracking.len());
        }
    }

    ColumnWidths {
        status: max_status,
        name: max_name,
        url: max_url,
        tracking: max_tracking,
        has_tracking,
    }
}

fn write_header(out: &mut dyn Write, widths: &ColumnWidths) -> std::io::Result<()> {
    let sw = widths.status;
    let nw = widths.name;
    let uw = widths.url;
    let mut line = format!("  {HEADER_STATUS:<sw$} {HEADER_NAME:<nw$} {HEADER_URL:<uw$}");
    if widths.has_tracking {
        line.push_str(&format!(" {HEADER_TRACKING}"));
    }
    let trimmed = line.trim_end();
    writeln!(out, "{trimmed}")
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        let noun = if group.remotes.len() == 1 {
            "remote"
        } else {
            "remotes"
        };
        writeln!(out, "\n{} ({} {noun})", group.name, group.remotes.len())?;

        let widths = compute_column_widths(&group.remotes);
        write_header(out, &widths)?;

        for remote in &group.remotes {
            let label = remote.classification.label();
            let url = remote.url.as_deref().unwrap_or("");
            let tracking = format_tracking(remote);

            let sw = widths.status;
            let nw = widths.name;
            let uw = widths.url;
            let mut line = format!("  {label:<sw$} {:<nw$} {url:<uw$}", remote.name);
            if widths.has_tracking {
                let tw = widths.tracking;
                line.push_str(&format!(" {tracking:<tw$}"));
            }
            let trimmed = line.trim_end();
            writeln!(out, "{trimmed}")?;
        }
    }

    write_remote_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the remote-specific summary line.
fn write_remote_summary(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} remotes scanned: {} unreachable, {} orphaned, {} active",
        result.total_scanned, c.unreachable, c.orphaned, c.active,
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    shared::write_json_flat(out, result)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &RemoteScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for remote in &group.remotes {
            let repo = remote.repo_path.display();
            let url = remote.url.as_deref().unwrap_or("");

            writeln!(
                out,
                "{repo}\t{}\t{}\t{url}\t{}\t{}",
                remote.name,
                remote.classification.label(),
                remote.tracking_count,
                remote.is_origin,
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

    fn make_scan_result() -> RemoteScanResult {
        RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/backend"),
                name: "backend".to_string(),
                remotes: vec![
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "origin".to_string(),
                        classification: RemoteClassification::Unreachable,
                        url: Some("https://github.com/old-org/backend.git".to_string()),
                        tracking_count: 12,
                        is_origin: true,
                    },
                    RemoteInfo {
                        repo_path: PathBuf::from("/repos/backend"),
                        name: "upstream".to_string(),
                        classification: RemoteClassification::Active,
                        url: Some("https://github.com/new-org/backend.git".to_string()),
                        tracking_count: 5,
                        is_origin: false,
                    },
                ],
            }],
            total_scanned: 2,
            counts: RemoteCounts {
                unreachable: 1,
                orphaned: 0,
                active: 1,
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

        assert!(output.contains("backend (2 remotes)"));
        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("NAME"));
        assert!(output.contains("URL"));
        assert!(output.contains("TRACKING"));
        assert!(output.contains("unreachable"));
        assert!(output.contains("active"));
        assert!(output.contains("origin"));
        assert!(output.contains("upstream"));
        assert!(output.contains("12 tracking branches"));
        assert!(output.contains("5 tracking branches"));
        assert!(output.contains("2 remotes scanned: 1 unreachable, 0 orphaned, 1 active"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = RemoteScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: RemoteCounts::default(),
            warnings: vec!["could not list remotes for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list remotes for /repo/Foo"));
    }

    #[test]
    fn json_output_valid() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["classification"], "unreachable");
        assert_eq!(arr[0]["name"], "origin");
        assert_eq!(arr[0]["tracking_count"], 12);
        assert!(arr[0]["is_origin"].as_bool().unwrap());
        assert_eq!(arr[1]["classification"], "active");
        assert_eq!(arr[1]["name"], "upstream");
    }

    #[test]
    fn json_output_orphaned_null_url() {
        let result = RemoteScanResult {
            repos: vec![RemoteRepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                remotes: vec![RemoteInfo {
                    repo_path: PathBuf::from("/repos/test"),
                    name: "stale".to_string(),
                    classification: RemoteClassification::Orphaned,
                    url: None,
                    tracking_count: 3,
                    is_origin: false,
                }],
            }],
            total_scanned: 1,
            counts: RemoteCounts {
                orphaned: 1,
                ..Default::default()
            },
            warnings: vec![],
        };

        let mut buf = Vec::new();
        write_json(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let arr = parsed.as_array().unwrap();
        assert!(arr[0]["url"].is_null());
        assert_eq!(arr[0]["classification"], "orphaned");
    }

    #[test]
    fn porcelain_output_tab_delimited() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_porcelain(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        let fields: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "/repos/backend");
        assert_eq!(fields[1], "origin");
        assert_eq!(fields[2], "unreachable");
        assert_eq!(fields[3], "https://github.com/old-org/backend.git");
        assert_eq!(fields[4], "12");
        assert_eq!(fields[5], "true");
    }
}
