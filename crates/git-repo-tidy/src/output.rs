use std::io::Write;

use git_tidy_core::output as shared;

use crate::types::{JsonRepo, RepoScanResult, format_disk_size, format_last_commit_age};

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    if !result.repos.is_empty() {
        writeln!(out)?;
    }

    for repo in &result.repos {
        let label = format!("{:<10}", repo.classification.label());
        let age = format_last_commit_age(repo.last_commit_age_days);
        let size = format_disk_size(repo.disk_usage_bytes);

        let dirty_note = if repo.is_dirty {
            let noun = if repo.dirty_file_count == 1 {
                "file"
            } else {
                "files"
            };
            format!("  dirty ({} {noun})", repo.dirty_file_count)
        } else {
            String::new()
        };

        writeln!(
            out,
            "  {label} {:<25} {:<20} {:<10}{dirty_note}",
            repo.name, age, size,
        )?;
    }

    write_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Write the repo-specific summary lines.
fn write_summary(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    let dirty_note = if c.dirty > 0 {
        format!(" ({} dirty)", c.dirty)
    } else {
        String::new()
    };

    writeln!(
        out,
        "\n{} repos scanned: {} stale, {} orphaned, {} active{dirty_note}",
        result.total_scanned, c.stale, c.orphaned, c.active,
    )?;

    writeln!(
        out,
        "Total: {} (stale + orphaned: {} reclaimable)",
        format_disk_size(result.total_disk_usage_bytes),
        format_disk_size(result.reclaimable_bytes),
    )
}

/// Write JSON scan output using the flat format.
pub fn write_json(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    let all_repos: Vec<JsonRepo> = result.repos.iter().map(JsonRepo::from).collect();
    shared::write_json_pretty(out, &all_repos)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    for repo in &result.repos {
        let date = repo.last_commit_date.as_deref().unwrap_or("");
        let age = repo
            .last_commit_age_days
            .map(|d| d.to_string())
            .unwrap_or_default();
        let url = repo.remote_url.as_deref().unwrap_or("");

        writeln!(
            out,
            "{}\t{}\t{}\t{date}\t{age}\t{}\t{url}\t{}\t{}\t{}\t{}",
            repo.path.display(),
            repo.name,
            repo.classification.label(),
            repo.disk_usage_bytes,
            repo.branch_count,
            repo.has_remote,
            repo.is_dirty,
            repo.dirty_file_count,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;

    fn make_scan_result() -> RepoScanResult {
        RepoScanResult {
            repos: vec![
                RepoInfo {
                    path: PathBuf::from("/repos/old-project"),
                    name: "old-project".to_string(),
                    classification: RepoClassification::Stale,
                    last_commit_date: Some("2024-01-15T12:00:00+00:00".to_string()),
                    last_commit_age_days: Some(549),
                    disk_usage_bytes: 142 * 1024 * 1024,
                    remote_url: Some("https://github.com/user/old.git".to_string()),
                    branch_count: 3,
                    has_remote: true,
                    is_dirty: false,
                    dirty_file_count: 0,
                },
                RepoInfo {
                    path: PathBuf::from("/repos/orphan"),
                    name: "orphan".to_string(),
                    classification: RepoClassification::Orphaned,
                    last_commit_date: Some("2023-06-01T12:00:00+00:00".to_string()),
                    last_commit_age_days: Some(800),
                    disk_usage_bytes: 89 * 1024 * 1024,
                    remote_url: None,
                    branch_count: 1,
                    has_remote: false,
                    is_dirty: true,
                    dirty_file_count: 3,
                },
                RepoInfo {
                    path: PathBuf::from("/repos/main-app"),
                    name: "main-app".to_string(),
                    classification: RepoClassification::Active,
                    last_commit_date: Some("2025-02-17T12:00:00+00:00".to_string()),
                    last_commit_age_days: Some(0),
                    disk_usage_bytes: 256 * 1024 * 1024,
                    remote_url: Some("https://github.com/user/main.git".to_string()),
                    branch_count: 5,
                    has_remote: true,
                    is_dirty: false,
                    dirty_file_count: 0,
                },
            ],
            total_scanned: 3,
            counts: RepoCounts {
                stale: 1,
                orphaned: 1,
                active: 1,
                dirty: 1,
            },
            warnings: vec![],
            total_disk_usage_bytes: (142 + 89 + 256) * 1024 * 1024,
            reclaimable_bytes: (142 + 89) * 1024 * 1024,
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("stale"));
        assert!(output.contains("old-project"));
        assert!(output.contains("549 days ago"));
        assert!(output.contains("142 MB"));

        assert!(output.contains("orphaned"));
        assert!(output.contains("orphan"));
        assert!(output.contains("dirty (3 files)"));
        assert!(output.contains("89 MB"));

        assert!(output.contains("active"));
        assert!(output.contains("main-app"));
        assert!(output.contains("today"));
    }

    #[test]
    fn human_output_summary() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("3 repos scanned: 1 stale, 1 orphaned, 1 active (1 dirty)"));
        assert!(output.contains("reclaimable"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = RepoScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: RepoCounts::default(),
            warnings: vec!["could not scan /bad/path".to_string()],
            total_disk_usage_bytes: 0,
            reclaimable_bytes: 0,
        };
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not scan /bad/path"));
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
        assert_eq!(arr[0]["name"], "old-project");
        assert_eq!(arr[1]["classification"], "orphaned");
        assert!(arr[1]["is_dirty"].as_bool().unwrap());
        assert_eq!(arr[2]["classification"], "active");
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
        assert_eq!(fields.len(), 11);
        assert_eq!(fields[0], "/repos/old-project");
        assert_eq!(fields[1], "old-project");
        assert_eq!(fields[2], "stale");
        assert_eq!(fields[3], "2024-01-15T12:00:00+00:00");
        assert_eq!(fields[4], "549");
        assert_eq!(fields[6], "https://github.com/user/old.git");
        assert_eq!(fields[7], "3");
        assert_eq!(fields[8], "true");
        assert_eq!(fields[9], "false");
        assert_eq!(fields[10], "0");
    }

    #[test]
    fn human_output_no_dirty_note_in_summary() {
        let result = RepoScanResult {
            repos: vec![RepoInfo {
                path: PathBuf::from("/repos/clean"),
                name: "clean".to_string(),
                classification: RepoClassification::Active,
                last_commit_date: Some("2025-02-17T12:00:00+00:00".to_string()),
                last_commit_age_days: Some(0),
                disk_usage_bytes: 100 * 1024 * 1024,
                remote_url: Some("https://github.com/user/clean.git".to_string()),
                branch_count: 1,
                has_remote: true,
                is_dirty: false,
                dirty_file_count: 0,
            }],
            total_scanned: 1,
            counts: RepoCounts {
                active: 1,
                ..Default::default()
            },
            warnings: vec![],
            total_disk_usage_bytes: 100 * 1024 * 1024,
            reclaimable_bytes: 0,
        };
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // No "(X dirty)" when there are no dirty repos
        assert!(output.contains("1 repos scanned: 0 stale, 0 orphaned, 1 active\n"));
        assert!(!output.contains("dirty"));
    }
}
