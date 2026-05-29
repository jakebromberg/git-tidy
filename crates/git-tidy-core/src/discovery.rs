use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;

/// Maximum directory depth to recurse when searching for repos.
const MAX_DEPTH: usize = 5;

/// Discover git repos under `directory`, recursing into non-repo subdirectories.
///
/// Finds directories containing a `.git` **directory** (not a `.git` file, which
/// indicates a linked worktree). Returns sorted, canonicalized paths.
///
/// Recurses into plain directories (up to `MAX_DEPTH` levels) to find repos
/// nested inside organizational folders (e.g. `~/Developer/WXYC/request-o-matic`).
/// Once a repo is found, its children are not searched.
///
/// If no repos are found at any depth, falls back to checking whether `directory`
/// itself is a repo (has a `.git` directory). This lets tools work when run
/// from inside a single repo.
///
/// Skips:
/// - Non-directory entries
/// - Hidden directories (name starts with `.`) below the top level
/// - Entries without `.git`
/// - Linked worktrees (`.git` is a file)
/// - Bare repos (`HEAD` file exists but no `.git` subdir)
/// - Entries where `.git` is a symlink (avoids double-counting shared repos)
/// - Repos whose canonical path escapes the canonical scan root (via symlinks)
pub fn discover_repos(directory: &Path) -> Result<Vec<PathBuf>, Error> {
    if !directory.is_dir() {
        return Err(Error::DirectoryNotFound {
            path: directory.to_path_buf(),
        });
    }

    let directory = directory.canonicalize().map_err(Error::Io)?;
    let mut repos = Vec::new();
    discover_repos_recursive(&directory, &directory, 0, &mut repos);

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

fn discover_repos_recursive(
    directory: &Path,
    scan_root: &Path,
    depth: usize,
    repos: &mut Vec<PathBuf>,
) {
    if depth >= MAX_DEPTH {
        return;
    }

    let entries = match fs::read_dir(directory) {
        Ok(e) => e,
        Err(_) => return, // permission denied or other error — skip silently
    };

    for entry in entries.flatten() {
        // file_type() uses d_type from readdir — no extra stat on macOS/Linux
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }

        let entry_path = entry.path();

        // Skip hidden directories (e.g. .git, .claude, .cache)
        if let Some(name) = entry_path.file_name().and_then(|n| n.to_str())
            && name.starts_with('.')
        {
            continue;
        }

        let git_path = entry_path.join(".git");

        // Single symlink_metadata call replaces exists() + is_dir() + symlink_metadata()
        let git_meta = match git_path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => {
                // No .git — recurse into this plain directory
                discover_repos_recursive(&entry_path, scan_root, depth + 1, repos);
                continue;
            }
        };
        if git_meta.is_symlink() {
            continue; // symlinked .git — avoid double-counting
        }
        if !git_meta.is_dir() {
            continue; // .git is a file — linked worktree
        }

        // Confirmed repo — canonicalize and verify it remains under the scan root.
        // Skip entries whose canonical path escapes scan_root (via symlinks) so
        // destructive consumers (e.g. git-repo-tidy) cannot delete repos the
        // user did not intend to expose.
        let canonical = match entry_path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue, // unreadable symlink target — skip
        };
        if !canonical.starts_with(scan_root) {
            continue;
        }
        repos.push(canonical);
    }
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

    #[test]
    fn discover_repos_finds_nested_repos() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // ~/Developer/org/nested-repo/.git
        let nested = base.join("org").join("nested-repo");
        fs::create_dir_all(nested.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert_eq!(result, vec![nested]);
    }

    #[test]
    fn discover_repos_finds_repos_at_multiple_depths() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // depth 1: base/shallow/.git
        let shallow = base.join("shallow");
        fs::create_dir_all(shallow.join(".git")).unwrap();

        // depth 2: base/org/deep/.git
        let deep = base.join("org").join("deep");
        fs::create_dir_all(deep.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert_eq!(result, vec![deep, shallow]);
    }

    #[test]
    fn discover_repos_skips_hidden_directories() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // base/.hidden/secret-repo/.git — should NOT be found
        let hidden = base.join(".hidden").join("secret-repo");
        fs::create_dir_all(hidden.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn discover_repos_does_not_recurse_into_repos() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // base/parent-repo/.git — a repo
        let parent = base.join("parent-repo");
        fs::create_dir_all(parent.join(".git")).unwrap();
        // base/parent-repo/sub/.git — nested repo inside a repo
        let sub = parent.join("sub");
        fs::create_dir_all(sub.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        // Should find parent-repo but NOT recurse into it to find sub
        assert_eq!(result, vec![parent]);
    }

    #[test]
    fn discover_repos_skips_repo_escaping_scan_root_via_symlink() {
        // Scenario: an external repo sits outside the scan root. Inside the
        // scan root, a subdirectory contains a symlink whose canonical target
        // is that external repo. discover_repos must not return the external
        // path — destructive callers must never see paths outside the root.
        let scan_dir = tempfile::tempdir().unwrap();
        let external_dir = tempfile::tempdir().unwrap();
        let scan_root = scan_dir.path().canonicalize().unwrap();
        let external_root = external_dir.path().canonicalize().unwrap();

        // External repo: <external>/important-repo/.git
        let external_repo = external_root.join("important-repo");
        fs::create_dir_all(external_repo.join(".git")).unwrap();

        // Inside scan root: <scan>/projects/ → real dir
        // Inside projects: <scan>/projects/myrepo → symlink to external_repo
        let projects = scan_root.join("projects");
        fs::create_dir_all(&projects).unwrap();
        std::os::unix::fs::symlink(&external_repo, projects.join("myrepo")).unwrap();

        let result = discover_repos(&scan_root).unwrap();

        for path in &result {
            assert!(
                path.starts_with(&scan_root),
                "discover_repos returned path outside scan root: {} (scan root: {})",
                path.display(),
                scan_root.display(),
            );
            assert!(
                !path.starts_with(&external_root),
                "discover_repos returned external path: {}",
                path.display(),
            );
        }
    }

    #[test]
    fn discover_repos_keeps_repo_via_inner_symlink_within_root() {
        // Symlinks that resolve to a location still inside the scan root are
        // legitimate and must remain visible.
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // <base>/real/myrepo/.git
        let real_repo = base.join("real").join("myrepo");
        fs::create_dir_all(real_repo.join(".git")).unwrap();

        // <base>/link → symlink to <base>/real (resolves within base)
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();

        let result = discover_repos(&base).unwrap();
        // The real path of the repo must be present. The symlinked copy is
        // either filtered (its canonical form matches the real path and
        // dedup is not enforced) or also present — either is acceptable, but
        // the real path is mandatory.
        assert!(
            result.iter().any(|p| p == &real_repo),
            "expected to find real repo {} in {:?}",
            real_repo.display(),
            result,
        );
        // And nothing must escape base.
        for path in &result {
            assert!(
                path.starts_with(&base),
                "path {} escapes scan root {}",
                path.display(),
                base.display(),
            );
        }
    }

    #[test]
    fn discover_repos_respects_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        // Create a repo deeper than MAX_DEPTH
        let mut deep_path = base.clone();
        for i in 0..10 {
            deep_path = deep_path.join(format!("level{i}"));
        }
        fs::create_dir_all(deep_path.join(".git")).unwrap();

        let result = discover_repos(&base).unwrap();
        assert!(result.is_empty());
    }
}
