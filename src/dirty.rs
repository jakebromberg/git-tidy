use std::path::Path;

use crate::error::Error;
use crate::git::GitOps;

/// Files that are always considered noise regardless of gitignore status.
const NOISE_PATTERNS: &[&str] = &[
    ".DS_Store",
    "*.pyc",
    "__pycache__",
    "uv.lock",
    "package-lock.json",
    "Podfile.lock",
    "yarn.lock",
];

/// Result of dirty detection on a worktree.
#[derive(Debug, Clone)]
pub struct DirtyResult {
    /// All dirty file paths from `git status --porcelain`.
    pub all_files: Vec<String>,
    /// Only the meaningful dirty files (not noise).
    pub meaningful_files: Vec<String>,
}

/// Check a worktree for dirty (uncommitted) files, filtering out noise.
pub fn check_dirty(git: &dyn GitOps, worktree_path: &Path) -> Result<DirtyResult, Error> {
    let lines = git.status_porcelain(worktree_path)?;

    let mut all_files = Vec::new();
    let mut meaningful_files = Vec::new();

    for line in &lines {
        if line.len() < 4 {
            continue;
        }
        let status_code = &line[..2];
        let file_path = line[3..].trim();

        if file_path.is_empty() {
            continue;
        }

        all_files.push(file_path.to_string());

        // Untracked files that match noise patterns are not meaningful
        if status_code == "??" && is_noise(file_path) {
            continue;
        }

        // All other dirty files (modified tracked, staged, untracked non-noise) are meaningful
        meaningful_files.push(file_path.to_string());
    }

    Ok(DirtyResult {
        all_files,
        meaningful_files,
    })
}

/// Check if a file path matches any noise pattern.
fn is_noise(file_path: &str) -> bool {
    let basename = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    for pattern in NOISE_PATTERNS {
        if let Some(suffix) = pattern.strip_prefix('*') {
            // Suffix match: *.pyc matches any file ending in .pyc
            if basename.ends_with(suffix) {
                return true;
            }
        } else {
            // Exact basename match
            if basename == *pattern {
                return true;
            }
            // Also match as a directory component
            if file_path.contains(&format!("{pattern}/")) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::git::tests::MockGitBuilder;

    #[test]
    fn is_noise_ds_store() {
        assert!(is_noise(".DS_Store"));
        assert!(is_noise("subdir/.DS_Store"));
    }

    #[test]
    fn is_noise_pyc() {
        assert!(is_noise("module.pyc"));
        assert!(is_noise("src/module.pyc"));
    }

    #[test]
    fn is_noise_pycache() {
        assert!(is_noise("__pycache__"));
        assert!(is_noise("src/__pycache__/module.cpython-39.pyc"));
    }

    #[test]
    fn is_noise_lockfiles() {
        assert!(is_noise("uv.lock"));
        assert!(is_noise("package-lock.json"));
        assert!(is_noise("Podfile.lock"));
        assert!(is_noise("yarn.lock"));
    }

    #[test]
    fn is_not_noise_regular_files() {
        assert!(!is_noise("main.rs"));
        assert!(!is_noise("src/lib.rs"));
        assert!(!is_noise("Cargo.toml"));
        assert!(!is_noise("README.md"));
    }

    #[test]
    fn check_dirty_all_noise() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(
                &path,
                vec![
                    "?? .DS_Store".to_string(),
                    "?? __pycache__/".to_string(),
                    "?? uv.lock".to_string(),
                ],
            )
            .build();

        let result = check_dirty(&git, &path).unwrap();
        assert_eq!(result.all_files.len(), 3);
        assert!(result.meaningful_files.is_empty());
    }

    #[test]
    fn check_dirty_mixed() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(
                &path,
                vec![
                    " M src/main.rs".to_string(),
                    "?? .DS_Store".to_string(),
                    "?? new_file.txt".to_string(),
                ],
            )
            .build();

        let result = check_dirty(&git, &path).unwrap();
        assert_eq!(result.all_files.len(), 3);
        assert_eq!(result.meaningful_files.len(), 2);
        assert!(result.meaningful_files.contains(&"src/main.rs".to_string()));
        assert!(result.meaningful_files.contains(&"new_file.txt".to_string()));
    }

    #[test]
    fn check_dirty_staged_changes() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(
                &path,
                vec!["M  src/lib.rs".to_string(), "A  new.rs".to_string()],
            )
            .build();

        let result = check_dirty(&git, &path).unwrap();
        assert_eq!(result.meaningful_files.len(), 2);
    }

    #[test]
    fn check_dirty_empty() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(&path, vec![])
            .build();

        let result = check_dirty(&git, &path).unwrap();
        assert!(result.all_files.is_empty());
        assert!(result.meaningful_files.is_empty());
    }
}
