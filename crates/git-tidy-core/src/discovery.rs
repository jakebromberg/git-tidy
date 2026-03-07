use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// Discover git repos as immediate subdirectories of `directory`.
///
/// Finds directories containing a `.git` **directory** (not a `.git` file, which
/// indicates a linked worktree). Returns sorted, canonicalized paths.
///
/// If no child repos are found, falls back to checking whether `directory`
/// itself is a repo (has a `.git` directory). This lets tools work when run
/// from inside a single repo.
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

        // file_type() uses d_type from readdir — no extra stat on macOS/Linux
        if !entry.file_type().map_err(Error::Io)?.is_dir() {
            continue;
        }

        let entry_path = entry.path();
        let git_path = entry_path.join(".git");

        // Single symlink_metadata call replaces exists() + is_dir() + symlink_metadata()
        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue, // no .git
        };
        if git_meta.is_symlink() {
            continue; // symlinked .git — avoid double-counting
        }
        if !git_meta.is_dir() {
            continue; // .git is a file — linked worktree
        }

        // Deferred canonicalize — only for confirmed repos
        let entry_path = entry_path.canonicalize().unwrap_or(entry_path);
        repos.push(entry_path);
    }

    // Fallback: if no child repos found, check if directory itself is a repo
    if repos.is_empty() {
        let git_path = directory.join(".git");
        if let Ok(meta) = git_path.symlink_metadata()
            && meta.is_dir()
        {
            repos.push(directory.to_path_buf());
        }
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

    #[test]
    fn discover_repos_falls_back_to_current_dir_if_repo() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // The directory itself is a git repo (has .git directory)
        fs::create_dir_all(base.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert_eq!(result, vec![base]);
    }

    #[test]
    fn discover_repos_prefers_child_repos_over_self() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // The directory itself is a git repo
        fs::create_dir_all(base.join(".git")).unwrap();

        // And it contains a child repo
        let child = base.join("child-repo");
        fs::create_dir_all(child.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        // Should find the child, not the parent
        assert_eq!(result, vec![child]);
    }
}
