//! Shared CLI utilities for resolving common command-line arguments.

use std::path::{Path, PathBuf};

use clap::{CommandFactory, Subcommand};

use crate::error::Error;

/// Shared subcommands available in all git-tidy tools.
#[derive(Subcommand, Debug)]
pub enum SharedCommands {
    /// Generate shell completions
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

impl SharedCommands {
    /// Execute the shared command (e.g., generate completions for the given binary).
    pub fn run<C: CommandFactory>(&self, bin_name: &str) {
        match self {
            SharedCommands::Completions { shell } => {
                let mut cmd = C::command();
                clap_complete::generate(*shell, &mut cmd, bin_name, &mut std::io::stdout());
            }
        }
    }
}

/// Output format for scan/clean results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Porcelain,
}

impl OutputFormat {
    /// Derive the output format from the `--json` and `--porcelain` CLI flags.
    pub fn from_flags(json: bool, porcelain: bool) -> Self {
        if json {
            Self::Json
        } else if porcelain {
            Self::Porcelain
        } else {
            Self::Human
        }
    }
}

/// Resolve an optional directory argument, defaulting to the current directory.
pub fn resolve_directory(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| std::env::current_dir().expect("could not determine current directory"))
}

/// Validate that a path is an existing directory.
pub fn validate_directory(directory: &Path) -> Result<(), Error> {
    if !directory.is_dir() {
        return Err(Error::DirectoryNotFound {
            path: directory.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser, Debug)]
    #[command(name = "test-tool")]
    struct TestCli {
        #[command(subcommand)]
        command: Option<TestCommand>,
    }

    #[derive(Subcommand, Debug)]
    enum TestCommand {
        Scan,
        #[command(flatten)]
        Shared(SharedCommands),
    }

    #[test]
    fn shared_commands_parses_completions_zsh() {
        let cli = TestCli::parse_from(["test-tool", "completions", "zsh"]);
        assert!(matches!(
            cli.command,
            Some(TestCommand::Shared(SharedCommands::Completions { .. }))
        ));
    }

    #[test]
    fn shared_commands_parses_completions_bash() {
        let cli = TestCli::parse_from(["test-tool", "completions", "bash"]);
        match cli.command {
            Some(TestCommand::Shared(SharedCommands::Completions { shell })) => {
                assert_eq!(shell, clap_complete::Shell::Bash);
            }
            _ => panic!("expected Completions command"),
        }
    }

    #[test]
    fn shared_commands_parses_completions_fish() {
        let cli = TestCli::parse_from(["test-tool", "completions", "fish"]);
        match cli.command {
            Some(TestCommand::Shared(SharedCommands::Completions { shell })) => {
                assert_eq!(shell, clap_complete::Shell::Fish);
            }
            _ => panic!("expected Completions command"),
        }
    }

    #[test]
    fn shared_commands_coexist_with_tool_commands() {
        let cli = TestCli::parse_from(["test-tool", "scan"]);
        assert!(matches!(cli.command, Some(TestCommand::Scan)));
    }

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

    #[test]
    fn output_format_from_flags() {
        assert_eq!(OutputFormat::from_flags(false, false), OutputFormat::Human);
        assert_eq!(OutputFormat::from_flags(true, false), OutputFormat::Json);
        assert_eq!(
            OutputFormat::from_flags(false, true),
            OutputFormat::Porcelain
        );
        // json takes precedence
        assert_eq!(OutputFormat::from_flags(true, true), OutputFormat::Json);
    }

    #[test]
    fn validate_directory_exists() {
        assert!(validate_directory(Path::new("/tmp")).is_ok());
    }

    #[test]
    fn validate_directory_not_found() {
        let result = validate_directory(Path::new("/nonexistent/path/xyz"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::DirectoryNotFound { .. }));
    }
}
