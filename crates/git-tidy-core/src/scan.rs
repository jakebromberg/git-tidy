use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::progress::Progress;

/// Run a per-repo classification function in parallel across a set of repo paths.
///
/// Handles the parallel dispatch, progress bar, and warning collection.
/// The caller provides a closure that classifies a single repo, returning
/// `(Option<G>, Vec<String>)` where `G` is the tool-specific repo group
/// and the Vec contains any warnings for that repo.
///
/// Returns `(groups, warnings)` — the non-None groups and all accumulated warnings.
pub fn parallel_classify<G: Send>(
    repo_paths: &[PathBuf],
    classify_fn: impl Fn(&Path) -> (Option<G>, Vec<String>) + Sync + Send,
    label: &str,
    progress: &Progress,
) -> (Vec<G>, Vec<String>) {
    let pb = progress.bar(repo_paths.len() as u64, label);
    let per_repo: Vec<_> = repo_paths
        .par_iter()
        .map(|repo_path| {
            let result = classify_fn(repo_path);
            pb.inc(1);
            result
        })
        .collect();
    pb.finish_and_clear();

    let mut groups = Vec::new();
    let mut warnings = Vec::new();
    for (group, local_warnings) in per_repo {
        warnings.extend(local_warnings);
        if let Some(g) = group {
            groups.push(g);
        }
    }
    (groups, warnings)
}
