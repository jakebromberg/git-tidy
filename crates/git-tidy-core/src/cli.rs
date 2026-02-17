//! Shared CLI utilities for resolving common command-line arguments.

use std::path::PathBuf;

/// Resolve an optional directory argument, defaulting to the current directory.
pub fn resolve_directory(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| std::env::current_dir().expect("could not determine current directory"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_directory_with_some() {
        let path = PathBuf::from("/tmp/test");
        assert_eq!(resolve_directory(Some(path.clone())), path);
    }

    #[test]
    fn resolve_directory_with_none() {
        let result = resolve_directory(None);
        // Should return the current directory
        assert!(result.is_absolute());
        assert_eq!(result, std::env::current_dir().unwrap());
    }
}
