use std::path::Path;

use crate::error::Error;
use crate::git::GitOps;

/// Default noise patterns: files that are always considered noise regardless of gitignore status.
pub const DEFAULT_NOISE_PATTERNS: &[&str] = &[
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
pub fn check_dirty(
    git: &dyn GitOps,
    worktree_path: &Path,
    noise_patterns: &[String],
) -> Result<DirtyResult, Error> {
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
        if status_code == "??" && is_noise(file_path, noise_patterns) {
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
fn is_noise(file_path: &str, patterns: &[String]) -> bool {
    let basename = Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path);

    for pattern in patterns {
        if let Some(suffix) = pattern.strip_prefix('*') {
            // Suffix match: *.pyc matches any file ending in .pyc
            if basename.ends_with(suffix) {
                return true;
            }
        } else {
            // Exact basename match
            if basename == pattern.as_str() {
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
    use crate::testutil::MockGitBuilder;

    fn defaults() -> Vec<String> {
        DEFAULT_NOISE_PATTERNS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    #[test]
    fn is_noise_ds_store() {
        let patterns = defaults();
        assert!(is_noise(".DS_Store", &patterns));
        assert!(is_noise("subdir/.DS_Store", &patterns));
    }

    #[test]
    fn is_noise_pyc() {
        let patterns = defaults();
        assert!(is_noise("module.pyc", &patterns));
        assert!(is_noise("src/module.pyc", &patterns));
    }

    #[test]
    fn is_noise_pycache() {
        let patterns = defaults();
        assert!(is_noise("__pycache__", &patterns));
        assert!(is_noise("src/__pycache__/module.cpython-39.pyc", &patterns));
    }

    #[test]
    fn is_noise_lockfiles() {
        let patterns = defaults();
        assert!(is_noise("uv.lock", &patterns));
        assert!(is_noise("package-lock.json", &patterns));
        assert!(is_noise("Podfile.lock", &patterns));
        assert!(is_noise("yarn.lock", &patterns));
    }

    #[test]
    fn is_not_noise_regular_files() {
        let patterns = defaults();
        assert!(!is_noise("main.rs", &patterns));
        assert!(!is_noise("src/lib.rs", &patterns));
        assert!(!is_noise("Cargo.toml", &patterns));
        assert!(!is_noise("README.md", &patterns));
    }

    #[test]
    fn is_noise_custom_suffix_pattern() {
        let patterns = vec!["*.swp".to_string()];
        assert!(is_noise("file.swp", &patterns));
        assert!(is_noise("dir/file.swp", &patterns));
        assert!(!is_noise("file.txt", &patterns));
    }

    #[test]
    fn is_noise_custom_exact_pattern() {
        let patterns = vec![".envrc".to_string()];
        assert!(is_noise(".envrc", &patterns));
        assert!(is_noise("subdir/.envrc", &patterns));
        assert!(!is_noise("envrc", &patterns));
    }

    #[test]
    fn is_noise_empty_patterns() {
        let patterns: Vec<String> = vec![];
        assert!(!is_noise(".DS_Store", &patterns));
        assert!(!is_noise("anything", &patterns));
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

        let result = check_dirty(&git, &path, &defaults()).unwrap();
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

        let result = check_dirty(&git, &path, &defaults()).unwrap();
        assert_eq!(result.all_files.len(), 3);
        assert_eq!(result.meaningful_files.len(), 2);
        assert!(result.meaningful_files.contains(&"src/main.rs".to_string()));
        assert!(
            result
                .meaningful_files
                .contains(&"new_file.txt".to_string())
        );
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

        let result = check_dirty(&git, &path, &defaults()).unwrap();
        assert_eq!(result.meaningful_files.len(), 2);
    }

    #[test]
    fn check_dirty_empty() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(&path, vec![])
            .build();

        let result = check_dirty(&git, &path, &defaults()).unwrap();
        assert!(result.all_files.is_empty());
        assert!(result.meaningful_files.is_empty());
    }

    #[test]
    fn check_dirty_custom_patterns() {
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(
                &path,
                vec![
                    "?? .envrc".to_string(),
                    "?? file.swp".to_string(),
                    "?? important.txt".to_string(),
                ],
            )
            .build();

        let patterns = vec![".envrc".to_string(), "*.swp".to_string()];
        let result = check_dirty(&git, &path, &patterns).unwrap();
        assert_eq!(result.all_files.len(), 3);
        assert_eq!(result.meaningful_files.len(), 1);
        assert_eq!(result.meaningful_files[0], "important.txt");
    }
}
