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

    // Pre-compute slash-suffixed directory patterns once
    let dir_patterns: Vec<String> = noise_patterns
        .iter()
        .filter(|p| !p.starts_with('*'))
        .map(|p| format!("{p}/"))
        .collect();

    let mut all_files = Vec::new();
    let mut meaningful_files = Vec::new();

    for line in &lines {
        if line.len() < 4 {
            continue;
        }
        let status_code = &line[..2];
        let raw_path = line[3..].trim();

        if raw_path.is_empty() {
            continue;
        }

        // Rename status (R*) and copy status (C*) use the format `<old> -> <new>` in porcelain v1. The "current" file the working tree contains is the new path; using the raw "old -> new" string breaks noise filtering (the basename match runs against the literal "new" segment fine, but a rename of `.DS_Store -> foo` would also be reported and the file_path stored in the result would contain " -> ").
        let file_path = if status_code.starts_with('R') || status_code.starts_with('C') {
            raw_path
                .rsplit_once(" -> ")
                .map(|(_, new)| new)
                .unwrap_or(raw_path)
        } else {
            raw_path
        };

        all_files.push(file_path.to_string());

        // Untracked files that match noise patterns are not meaningful
        if status_code == "??" && is_noise(file_path, noise_patterns, &dir_patterns) {
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
///
/// `dir_patterns` contains pre-computed slash-suffixed versions of non-glob patterns
/// (e.g., `"__pycache__/"`) to avoid repeated allocations in loops.
fn is_noise(file_path: &str, patterns: &[String], dir_patterns: &[String]) -> bool {
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
        }
    }

    // Check directory component matches using pre-computed patterns
    for dir_pat in dir_patterns {
        if file_path.contains(dir_pat.as_str()) {
            return true;
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

    /// Compute dir_patterns from a set of noise patterns (mirrors check_dirty logic).
    fn dir_pats(patterns: &[String]) -> Vec<String> {
        patterns
            .iter()
            .filter(|p| !p.starts_with('*'))
            .map(|p| format!("{p}/"))
            .collect()
    }

    #[test]
    fn is_noise_ds_store() {
        let patterns = defaults();
        let dp = dir_pats(&patterns);
        assert!(is_noise(".DS_Store", &patterns, &dp));
        assert!(is_noise("subdir/.DS_Store", &patterns, &dp));
    }

    #[test]
    fn is_noise_pyc() {
        let patterns = defaults();
        let dp = dir_pats(&patterns);
        assert!(is_noise("module.pyc", &patterns, &dp));
        assert!(is_noise("src/module.pyc", &patterns, &dp));
    }

    #[test]
    fn is_noise_pycache() {
        let patterns = defaults();
        let dp = dir_pats(&patterns);
        assert!(is_noise("__pycache__", &patterns, &dp));
        assert!(is_noise(
            "src/__pycache__/module.cpython-39.pyc",
            &patterns,
            &dp
        ));
    }

    #[test]
    fn is_noise_lockfiles() {
        let patterns = defaults();
        let dp = dir_pats(&patterns);
        assert!(is_noise("uv.lock", &patterns, &dp));
        assert!(is_noise("package-lock.json", &patterns, &dp));
        assert!(is_noise("Podfile.lock", &patterns, &dp));
        assert!(is_noise("yarn.lock", &patterns, &dp));
    }

    #[test]
    fn is_not_noise_regular_files() {
        let patterns = defaults();
        let dp = dir_pats(&patterns);
        assert!(!is_noise("main.rs", &patterns, &dp));
        assert!(!is_noise("src/lib.rs", &patterns, &dp));
        assert!(!is_noise("Cargo.toml", &patterns, &dp));
        assert!(!is_noise("README.md", &patterns, &dp));
    }

    #[test]
    fn is_noise_custom_suffix_pattern() {
        let patterns = vec!["*.swp".to_string()];
        let dp = dir_pats(&patterns);
        assert!(is_noise("file.swp", &patterns, &dp));
        assert!(is_noise("dir/file.swp", &patterns, &dp));
        assert!(!is_noise("file.txt", &patterns, &dp));
    }

    #[test]
    fn is_noise_custom_exact_pattern() {
        let patterns = vec![".envrc".to_string()];
        let dp = dir_pats(&patterns);
        assert!(is_noise(".envrc", &patterns, &dp));
        assert!(is_noise("subdir/.envrc", &patterns, &dp));
        assert!(!is_noise("envrc", &patterns, &dp));
    }

    #[test]
    fn is_noise_empty_patterns() {
        let patterns: Vec<String> = vec![];
        let dp = dir_pats(&patterns);
        assert!(!is_noise(".DS_Store", &patterns, &dp));
        assert!(!is_noise("anything", &patterns, &dp));
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
    fn check_dirty_rename_extracts_new_path() {
        // Regression: porcelain v1 reports a rename as `R  old -> new`. Previously file_path stored the literal "old -> new", breaking noise filtering and producing confusing dirty-file output.
        let path = PathBuf::from("/worktree");
        let git = MockGitBuilder::new()
            .with_status_porcelain(
                &path,
                vec![
                    "R  src/old.rs -> src/new.rs".to_string(),
                    "C  src/orig.rs -> src/copy.rs".to_string(),
                ],
            )
            .build();

        let result = check_dirty(&git, &path, &defaults()).unwrap();
        assert!(
            result.all_files.contains(&"src/new.rs".to_string()),
            "rename should store the new path, got {:?}",
            result.all_files,
        );
        assert!(
            result.all_files.contains(&"src/copy.rs".to_string()),
            "copy should store the new path, got {:?}",
            result.all_files,
        );
        assert!(
            !result.all_files.iter().any(|f| f.contains(" -> ")),
            "no file_path should contain the arrow separator, got {:?}",
            result.all_files,
        );
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
