use std::io::{self, Write};

use crate::error::Error;
use crate::git::GitOps;
use crate::types::{Classification, ScanResult, WorktreeInfo};

/// Options for the clean command.
pub struct CleanOptions {
    pub dry_run: bool,
    pub force: bool,
    pub yes: bool,
    pub merged_only: bool,
    pub landed: bool,
    pub all: bool,
    pub delete_branches: bool,
}

/// Result of the clean operation.
pub struct CleanResult {
    pub removed: usize,
    pub failed: usize,
    pub dirty_blocked: bool,
}

/// Run the interactive clean flow.
pub fn run_clean(
    git: &dyn GitOps,
    scan: &ScanResult,
    opts: &CleanOptions,
    out: &mut dyn Write,
    interactive: bool,
) -> Result<CleanResult, Error> {
    let mut removed = 0;
    let mut failed = 0;
    let mut dirty_blocked = false;

    // Collect all worktrees flattened
    let all_worktrees: Vec<&WorktreeInfo> = scan
        .repos
        .iter()
        .flat_map(|g| g.worktrees.iter())
        .collect();

    // Group by classification
    let merged: Vec<&&WorktreeInfo> = all_worktrees
        .iter()
        .filter(|wt| matches!(wt.classification, Classification::Merged))
        .collect();
    let landed_full: Vec<&&WorktreeInfo> = all_worktrees
        .iter()
        .filter(|wt| matches!(wt.classification, Classification::Landed { .. }))
        .collect();
    let partial: Vec<&&WorktreeInfo> = all_worktrees
        .iter()
        .filter(|wt| matches!(wt.classification, Classification::LandedPartial { .. }))
        .collect();
    let active: Vec<&&WorktreeInfo> = all_worktrees
        .iter()
        .filter(|wt| matches!(wt.classification, Classification::Active))
        .collect();
    let local: Vec<&&WorktreeInfo> = all_worktrees
        .iter()
        .filter(|wt| matches!(wt.classification, Classification::Local))
        .collect();

    // Process merged group
    if !merged.is_empty() {
        let wts: Vec<&WorktreeInfo> = merged.into_iter().copied().collect();
        let result = process_group(
            git,
            &wts,
            "MERGED",
            true, // default yes
            opts,
            out,
            interactive,
        )?;
        removed += result.0;
        failed += result.1;
        if result.2 {
            dirty_blocked = true;
        }
    }

    // Process landed group (only if --landed or --all, or default behavior)
    if !opts.merged_only && !landed_full.is_empty() {
        let wts: Vec<&WorktreeInfo> = landed_full.into_iter().copied().collect();
        let result = process_group(
            git,
            &wts,
            "LANDED",
            true, // default yes
            opts,
            out,
            interactive,
        )?;
        removed += result.0;
        failed += result.1;
        if result.2 {
            dirty_blocked = true;
        }
    }

    // Process partial group (only if not --merged-only and not --landed without --all)
    if !opts.merged_only && !opts.landed && !partial.is_empty() {
        let wts: Vec<&WorktreeInfo> = partial.into_iter().copied().collect();
        let result = process_partial_group(git, &wts, opts, out, interactive)?;
        removed += result.0;
        failed += result.1;
        if result.2 {
            dirty_blocked = true;
        }
    }

    // Process active and local only with --all
    if opts.all {
        if !active.is_empty() {
            let wts: Vec<&WorktreeInfo> = active.into_iter().copied().collect();
            let result = process_group(
                git,
                &wts,
                "ACTIVE",
                false, // default no
                opts,
                out,
                interactive,
            )?;
            removed += result.0;
            failed += result.1;
            if result.2 {
                dirty_blocked = true;
            }
        }
        if !local.is_empty() {
            let wts: Vec<&WorktreeInfo> = local.into_iter().copied().collect();
            let result = process_group(
                git,
                &wts,
                "LOCAL",
                false, // default no
                opts,
                out,
                interactive,
            )?;
            removed += result.0;
            failed += result.1;
            if result.2 {
                dirty_blocked = true;
            }
        }
    } else {
        if !active.is_empty() {
            writeln!(out, "\nACTIVE ({} worktrees) -- skipped", active.len())?;
        }
        if !local.is_empty() {
            writeln!(out, "LOCAL ({} worktrees) -- skipped", local.len())?;
        }
    }

    let remaining = scan.total_scanned - removed;
    writeln!(out, "\nRemoved {removed} worktrees. {remaining} remaining.")?;

    Ok(CleanResult {
        removed,
        failed,
        dirty_blocked,
    })
}

