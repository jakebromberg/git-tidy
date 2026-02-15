use crate::classification;
use crate::discovery;
use crate::error::Error;
use crate::git::GitOps;
use crate::types::{RepoGroup, ScanCounts, ScanResult};

/// Run the full scan pipeline: discover worktrees, fetch, classify, and return results.
pub fn run_scan(
    git: &dyn GitOps,
    directory: &std::path::Path,
    behind_threshold: usize,
    verbose: bool,
) -> Result<ScanResult, Error> {
    let groups = discovery::discover_worktrees(directory)?;

    let mut repos = Vec::new();
    let mut counts = ScanCounts::default();
    let mut warnings = Vec::new();
    let mut total_scanned = 0;

    for (repo_path, worktrees) in &groups {
        // Fetch to get current remote state
        if let Err(e) = git.fetch_prune(repo_path) {
            warnings.push(format!("fetch failed for {}: {e}", repo_path.display()));
        }

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

        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_path.display().to_string());

        let mut classified = Vec::new();
        for wt in worktrees {
            match classification::classify_worktree(
                git,
                &wt.path,
                repo_path,
                &default_branch,
                behind_threshold,
                verbose,
            ) {
                Ok(info) => {
                    counts.increment(&info.classification);
                    total_scanned += 1;
                    classified.push(info);
                }
                Err(e) => {
                    warnings.push(format!(
                        "error classifying {}: {e}",
                        wt.path.display()
                    ));
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
    }

    Ok(ScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}
