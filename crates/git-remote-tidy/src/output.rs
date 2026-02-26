use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::types::ClassificationLabel;

use crate::types::RemoteScanResult;

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

        for remote in &group.remotes {
            let label = format!("{:<13}", remote.classification.label());
            let url = remote.url.as_deref().unwrap_or("");
            let tracking = if remote.tracking_count > 0 {
                let branch_noun = if remote.tracking_count == 1 {
                    "tracking branch"
                } else {
                    "tracking branches"
                };
                format!("({} {branch_noun})", remote.tracking_count)
            } else {
                String::new()
            };

            writeln!(out, "  {label} {:<12} {:<50} {tracking}", remote.name, url,)?;
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
