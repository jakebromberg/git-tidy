//! Shared CLI utilities for resolving common command-line arguments.

use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, Subcommand};

use crate::error::Error;

// The shared `[DIRECTORY]` positional, common to every git-tidy binary.
//
// Flatten this into a tool's `Cli` with `#[command(flatten)]` to inherit the
// directory argument without re-declaring it. The flatten preserves clap's
// `global = true` semantics, so the directory is accepted both before and
// after a subcommand.
//
// NOTE: this uses a plain `//` comment rather than a `///` doc comment on
// purpose. clap consumes an `Args` struct's doc comment as the *parent*
// command's `about`/`long_about` when the struct is flattened at the top
// level, which would rewrite every binary's `--help` header. The per-field
// doc comments below still become the flag help text.
#[derive(Args, Debug)]
pub struct CommonArgs {
    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,
}

// The shared repo-name filter flags (`--match-repo` / `--exclude-repo`),
// common to every scan-shaped git-tidy binary.
//
// Flatten this into a tool's `Cli` at the position where the repo filters
// should appear in `--help`; the surrounding tool-specific flags keep their
// declaration order. The flags carry `global = true`, so they are accepted
// after a subcommand.
//
// NOTE: plain `//` comment on purpose, for the same reason as `CommonArgs`.
#[derive(Args, Debug)]
pub struct RepoFilterArgs {
    /// Filter repos by name substring (can be repeated, OR semantics)
    #[arg(long = "match-repo", global = true)]
    pub match_repo_patterns: Vec<String>,

    /// Exclude repos by name substring (takes precedence over --match-repo)
    #[arg(long = "exclude-repo", global = true)]
    pub exclude_repo_patterns: Vec<String>,
}

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

    #[derive(Parser, Debug)]
    #[command(name = "filter-tool")]
    struct FilterCli {
        #[command(subcommand)]
        command: Option<TestCommand>,

        #[command(flatten)]
        common: CommonArgs,

        #[command(flatten)]
        repo_filter: RepoFilterArgs,
    }

    #[test]
    fn common_args_directory_before_subcommand() {
        let cli = FilterCli::parse_from(["filter-tool", "/tmp/dev"]);
        assert_eq!(cli.common.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn common_args_directory_after_subcommand() {
        let cli = FilterCli::parse_from(["filter-tool", "scan", "/tmp/dev"]);
        assert_eq!(cli.common.directory, Some(PathBuf::from("/tmp/dev")));
        assert!(matches!(cli.command, Some(TestCommand::Scan)));
    }

    #[test]
    fn repo_filter_args_global_after_subcommand() {
        let cli = FilterCli::parse_from([
            "filter-tool",
            "scan",
            "--match-repo",
            "myproject",
            "--exclude-repo",
            "archive",
        ]);
        assert_eq!(cli.repo_filter.match_repo_patterns, vec!["myproject"]);
        assert_eq!(cli.repo_filter.exclude_repo_patterns, vec!["archive"]);
        assert!(matches!(cli.command, Some(TestCommand::Scan)));
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
