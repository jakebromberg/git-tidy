use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// Discover git repos as immediate subdirectories of `directory`.
///
/// Finds directories containing a `.git` **directory** (not a `.git` file, which
/// indicates a linked worktree). Returns sorted, canonicalized paths.
///
/// Skips:
/// - Non-directory entries
/// - Entries without `.git`
/// - Linked worktrees (`.git` is a file)
/// - Bare repos (`HEAD` file exists but no `.git` subdir)
/// - Entries where `.git` is a symlink (avoids double-counting shared repos)
pub fn discover_repos(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    if !directory.is_dir() {
        return Err(Error::DirectoryNotFound {
            path: directory.to_path_buf(),
        });
    }

    let directory = directory.canonicalize().map_err(Error::Io)?;
    let entries = fs::read_dir(&directory).map_err(Error::Io)?;

    let mut repos = Vec::new();

    for entry in entries {
        let entry = entry.map_err(Error::Io)?;
        let entry_path = entry.path().canonicalize().unwrap_or_else(|_| entry.path());

        if !entry_path.is_dir() {
            continue;
        }

        let git_path = entry_path.join(".git");

        // Must have .git
        if !git_path.exists() {
            continue;
        }

        // .git must be a directory (not a file = linked worktree)
        if !git_path.is_dir() {
            continue;
        }

        // Skip if .git is a symlink (avoids double-counting)
        if git_path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
            continue;
        }

        repos.push(entry_path);
    }

    repos.sort();
    Ok(repos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_repos_finds_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        let repo = base.join("my-repo");
        fs::create_dir_all(repo.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert_eq!(result, vec![repo]);
    }

    #[test]
    fn discover_repos_skips_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        let worktree = base.join("my-worktree");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            worktree.join(".git"),
            "gitdir: /some/repo/.git/worktrees/my-worktree\n",
        )
        .unwrap();

        let result = discover_repos(&base).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn discover_repos_skips_non_repo_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        let other = base.join("random-dir");
        fs::create_dir_all(&other).unwrap();

        let result = discover_repos(&base).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn discover_repos_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        let repo_a = base.join("alpha");
        fs::create_dir_all(repo_a.join(".git")).unwrap();

        let repo_b = base.join("beta");
        fs::create_dir_all(repo_b.join(".git")).unwrap();

        let worktree = base.join("gamma-wt");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join(".git"), "gitdir: /x/.git/worktrees/y\n").unwrap();

        let other = base.join("delta");
        fs::create_dir_all(&other).unwrap();

        let result = discover_repos(&base).unwrap();
        assert_eq!(result, vec![repo_a, repo_b]);
    }

    #[test]
    fn discover_repos_nonexistent_directory() {
        let result = discover_repos(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn discover_repos_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = discover_repos(dir.path()).unwrap();
        assert!(result.is_empty());
    }
}
