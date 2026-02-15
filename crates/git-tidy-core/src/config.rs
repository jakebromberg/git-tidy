use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::dirty::DEFAULT_NOISE_PATTERNS;

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    noise: NoiseSection,
}

#[derive(Debug, Default, Deserialize)]
struct NoiseSection {
    #[serde(default)]
    extra: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

/// Resolved noise configuration from defaults, config file, and CLI flags.
pub struct NoiseConfig {
    pub config_extra: Vec<String>,
    pub config_exclude: Vec<String>,
    pub cli_extra: Vec<String>,
    pub no_defaults: bool,
}

impl NoiseConfig {
    /// Merge the three layers into a final list of noise patterns.
    ///
    /// Merge order: `final = (defaults - config_exclude) + config_extra + cli_extra`
    /// When `no_defaults` is true, defaults are cleared entirely.
    pub fn resolve(&self) -> Vec<String> {
        let mut patterns: Vec<String> = if self.no_defaults {
            Vec::new()
        } else {
            DEFAULT_NOISE_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .filter(|s| !self.config_exclude.contains(s))
                .collect()
        };

        for extra in &self.config_extra {
            if !patterns.contains(extra) {
                patterns.push(extra.clone());
            }
        }

        for extra in &self.cli_extra {
            if !patterns.contains(extra) {
                patterns.push(extra.clone());
            }
        }

        patterns
    }
}

/// Load noise configuration from a TOML config file.
///
/// Returns `(extra, exclude)` vectors. If the file doesn't exist, returns empty vectors.
/// If the file is malformed, warns to stderr and returns empty vectors.
pub fn load_config_file(path: &Path) -> (Vec<String>, Vec<String>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (vec![], vec![]),
        Err(e) => {
            eprintln!(
                "warning: could not read config file {}: {e}",
                path.display()
            );
            return (vec![], vec![]);
        }
    };

    match toml::from_str::<ConfigFile>(&content) {
        Ok(config) => (config.noise.extra, config.noise.exclude),
        Err(e) => {
            eprintln!(
                "warning: could not parse config file {}: {e}",
                path.display()
            );
            (vec![], vec![])
        }
    }
}

/// Return the default config file path.
///
/// Respects `$XDG_CONFIG_HOME` if set, otherwise falls back to `$HOME/.config`.
pub fn default_config_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(xdg)
                .join("git-worktree-tidy")
                .join("config.toml"),
        );
    }
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("git-worktree-tidy")
            .join("config.toml")
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn defaults() -> Vec<String> {
        DEFAULT_NOISE_PATTERNS
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    #[test]
    fn resolve_defaults_only() {
        let config = NoiseConfig {
            config_extra: vec![],
            config_exclude: vec![],
            cli_extra: vec![],
            no_defaults: false,
        };
        assert_eq!(config.resolve(), defaults());
    }

    #[test]
    fn resolve_with_extras() {
        let config = NoiseConfig {
            config_extra: vec!["*.swp".to_string()],
            config_exclude: vec![],
            cli_extra: vec![],
            no_defaults: false,
        };
        let result = config.resolve();
        assert!(result.contains(&"*.swp".to_string()));
        // Defaults are still present
        assert!(result.contains(&".DS_Store".to_string()));
    }

    #[test]
    fn resolve_with_excludes() {
        let config = NoiseConfig {
            config_extra: vec![],
            config_exclude: vec!["package-lock.json".to_string()],
            cli_extra: vec![],
            no_defaults: false,
        };
        let result = config.resolve();
        assert!(!result.contains(&"package-lock.json".to_string()));
        // Other defaults still present
        assert!(result.contains(&".DS_Store".to_string()));
    }

    #[test]
    fn resolve_no_defaults() {
        let config = NoiseConfig {
            config_extra: vec![],
            config_exclude: vec![],
            cli_extra: vec!["*.swp".to_string()],
            no_defaults: true,
        };
        let result = config.resolve();
        assert_eq!(result, vec!["*.swp".to_string()]);
    }

    #[test]
    fn resolve_no_defaults_ignores_excludes() {
        let config = NoiseConfig {
            config_extra: vec!["*.swp".to_string()],
            config_exclude: vec![".DS_Store".to_string()],
            cli_extra: vec![],
            no_defaults: true,
        };
        let result = config.resolve();
        assert_eq!(result, vec!["*.swp".to_string()]);
    }

    #[test]
    fn resolve_all_three_layers() {
        let config = NoiseConfig {
            config_extra: vec![".envrc".to_string()],
            config_exclude: vec!["uv.lock".to_string()],
            cli_extra: vec!["*.swp".to_string()],
            no_defaults: false,
        };
        let result = config.resolve();
        assert!(!result.contains(&"uv.lock".to_string()));
        assert!(result.contains(&".envrc".to_string()));
        assert!(result.contains(&"*.swp".to_string()));
        assert!(result.contains(&".DS_Store".to_string()));
    }

    #[test]
    fn resolve_deduplicates_extras() {
        let config = NoiseConfig {
            config_extra: vec!["*.swp".to_string()],
            config_exclude: vec![],
            cli_extra: vec!["*.swp".to_string()],
            no_defaults: false,
        };
        let result = config.resolve();
        assert_eq!(result.iter().filter(|p| *p == "*.swp").count(), 1);
    }

    #[test]
    fn load_config_file_missing() {
        let (extra, exclude) = load_config_file(Path::new("/nonexistent/config.toml"));
        assert!(extra.is_empty());
        assert!(exclude.is_empty());
    }

    #[test]
    fn load_config_file_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"[noise]
extra = ["*.swp", ".envrc"]
exclude = ["package-lock.json"]
"#
        )
        .unwrap();

        let (extra, exclude) = load_config_file(&path);
        assert_eq!(extra, vec!["*.swp".to_string(), ".envrc".to_string()]);
        assert_eq!(exclude, vec!["package-lock.json".to_string()]);
    }

    #[test]
    fn load_config_file_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();

        let (extra, exclude) = load_config_file(&path);
        assert!(extra.is_empty());
        assert!(exclude.is_empty());
    }

    #[test]
    fn load_config_file_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "").unwrap();

        let (extra, exclude) = load_config_file(&path);
        assert!(extra.is_empty());
        assert!(exclude.is_empty());
    }

    #[test]
    fn load_config_file_no_noise_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[other]\nkey = \"value\"\n").unwrap();

        let (extra, exclude) = load_config_file(&path);
        assert!(extra.is_empty());
        assert!(exclude.is_empty());
    }

    #[test]
    fn default_config_path_returns_expected_structure() {
        // This test depends on HOME being set, which it typically is
        if let Some(path) = default_config_path() {
            assert!(path.ends_with(".config/git-worktree-tidy/config.toml"));
        }
    }
}