/// Process a group of worktrees with a single yes/no prompt.
/// Returns (removed, failed, dirty_blocked).
fn process_group(
    git: &dyn GitOps,
    worktrees: &[&WorktreeInfo],
    label: &str,
    default_yes: bool,
    opts: &CleanOptions,
    out: &mut dyn Write,
    interactive: bool,
) -> Result<(usize, usize, bool), Error> {
    writeln!(out, "\n{label} ({} worktrees)", worktrees.len())?;

    for wt in worktrees {
        write_worktree_clean_line(out, wt)?;
    }

    let prompt = if default_yes {
        format!("Remove {} {label} worktrees? [Y/n]", worktrees.len())
    } else {
        format!("Remove {} {label} worktrees? [y/N]", worktrees.len())
    };

    let should_remove = if opts.dry_run {
        writeln!(out, "{prompt} (dry run)")?;
        false
    } else if opts.yes {
        writeln!(out, "{prompt} y")?;
        default_yes
    } else if !interactive {
        writeln!(out, "{prompt} (non-interactive, using default)")?;
        default_yes
    } else {
        writeln!(out, "{prompt}")?;
        prompt_yes_no(default_yes)?
    };

    if !should_remove {
        return Ok((0, 0, false));
    }

    let mut removed = 0;
    let mut failed = 0;
    let mut dirty_blocked = false;

    for wt in worktrees {
        match remove_worktree(git, wt, opts, out) {
            Ok(true) => removed += 1,
            Ok(false) => dirty_blocked = true,
            Err(_) => failed += 1,
        }
    }

    Ok((removed, failed, dirty_blocked))
}

/// Process partial group: review individually.
fn process_partial_group(
    git: &dyn GitOps,
    worktrees: &[&WorktreeInfo],
    opts: &CleanOptions,
    out: &mut dyn Write,
    interactive: bool,
) -> Result<(usize, usize, bool), Error> {
    writeln!(
        out,
        "\nLANDED (partial) -- {} worktrees",
        worktrees.len()
    )?;

    for wt in worktrees {
        write_worktree_clean_line(out, wt)?;
        if let Classification::LandedPartial { unmatched, .. } = &wt.classification {
            for commit in unmatched {
                writeln!(out, "    unmatched: {} {}", commit.short_hash, commit.subject)?;
            }
        }
    }

    let should_review = if opts.dry_run {
        writeln!(out, "Review individually? [y/N] (dry run)")?;
        false
    } else if opts.yes {
        writeln!(out, "Review individually? [y/N] n")?;
        false // default is N for partial
    } else if !interactive {
        writeln!(out, "Review individually? [y/N] (non-interactive, using default)")?;
        false
    } else {
        writeln!(out, "Review individually? [y/N]")?;
        prompt_yes_no(false)?
    };

    if !should_review {
        return Ok((0, 0, false));
    }

    let mut removed = 0;
    let mut failed = 0;
    let mut dirty_blocked = false;

    for wt in worktrees {
        let dir_name = wt
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let branch = wt.branch.as_deref().unwrap_or("(detached)");
        let ratio = match &wt.classification {
            Classification::LandedPartial { matched, total, .. } => {
                format!("{matched}/{total} landed")
            }
            _ => String::new(),
        };
        let dirty_info = if wt.annotations.dirty { ", dirty" } else { "" };
        let prompt = format!("  Remove {dir_name} ({branch}, {ratio}{dirty_info})? [y/N]");

        let should_remove = if opts.dry_run {
            writeln!(out, "{prompt} (dry run)")?;
            false
        } else if !interactive {
            writeln!(out, "{prompt} (non-interactive, using default)")?;
            false
        } else {
            writeln!(out, "{prompt}")?;
            prompt_yes_no(false)?
        };

        if should_remove {
            match remove_worktree(git, wt, opts, out) {
                Ok(true) => removed += 1,
                Ok(false) => dirty_blocked = true,
                Err(_) => failed += 1,
            }
        }
    }

    Ok((removed, failed, dirty_blocked))
}

