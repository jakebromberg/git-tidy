use std::path::Path;
use std::thread;

use crate::git::GitOps;
use crate::progress::Progress;

/// Fetch from all repos concurrently using `thread::scope`.
///
/// Returns a `Vec<String>` of warning messages for repos where fetch failed.
/// Single-repo and empty inputs skip thread overhead.
pub fn parallel_fetch(git: &dyn GitOps, repo_paths: &[&Path], progress: &Progress) -> Vec<String> {
    let pb = progress.bar(repo_paths.len() as u64, "Fetching");

    match repo_paths.len() {
        0 => {
            pb.finish_and_clear();
            Vec::new()
        }
        1 => {
            let mut warnings = Vec::new();
            if let Err(e) = git.fetch_prune(repo_paths[0]) {
                warnings.push(format!("fetch failed for {}: {e}", repo_paths[0].display()));
            }
            pb.inc(1);
            pb.finish_and_clear();
            warnings
        }
        _ => {
            let warnings = std::sync::Mutex::new(Vec::new());
            thread::scope(|s| {
                for &repo_path in repo_paths {
                    s.spawn(|| {
                        if let Err(e) = git.fetch_prune(repo_path) {
                            warnings
                                .lock()
                                .unwrap()
                                .push(format!("fetch failed for {}: {e}", repo_path.display()));
                        }
                        pb.inc(1);
                    });
                }
            });
            pb.finish_and_clear();
            warnings.into_inner().unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::testutil::MockGitBuilder;

    use super::*;

    #[test]
    fn parallel_fetch_collects_all_repos() {
        let git = MockGitBuilder::new().build();
        let paths = [
            PathBuf::from("/repo1"),
            PathBuf::from("/repo2"),
            PathBuf::from("/repo3"),
        ];
        let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();

        let warnings = parallel_fetch(&git, &path_refs, &Progress::disabled());

        assert!(warnings.is_empty());
        let calls = git.fetch_prune_calls();
        assert_eq!(calls.len(), 3);
        assert!(calls.contains(&PathBuf::from("/repo1")));
        assert!(calls.contains(&PathBuf::from("/repo2")));
        assert!(calls.contains(&PathBuf::from("/repo3")));
    }

    #[test]
    fn parallel_fetch_empty_repos() {
        let git = MockGitBuilder::new().build();

        let warnings = parallel_fetch(&git, &[], &Progress::disabled());

        assert!(warnings.is_empty());
        assert!(git.fetch_prune_calls().is_empty());
    }

    #[test]
    fn parallel_fetch_single_repo() {
        let git = MockGitBuilder::new().build();
        let path = PathBuf::from("/solo");

        let warnings = parallel_fetch(&git, &[path.as_path()], &Progress::disabled());

        assert!(warnings.is_empty());
        assert_eq!(git.fetch_prune_calls(), vec![PathBuf::from("/solo")]);
    }
}
