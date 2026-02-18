use std::path::Path;

use git_tidy_core::classification;
use git_tidy_core::error::Error;
use git_tidy_core::git::GitOps;
use git_tidy_core::output::repo_display_name;
use git_tidy_core::progress::Progress;
use git_tidy_core::types::{ClassificationLabel, RepoGroup, ScanCounts, ScanResult};

use crate::discovery;

/// Scan all worktrees under `directory` and classify them.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &Path,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
    progress: &Progress,
) -> Result<ScanResult, Error> {
    let groups = discovery::discover_worktrees(directory)?;

    let repo_paths: Vec<&std::path::Path> = groups.keys().map(|p| p.as_path()).collect();
    let mut warnings = git_tidy_core::fetch::parallel_fetch(git, &repo_paths, progress);

    let mut repos = Vec::new();
    let mut counts = ScanCounts::default();
    let mut total_scanned = 0;

    let pb = progress.bar(groups.len() as u64, "Scanning worktrees");
    for (repo_path, worktrees) in &groups {
        // Detect default branch
        let default_branch = match classification::detect_default_branch(git, repo_path) {
            Ok(b) => b,
            Err(_) => {
                warnings.push(format!(
                    "could not determine default branch for {} -- skipping",
                    repo_path.display()
                ));
                continue;
            }
        };

        let repo_name = repo_display_name(repo_path);

        let mut classified = Vec::new();
        for wt in worktrees {
            match classification::classify_worktree(
                git,
                &wt.path,
                repo_path,
                &default_branch,
                behind_threshold,
                verbose,
                noise_patterns,
            ) {
                Ok(info) => {
                    counts.increment(&info.classification);
                    total_scanned += 1;
                    classified.push(info);
                }
                Err(e) => {
                    warnings.push(format!("error classifying {}: {e}", wt.path.display()));
                }
            }
        }

        // Sort by classification priority
        classified.sort_by_key(|wt| wt.classification.priority());

        if !classified.is_empty() {
            repos.push(RepoGroup {
                repo_path: repo_path.clone(),
                name: repo_name,
                worktrees: classified,
            });
        }
        pb.inc(1);
    }
    pb.finish_and_clear();

    Ok(ScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}