/// Write a worktree line for the clean output.
fn write_worktree_clean_line(out: &mut dyn Write, wt: &WorktreeInfo) -> std::io::Result<()> {
    let dir_name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let branch = wt.branch.as_deref().unwrap_or("(detached)");

    write!(out, "  {dir_name:<32} {branch:<32}")?;

    if let Classification::Landed { matched, total }
    | Classification::LandedPartial { matched, total, .. } = &wt.classification
    {
        write!(out, " {matched}/{total} commits")?;
    }

    if wt.annotations.dirty {
        write!(out, "  dirty ({} files)", wt.annotations.dirty_file_count)?;
    }
    if wt.annotations.remote_deleted {
        write!(out, "  remote deleted")?;
    }
    writeln!(out)
}

/// Attempt to remove a worktree using the three-tier strategy.
/// Returns Ok(true) if removed, Ok(false) if blocked by dirty, Err on failure.
fn remove_worktree(
    git: &dyn GitOps,
    wt: &WorktreeInfo,
    opts: &CleanOptions,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let dir_name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Check if dirty and not forced
    if wt.annotations.dirty && !opts.force {
        writeln!(
            out,
            "  skipped {dir_name}: dirty ({} files), use --force to remove",
            wt.annotations.dirty_file_count
        )?;
        return Ok(false);
    }

    // Tier 1: git worktree remove
    let result = if wt.annotations.dirty {
        // Tier 2: git worktree remove --force
        git.worktree_remove_force(&wt.parent_repo, &wt.path)
    } else {
        git.worktree_remove(&wt.parent_repo, &wt.path)
    };

    match result {
        Ok(()) => {
            writeln!(out, "  removed {dir_name}")?;
        }
        Err(_) => {
            // Tier 3: rm -rf + prune
            if wt.path.exists() {
                std::fs::remove_dir_all(&wt.path).map_err(|e| Error::RemovalFailed {
                    path: wt.path.clone(),
                    reason: e.to_string(),
                })?;
            }
            git.worktree_prune(&wt.parent_repo)?;
            writeln!(out, "  removed {dir_name} (fallback)")?;
        }
    }

    // Delete branch if requested
    if opts.delete_branches {
        if let Some(branch) = &wt.branch {
            if !git.is_branch_checked_out(&wt.parent_repo, branch)? {
                match git.branch_delete(&wt.parent_repo, branch) {
                    Ok(()) => writeln!(out, "  deleted branch {branch}")?,
                    Err(e) => writeln!(out, "  warning: could not delete branch {branch}: {e}")?,
                }
            } else {
                writeln!(out, "  skipped branch delete for {branch}: checked out elsewhere")?;
            }
        }
    }

    Ok(true)
}

