use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-config-tidy",
    about = "Lint and fix common Git config issues"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub common: git_tidy_core::cli::CommonArgs,

    /// Show detailed lint reasoning
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(flatten)]
    pub repo_filter: git_tidy_core::cli::RepoFilterArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of local git config
    Lint {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },

    #[command(flatten)]
    Shared(git_tidy_core::cli::SharedCommands),

    /// Fix auto-fixable config issues
    Fix {
        /// Show what would be fixed without fixing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },
}

impl Cli {
    /// Resolve the target directory, defaulting to the current directory.
    pub fn target_directory(&self) -> PathBuf {
        git_tidy_core::cli::resolve_directory(self.common.directory.clone())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn default_command_is_lint() {
        let cli = Cli::parse_from(["git-config-tidy"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn lint_with_directory() {
        let cli = Cli::parse_from(["git-config-tidy", "lint", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Lint { .. })));
        assert_eq!(cli.common.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn lint_json_flag() {
        let cli = Cli::parse_from(["git-config-tidy", "lint", "--json"]);
        match cli.command {
            Some(Command::Lint { json, porcelain }) => {
                assert!(json);
                assert!(!porcelain);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn lint_porcelain_flag() {
        let cli = Cli::parse_from(["git-config-tidy", "lint", "--porcelain"]);
        match cli.command {
            Some(Command::Lint { json, porcelain }) => {
                assert!(!json);
                assert!(porcelain);
            }
            _ => panic!("expected Lint command"),
        }
    }

    #[test]
    fn fix_with_flags() {
        let cli = Cli::parse_from(["git-config-tidy", "fix", "--dry-run"]);
        match cli.command {
            Some(Command::Fix { dry_run, .. }) => {
                assert!(dry_run);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn fix_short_flags() {
        let cli = Cli::parse_from(["git-config-tidy", "fix", "-n"]);
        match cli.command {
            Some(Command::Fix { dry_run, .. }) => {
                assert!(dry_run);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn fix_json_flag() {
        let cli = Cli::parse_from(["git-config-tidy", "fix", "--json"]);
        match cli.command {
            Some(Command::Fix { json, .. }) => {
                assert!(json);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn completions_subcommand_zsh() {
        let cli = Cli::parse_from(["git-config-tidy", "completions", "zsh"]);
        assert!(matches!(
            cli.command,
            Some(Command::Shared(
                git_tidy_core::cli::SharedCommands::Completions { .. }
            ))
        ));
    }

    #[test]
    fn match_repo_and_exclude_repo_flags() {
        let cli = Cli::parse_from([
            "git-config-tidy",
            "--match-repo",
            "myproject",
            "--exclude-repo",
            "archive",
            "lint",
        ]);
        assert_eq!(
            cli.repo_filter.match_repo_patterns,
            vec!["myproject".to_string()]
        );
        assert_eq!(
            cli.repo_filter.exclude_repo_patterns,
            vec!["archive".to_string()]
        );
    }
}
