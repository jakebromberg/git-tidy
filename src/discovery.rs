use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// A discovered linked worktree before classification.
#[derive(Debug, Clone)]
pub struct DiscoveredWorktree {
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
    /// Absolute path to the parent (main) repo.
    pub parent_repo: PathBuf,
}

/// Parse a `.git` file and extract the gitdir path.
/// The file contains a line like: `gitdir: /path/to/.git/worktrees/name`
pub fn parse_git_file(git_file: &Path) -> Result<PathBuf, Error> {
    let content = fs::read_to_string(git_file).map_err(|_| Error::InvalidGitFile {
        path: git_file.to_path_buf(),
    })?;

    let gitdir = content
        .lines()
        .find_map(|line| line.strip_prefix("gitdir: "))
        .ok_or_else(|| Error::InvalidGitFile {
            path: git_file.to_path_buf(),
        })?;

    Ok(PathBuf::from(gitdir.trim()))
}

/// Derive the parent repo path from a gitdir pointer.
/// E.g., `/path/to/repo/.git/worktrees/name` -> `/path/to/repo`
pub fn derive_parent_repo(gitdir: &Path) -> Option<PathBuf> {
    // Walk up from gitdir looking for ".git/worktrees"
    let gitdir_str = gitdir.to_string_lossy();
    if let Some(idx) = gitdir_str.find("/.git/worktrees/") {
        Some(PathBuf::from(&gitdir_str[..idx]))
    } else {
        None
    }
}

/// Discover all linked worktrees as immediate subdirectories of `directory`.
/// Returns worktrees grouped by parent repo path.
pub fn discover_worktrees(
    directory: &Path,
) -> Result<BTreeMap<PathBuf, Vec<DiscoveredWorktree>>, Error> {
    if !directory.is_dir() {
        return Err(Error::DirectoryNotFound {
            path: directory.to_path_buf(),
        });
    }

    let mut grouped: BTreeMap<PathBuf, Vec<DiscoveredWorktree>> = BTreeMap::new();

    let directory = directory.canonicalize().map_err(Error::Io)?;
    let entries = fs::read_dir(&directory).map_err(Error::Io)?;

    for entry in entries {
        let entry = entry.map_err(Error::Io)?;
        let entry_path = entry.path().canonicalize().unwrap_or_else(|_| entry.path());

        if !entry_path.is_dir() {
            continue;
        }

        let git_path = entry_path.join(".git");

        // Skip if no .git at all
        if !git_path.exists() {
            continue;
        }

        // Skip if .git is a directory (main worktree or standalone repo)
        if git_path.is_dir() {
            continue;
        }

        // .git is a file — this is a linked worktree
        let gitdir = match parse_git_file(&git_path) {
            Ok(gd) => gd,
            Err(_) => continue, // skip malformed .git files
        };

        let parent_repo = match derive_parent_repo(&gitdir) {
            Some(p) => p.canonicalize().unwrap_or(p),
            None => continue, // skip if we can't derive the parent
        };

        let worktree = DiscoveredWorktree {
            path: entry_path,
            parent_repo: parent_repo.clone(),
        };

        grouped.entry(parent_repo).or_default().push(worktree);
    }

    Ok(grouped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let git_file = dir.path().join(".git");
        fs::write(
            &git_file,
            "gitdir: /Users/jake/Developer/MyRepo/.git/worktrees/MyRepo-feature\n",
        )
        .unwrap();

        let result = parse_git_file(&git_file).unwrap();
        assert_eq!(
            result,
            PathBuf::from("/Users/jake/Developer/MyRepo/.git/worktrees/MyRepo-feature")
        );
    }

    #[test]
    fn parse_git_file_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let git_file = dir.path().join(".git");
        fs::write(&git_file, "not a gitdir line\n").unwrap();

        assert!(parse_git_file(&git_file).is_err());
    }

    #[test]
    fn parse_git_file_missing() {
        let result = parse_git_file(Path::new("/nonexistent/.git"));
        assert!(result.is_err());
    }

    #[test]
    fn derive_parent_repo_valid() {
        let gitdir = PathBuf::from("/Users/jake/Developer/MyRepo/.git/worktrees/MyRepo-feature");
        let parent = derive_parent_repo(&gitdir).unwrap();
        assert_eq!(parent, PathBuf::from("/Users/jake/Developer/MyRepo"));
    }

    #[test]
    fn derive_parent_repo_invalid() {
        let gitdir = PathBuf::from("/some/random/path");
        assert!(derive_parent_repo(&gitdir).is_none());
    }

    #[test]
    fn discover_worktrees_basic() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // Create a main repo (has .git directory — should be skipped)
        let main_repo = base.join("MyRepo");
        fs::create_dir_all(main_repo.join(".git")).unwrap();

        // Create a linked worktree (has .git file)
        let worktree = base.join("MyRepo-feature");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            worktree.join(".git"),
            format!(
                "gitdir: {}/.git/worktrees/MyRepo-feature\n",
                main_repo.display()
            ),
        )
        .unwrap();

        // Create a non-repo directory (no .git — should be skipped)
        let other = base.join("random-dir");
        fs::create_dir_all(&other).unwrap();

        let result = discover_worktrees(&base).unwrap();

        assert_eq!(result.len(), 1);
        let (repo, worktrees) = result.iter().next().unwrap();
        assert_eq!(*repo, main_repo);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(worktrees[0].path, worktree);
    }

    #[test]
    fn discover_worktrees_multiple_repos() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // Repo A
        let repo_a = base.join("RepoA");
        fs::create_dir_all(repo_a.join(".git")).unwrap();
        let wt_a1 = base.join("RepoA-feat1");
        fs::create_dir_all(&wt_a1).unwrap();
        fs::write(
            wt_a1.join(".git"),
            format!("gitdir: {}/.git/worktrees/RepoA-feat1\n", repo_a.display()),
        )
        .unwrap();
        let wt_a2 = base.join("RepoA-feat2");
        fs::create_dir_all(&wt_a2).unwrap();
        fs::write(
            wt_a2.join(".git"),
            format!("gitdir: {}/.git/worktrees/RepoA-feat2\n", repo_a.display()),
        )
        .unwrap();

        // Repo B
        let repo_b = base.join("RepoB");
        fs::create_dir_all(repo_b.join(".git")).unwrap();
        let wt_b1 = base.join("RepoB-fix");
        fs::create_dir_all(&wt_b1).unwrap();
        fs::write(
            wt_b1.join(".git"),
            format!("gitdir: {}/.git/worktrees/RepoB-fix\n", repo_b.display()),
        )
        .unwrap();

        let result = discover_worktrees(&base).unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result[&repo_a].len(), 2);
        assert_eq!(result[&repo_b].len(), 1);
    }

    #[test]
    fn discover_worktrees_nonexistent_directory() {
        let result = discover_worktrees(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn discover_worktrees_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = discover_worktrees(dir.path()).unwrap();
        assert!(result.is_empty());
    }
}
