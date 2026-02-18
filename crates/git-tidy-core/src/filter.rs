use std::path::PathBuf;

/// Case-insensitive substring name filter.
///
/// Include patterns use OR semantics (match any). Exclude takes precedence.
/// An empty include list means "match all". An empty exclude list means "exclude none".
#[derive(Debug, Clone, Default)]
pub struct NameFilter {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl NameFilter {
    /// Create a new filter. Patterns are lowercased at construction time.
    pub fn new(include: &[String], exclude: &[String]) -> Self {
        Self {
            include: include.iter().map(|p| p.to_lowercase()).collect(),
            exclude: exclude.iter().map(|p| p.to_lowercase()).collect(),
        }
    }

    /// Returns `true` when no filtering is needed (both lists empty).
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Test whether `name` passes the filter.
    ///
    /// 1. If any exclude pattern matches (substring, case-insensitive) -> `false`
    /// 2. If include is empty -> `true` (match all)
    /// 3. If any include pattern matches -> `true`
    /// 4. Otherwise -> `false`
    pub fn matches(&self, name: &str) -> bool {
        let lower = name.to_lowercase();

        if self.exclude.iter().any(|p| lower.contains(p.as_str())) {
            return false;
        }

        if self.include.is_empty() {
            return true;
        }

        self.include.iter().any(|p| lower.contains(p.as_str()))
    }
}

/// Filter a `Vec<PathBuf>` by basename using a `NameFilter`.
///
/// Extracts `file_name()` from each path and calls `filter.matches()`.
/// Returns all paths unchanged when `filter.is_empty()`.
pub fn filter_paths(paths: Vec<PathBuf>, filter: &NameFilter) -> Vec<PathBuf> {
    if filter.is_empty() {
        return paths;
    }

    paths
        .into_iter()
        .filter(|p| {
            let basename = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            filter.matches(basename)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- NameFilter::new ---

    #[test]
    fn new_lowercases_patterns() {
        let f = NameFilter::new(
            &["FoO".to_string(), "BAR".to_string()],
            &["Baz".to_string()],
        );
        assert_eq!(f.include, vec!["foo", "bar"]);
        assert_eq!(f.exclude, vec!["baz"]);
    }

    // --- NameFilter::is_empty ---

    #[test]
    fn is_empty_when_both_empty() {
        let f = NameFilter::default();
        assert!(f.is_empty());
    }

    #[test]
    fn is_empty_false_with_include() {
        let f = NameFilter::new(&["foo".to_string()], &[]);
        assert!(!f.is_empty());
    }

    #[test]
    fn is_empty_false_with_exclude() {
        let f = NameFilter::new(&[], &["foo".to_string()]);
        assert!(!f.is_empty());
    }

    // --- NameFilter::matches ---

    #[test]
    fn empty_filter_matches_everything() {
        let f = NameFilter::default();
        assert!(f.matches("anything"));
        assert!(f.matches(""));
    }

    #[test]
    fn include_matches_substring() {
        let f = NameFilter::new(&["feat".to_string()], &[]);
        assert!(f.matches("my-feature-branch"));
        assert!(!f.matches("bugfix-branch"));
    }

    #[test]
    fn include_is_case_insensitive() {
        let f = NameFilter::new(&["FEATURE".to_string()], &[]);
        assert!(f.matches("my-feature-branch"));
        assert!(f.matches("My-Feature-Branch"));
        assert!(f.matches("MY-FEATURE-BRANCH"));
    }

    #[test]
    fn multiple_include_patterns_use_or_semantics() {
        let f = NameFilter::new(&["alpha".to_string(), "gamma".to_string()], &[]);
        assert!(f.matches("alpha-thing"));
        assert!(f.matches("gamma-thing"));
        assert!(!f.matches("beta-thing"));
    }

    #[test]
    fn exclude_rejects_matching_names() {
        let f = NameFilter::new(&[], &["secret".to_string()]);
        assert!(!f.matches("my-secret-project"));
        assert!(f.matches("my-public-project"));
    }

    #[test]
    fn exclude_is_case_insensitive() {
        let f = NameFilter::new(&[], &["SECRET".to_string()]);
        assert!(!f.matches("my-secret-project"));
    }

    #[test]
    fn exclude_takes_precedence_over_include() {
        let f = NameFilter::new(&["feat".to_string()], &["wip".to_string()]);
        assert!(f.matches("feat-login"));
        assert!(!f.matches("feat-wip-draft"));
        assert!(!f.matches("wip-stuff"));
    }

    #[test]
    fn include_without_match_rejects() {
        let f = NameFilter::new(&["feat".to_string()], &[]);
        assert!(!f.matches("bugfix-login"));
    }

    #[test]
    fn exclude_only_passes_non_matching() {
        let f = NameFilter::new(&[], &["test".to_string()]);
        assert!(f.matches("production"));
        assert!(!f.matches("test-branch"));
    }

    // --- filter_paths ---

    #[test]
    fn filter_paths_empty_filter_returns_all() {
        let paths = vec![PathBuf::from("/dev/repo-a"), PathBuf::from("/dev/repo-b")];
        let f = NameFilter::default();
        let result = filter_paths(paths.clone(), &f);
        assert_eq!(result, paths);
    }

    #[test]
    fn filter_paths_filters_by_basename() {
        let paths = vec![
            PathBuf::from("/dev/my-feature"),
            PathBuf::from("/dev/my-bugfix"),
            PathBuf::from("/dev/other-feature"),
        ];
        let f = NameFilter::new(&["feature".to_string()], &[]);
        let result = filter_paths(paths, &f);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], PathBuf::from("/dev/my-feature"));
        assert_eq!(result[1], PathBuf::from("/dev/other-feature"));
    }

    #[test]
    fn filter_paths_with_exclude() {
        let paths = vec![
            PathBuf::from("/dev/repo-alpha"),
            PathBuf::from("/dev/repo-beta"),
            PathBuf::from("/dev/repo-gamma"),
        ];
        let f = NameFilter::new(&[], &["beta".to_string()]);
        let result = filter_paths(paths, &f);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], PathBuf::from("/dev/repo-alpha"));
        assert_eq!(result[1], PathBuf::from("/dev/repo-gamma"));
    }

    #[test]
    fn filter_paths_uses_basename_not_full_path() {
        let paths = vec![PathBuf::from("/feature/bugfix-branch")];
        let f = NameFilter::new(&["bugfix".to_string()], &[]);
        let result = filter_paths(paths, &f);
        assert_eq!(result.len(), 1);

        // "feature" is in the parent dir, not the basename
        let paths2 = vec![PathBuf::from("/feature/bugfix-branch")];
        let f2 = NameFilter::new(&["feature".to_string()], &[]);
        let result2 = filter_paths(paths2, &f2);
        assert_eq!(result2.len(), 0);
    }
}
