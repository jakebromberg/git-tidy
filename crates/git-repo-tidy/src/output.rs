use std::borrow::Cow;
use std::io::Write;

use git_tidy_core::output as shared;
use git_tidy_core::output::{Cell, ColumnSpec, TidyItem};

use crate::types::{JsonRepo, RepoInfo, RepoScanResult, format_disk_size, format_last_commit_age};

impl TidyItem for RepoInfo {
    const COLUMNS: &'static [ColumnSpec] = &[
        ColumnSpec::left("STATUS"),
        ColumnSpec::left("NAME"),
        ColumnSpec::left("AGE"),
        ColumnSpec::left("SIZE"),
    ];

    fn row(&self) -> Vec<Option<Cell>> {
        vec![
            Some(Cow::Borrowed(self.classification.label())),
            Some(Cow::Owned(self.name.clone())),
            Some(Cow::Owned(format_last_commit_age(
                self.last_commit_age_days,
            ))),
            Some(Cow::Owned(format_disk_size(self.disk_usage_bytes))),
        ]
    }

    fn annotations(&self) -> Vec<Cow<'static, str>> {
        if self.is_dirty {
            let noun = if self.dirty_file_count == 1 {
                "file"
            } else {
                "files"
            };
            vec![Cow::Owned(format!(
                "dirty ({} {noun})",
                self.dirty_file_count
            ))]
        } else {
            Vec::new()
        }
    }

    fn porcelain_fields(&self) -> Vec<Cow<'static, str>> {
        let date = self.last_commit_date.clone().unwrap_or_default();
        let age = self
            .last_commit_age_days
            .map(|d| d.to_string())
            .unwrap_or_default();
        let url = self.remote_url.clone().unwrap_or_default();
        vec![
            Cow::Owned(self.path.display().to_string()),
            Cow::Owned(self.name.clone()),
            Cow::Borrowed(self.classification.label()),
            Cow::Owned(date),
            Cow::Owned(age),
            Cow::Owned(self.disk_usage_bytes.to_string()),
            Cow::Owned(url),
            Cow::Owned(self.branch_count.to_string()),
            Cow::Owned(self.has_remote.to_string()),
            Cow::Owned(self.is_dirty.to_string()),
            Cow::Owned(self.dirty_file_count.to_string()),
        ]
    }
}

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    if !result.repos.is_empty() {
        writeln!(out)?;
        shared::format_table(out, &result.repos)?;
    }

    write_summary(out, result)?;
    shared::write_explain_hint(out)?;

    Ok(())
}

/// Ordered `(display, count key)` pairs for the repo summary breakdown. The
/// cross-cutting `dirty` count is appended separately (see `dirty_note`).
const REPO_SUMMARY: &[(&str, &str)] = &[
    ("stale", "stale"),
    ("orphaned", "orphaned"),
    ("active", "active"),
];

/// Write the repo-specific summary lines.
fn write_summary(out: &mut dyn Write, result: &RepoScanResult) -> std::io::Result<()> {
    let dirty_note = if result.dirty > 0 {
        format!(" ({} dirty)", result.dirty)
    } else {
        String::new()
    };

    writeln!(
        out,
        "\n{} repos scanned: {}{dirty_note}",
        result.total_scanned,
        shared::format_summary_buckets(&result.counts, REPO_SUMMARY),
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
    shared::format_porcelain(out, &result.repos)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::types::*;
    use git_tidy_core::counts::Counts;

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
            counts: Counts::from_pairs(&[("stale", 1), ("orphaned", 1), ("active", 1)]),
            dirty: 1,
            warnings: vec![],
            total_disk_usage_bytes: (142 + 89 + 256) * 1024 * 1024,
            reclaimable_bytes: (142 + 89) * 1024 * 1024,
        }
    }

    fn clean_repo(name: &str, classification: RepoClassification) -> RepoInfo {
        RepoInfo {
            path: PathBuf::from(format!("/repos/{name}")),
            name: name.to_string(),
            classification,
            last_commit_date: Some("2025-02-17T12:00:00+00:00".to_string()),
            last_commit_age_days: Some(0),
            disk_usage_bytes: 1024 * 1024,
            remote_url: Some("https://github.com/user/x.git".to_string()),
            branch_count: 1,
            has_remote: true,
            is_dirty: false,
            dirty_file_count: 0,
        }
    }

    fn dirty_repo(name: &str, dirty_file_count: usize) -> RepoInfo {
        RepoInfo {
            path: PathBuf::from(format!("/repos/{name}")),
            name: name.to_string(),
            classification: RepoClassification::Active,
            last_commit_date: Some("2025-02-17T12:00:00+00:00".to_string()),
            last_commit_age_days: Some(0),
            disk_usage_bytes: 1024 * 1024,
            remote_url: Some("https://github.com/user/x.git".to_string()),
            branch_count: 1,
            has_remote: true,
            is_dirty: true,
            dirty_file_count,
        }
    }

    #[test]
    fn human_output_basic() {
        let result = make_scan_result();
        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // Header row
        assert!(output.contains("STATUS"));
        assert!(output.contains("NAME"));
        assert!(output.contains("AGE"));
        assert!(output.contains("SIZE"));
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
            counts: Counts::default(),
            dirty: 0,
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
            repos: vec![clean_repo("clean", RepoClassification::Active)],
            total_scanned: 1,
            counts: Counts::from_pairs(&[("active", 1)]),
            dirty: 0,
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

    #[test]
    fn dirty_annotation_singular() {
        let repo = dirty_repo("one", 1);
        let anns: Vec<String> = repo
            .annotations()
            .into_iter()
            .map(|a| a.into_owned())
            .collect();
        assert_eq!(anns, vec!["dirty (1 file)"]);
    }

    #[test]
    fn dirty_annotation_plural() {
        let repo = dirty_repo("two", 2);
        let anns: Vec<String> = repo
            .annotations()
            .into_iter()
            .map(|a| a.into_owned())
            .collect();
        assert_eq!(anns, vec!["dirty (2 files)"]);
    }

    #[test]
    fn clean_repo_has_no_annotation() {
        let repo = clean_repo("clean", RepoClassification::Active);
        assert!(repo.annotations().is_empty());
    }
}
