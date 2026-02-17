use std::io::Write;

use git_tidy_core::output as shared;

use crate::types::{JsonStash, StashScanResult};

/// Write human-readable scan output.
pub fn write_human(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    shared::write_warnings(out, &result.warnings)?;

    for group in &result.repos {
        writeln!(out, "\n{} ({} stashes)", group.name, group.stashes.len())?;

        for stash in &group.stashes {
            let label = format!("{:<10}", stash.classification.label());
            let age = match stash.age_days {
                Some(0) => "today".to_string(),
                Some(1) => "1 day ago".to_string(),
                Some(d) => format!("{d} days ago"),
                None => String::new(),
            };

            writeln!(
                out,
                "  {label} {:<14} {:<50} {age}",
                stash.stash_ref, stash.message,
            )?;
        }
    }

    write_stash_summary(out, result)?;

    Ok(())
}

/// Write the stash-specific summary line.
fn write_stash_summary(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    let c = &result.counts;
    writeln!(
        out,
        "\n{} stashes scanned: {} committed, {} orphaned, {} aged, {} active",
        result.total_scanned, c.committed, c.orphaned, c.aged, c.active,
    )
}

/// Write JSON scan output using the flat spec format.
pub fn write_json(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    let all_stashes: Vec<JsonStash> = result
        .repos
        .iter()
        .flat_map(|g| g.stashes.iter())
        .map(JsonStash::from)
        .collect();

    shared::write_json_pretty(out, &all_stashes)
}

/// Write porcelain (machine-readable, tab-delimited) scan output.
pub fn write_porcelain(out: &mut dyn Write, result: &StashScanResult) -> std::io::Result<()> {
    for group in &result.repos {
        for stash in &group.stashes {
            let repo = stash.repo_path.display();
            let branch = stash.branch.as_deref().unwrap_or("");
            let age = stash.age_days.map(|d| d.to_string()).unwrap_or_default();

            writeln!(
                out,
                "{repo}\t{}\t{}\t{branch}\t{age}\t{}",
                stash.stash_ref,
                stash.classification.label(),
                stash.message,
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

    fn make_scan_result() -> StashScanResult {
        StashScanResult {
            repos: vec![StashRepoGroup {
                repo_path: PathBuf::from("/repos/my-repo"),
                name: "my-repo".to_string(),
                stashes: vec![
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{0}".to_string(),
                        classification: StashClassification::Committed,
                        branch: Some("feature-x".to_string()),
                        age_days: Some(23),
                        message: "WIP on feature-x: abc1234 Add login".to_string(),
                    },
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{1}".to_string(),
                        classification: StashClassification::Orphaned,
                        branch: Some("deleted-branch".to_string()),
                        age_days: Some(45),
                        message: "WIP on deleted-branch: def5678 Fix UI".to_string(),
                    },
                    StashInfo {
                        repo_path: PathBuf::from("/repos/my-repo"),
                        stash_ref: "stash@{2}".to_string(),
                        classification: StashClassification::Active,
                        branch: Some("main".to_string()),
                        age_days: Some(2),
                        message: "WIP on main: ghi9012 Temp changes".to_string(),
                    },
                ],
            }],
            total_scanned: 3,
            counts: StashCounts {
                committed: 1,
                orphaned: 1,
                aged: 0,
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

        assert!(output.contains("my-repo (3 stashes)"));
        assert!(output.contains("committed"));
        assert!(output.contains("orphaned"));
        assert!(output.contains("active"));
        assert!(output.contains("stash@{0}"));
        assert!(output.contains("23 days ago"));
        assert!(output.contains("45 days ago"));
        assert!(output.contains("2 days ago"));
        assert!(output.contains("3 stashes scanned: 1 committed, 1 orphaned, 0 aged, 1 active"));
    }

    #[test]
    fn human_output_with_warnings() {
        let result = StashScanResult {
            repos: vec![],
            total_scanned: 0,
            counts: StashCounts::default(),
            warnings: vec!["could not list stashes for /repo/Foo".to_string()],
        };

        let mut buf = Vec::new();
        write_human(&mut buf, &result).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("warning: could not list stashes for /repo/Foo"));
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
        assert_eq!(arr[0]["classification"], "committed");
        assert_eq!(arr[0]["branch"], "feature-x");
        assert_eq!(arr[0]["age_days"], 23);
        assert_eq!(arr[1]["classification"], "orphaned");
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
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "/repos/my-repo");
        assert_eq!(fields[1], "stash@{0}");
        assert_eq!(fields[2], "committed");
        assert_eq!(fields[3], "feature-x");
        assert_eq!(fields[4], "23");
        assert_eq!(fields[5], "WIP on feature-x: abc1234 Add login");
    }
}