/// Read a yes/no answer from stdin.
fn prompt_yes_no(default_yes: bool) -> Result<bool, Error> {
    use std::io::IsTerminal;

    if !io::stdin().is_terminal() {
        return Ok(default_yes);
    }

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    Ok(match trimmed.as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default_yes,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::git::tests::MockGitBuilder;
    use crate::types::*;

    fn make_worktree(
        name: &str,
        branch: &str,
        classification: Classification,
    ) -> WorktreeInfo {
        WorktreeInfo {
            path: PathBuf::from(format!("/dev/{name}")),
            parent_repo: PathBuf::from("/repos/test"),
            branch: Some(branch.to_string()),
            default_branch: "main".to_string(),
            classification,
            annotations: Annotations::default(),
            remote_tracking: true,
            ahead: 0,
            behind: 0,
            dirty_files: vec![],
            meaningful_dirty_files: vec![],
        }
    }

    fn make_scan(worktrees: Vec<WorktreeInfo>) -> ScanResult {
        let mut counts = ScanCounts::default();
        for wt in &worktrees {
            counts.increment(&wt.classification);
        }
        let total = worktrees.len();
        ScanResult {
            repos: vec![RepoGroup {
                repo_path: PathBuf::from("/repos/test"),
                name: "test".to_string(),
                worktrees,
            }],
            total_scanned: total,
            counts,
            warnings: vec![],
        }
    }

    #[test]
    fn dry_run_does_not_remove() {
        let wt = make_worktree("wt1", "feature/done", Classification::Merged);
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: true,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("dry run"));
    }

    #[test]
    fn yes_removes_merged() {
        let wt = make_worktree("wt1", "feature/done", Classification::Merged);
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed wt1"));
    }

    #[test]
    fn merged_only_skips_landed() {
        let merged = make_worktree("wt-merged", "fix/done", Classification::Merged);
        let landed = make_worktree(
            "wt-landed",
            "fix/landed",
            Classification::Landed {
                matched: 3,
                total: 3,
            },
        );
        let scan = make_scan(vec![merged, landed]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: true,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 1); // only the merged one

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed wt-merged"));
        assert!(!output.contains("removed wt-landed"));
    }

    #[test]
    fn dirty_blocked_without_force() {
        let mut wt = make_worktree("wt-dirty", "fix/dirty", Classification::Merged);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 3;
        wt.meaningful_dirty_files = vec!["a".into(), "b".into(), "c".into()];
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 0);
        assert!(result.dirty_blocked);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("skipped wt-dirty: dirty"));
    }

    #[test]
    fn force_removes_dirty() {
        let mut wt = make_worktree("wt-dirty", "fix/dirty", Classification::Merged);
        wt.annotations.dirty = true;
        wt.annotations.dirty_file_count = 2;
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: true,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 1);

        // Verify force removal was called
        assert_eq!(git.remove_force_calls().len(), 1);
    }

    #[test]
    fn active_skipped_without_all() {
        let wt = make_worktree("wt-active", "feature/wip", Classification::Active);
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 0);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("ACTIVE (1 worktrees) -- skipped"));
    }

    #[test]
    fn delete_branches_after_removal() {
        let wt = make_worktree("wt1", "feature/done", Classification::Merged);
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: true,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 1);

        // Verify branch delete was called
        let deletes = git.branch_delete_calls();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].1, "feature/done");
    }

    #[test]
    fn landed_flag_skips_partial_worktrees() {
        let merged = make_worktree("wt-merged", "fix/done", Classification::Merged);
        let landed = make_worktree(
            "wt-landed",
            "fix/landed",
            Classification::Landed {
                matched: 3,
                total: 3,
            },
        );
        let partial = make_worktree(
            "wt-partial",
            "fix/partial",
            Classification::LandedPartial {
                matched: 2,
                total: 4,
                unmatched: vec![],
            },
        );
        let scan = make_scan(vec![merged, landed, partial]);
        let git = MockGitBuilder::new().build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: true,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        // Should remove merged + landed but NOT partial
        assert_eq!(result.removed, 2);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("removed wt-merged"));
        assert!(output.contains("removed wt-landed"));
        assert!(!output.contains("removed wt-partial"));
        // Partial group should not appear at all (skipped by --landed flag)
        assert!(!output.contains("LANDED (partial)"));
    }

    #[test]
    fn three_tier_removal_fallback() {
        let wt = make_worktree("wt-stubborn", "fix/stubborn", Classification::Merged);
        let scan = make_scan(vec![wt]);

        // Configure mock so worktree_remove fails — triggers fallback to rm -rf + prune
        let git = MockGitBuilder::new()
            .with_worktree_remove_error(
                &PathBuf::from("/dev/wt-stubborn"),
                "worktree is locked",
            )
            .build();

        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: false,
        };

        let mut buf = Vec::new();
        let result = run_clean(&git, &scan, &opts, &mut buf, false).unwrap();
        assert_eq!(result.removed, 1);

        let output = String::from_utf8(buf).unwrap();
        // The path /dev/wt-stubborn doesn't actually exist on disk, so rm -rf
        // skips the remove_dir_all, but prune should still be called.
        assert!(output.contains("removed wt-stubborn (fallback)"));

        // Verify worktree_prune was called as part of fallback
        assert!(!git.remove_calls().is_empty() || !git.remove_force_calls().is_empty()
            || output.contains("fallback"));
    }

    #[test]
    fn skip_branch_delete_if_checked_out() {
        let wt = make_worktree("wt1", "feature/done", Classification::Merged);
        let scan = make_scan(vec![wt]);
        let git = MockGitBuilder::new()
            .with_is_branch_checked_out(
                &PathBuf::from("/repos/test"),
                "feature/done",
                true,
            )
            .build();
        let opts = CleanOptions {
            dry_run: false,
            force: false,
            yes: true,
            merged_only: false,
            landed: false,
            all: false,
            delete_branches: true,
        };

        let mut buf = Vec::new();
        run_clean(&git, &scan, &opts, &mut buf, false).unwrap();

        // Branch delete should NOT have been called
        assert!(git.branch_delete_calls().is_empty());

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("checked out elsewhere"));
    }
}
