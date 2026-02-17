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

    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,
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

    /// Fix auto-fixable config issues
    Fix {
        /// Show what would be fixed without fixing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

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
        git_tidy_core::cli::resolve_directory(self.directory.clone())
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
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
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
        let cli = Cli::parse_from(["git-config-tidy", "fix", "--dry-run", "--yes"]);
        match cli.command {
            Some(Command::Fix { dry_run, yes, .. }) => {
                assert!(dry_run);
                assert!(yes);
            }
            _ => panic!("expected Fix command"),
        }
    }

    #[test]
    fn fix_short_flags() {
        let cli = Cli::parse_from(["git-config-tidy", "fix", "-n", "-y"]);
        match cli.command {
            Some(Command::Fix { dry_run, yes, .. }) => {
                assert!(dry_run);
                assert!(yes);
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
}
