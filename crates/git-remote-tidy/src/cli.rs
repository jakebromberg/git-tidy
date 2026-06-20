use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-remote-tidy",
    about = "Scan, classify, and remove stale Git remotes and orphaned tracking refs"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub common: git_tidy_core::cli::CommonArgs,

    /// Show detailed classification reasoning
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Skip reachability checks (no network access)
    #[arg(long, global = true)]
    pub offline: bool,

    #[command(flatten)]
    pub repo_filter: git_tidy_core::cli::RepoFilterArgs,

    /// Filter remotes by name substring (can be repeated, OR semantics)
    #[arg(long = "match", global = true)]
    pub match_patterns: Vec<String>,

    /// Exclude remotes by name substring (takes precedence over --match)
    #[arg(long = "exclude", global = true)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of configured and orphaned remotes
    Scan {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },

    #[command(flatten)]
    Shared(git_tidy_core::cli::SharedCommands),

    /// Scan and remove stale remotes
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Allow removing the origin remote
        #[arg(short = 'f', long)]
        force: bool,

        /// Include orphaned remotes (default: unreachable only)
        #[arg(long)]
        all: bool,

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
    fn default_command_is_scan() {
        let cli = Cli::parse_from(["git-remote-tidy"]);
        assert!(cli.command.is_none());
        assert!(!cli.offline);
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-remote-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.common.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-remote-tidy", "scan", "--json"]);
        match cli.command {
            Some(Command::Scan { json, porcelain }) => {
                assert!(json);
                assert!(!porcelain);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn clean_with_flags() {
        let cli = Cli::parse_from(["git-remote-tidy", "clean", "--dry-run", "--force"]);
        match cli.command {
            Some(Command::Clean { dry_run, force, .. }) => {
                assert!(dry_run);
                assert!(force);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_all_flag() {
        let cli = Cli::parse_from(["git-remote-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn offline_flag() {
        let cli = Cli::parse_from(["git-remote-tidy", "--offline", "scan"]);
        assert!(cli.offline);
    }

    #[test]
    fn offline_flag_with_clean() {
        let cli = Cli::parse_from(["git-remote-tidy", "--offline", "clean", "--dry-run"]);
        assert!(cli.offline);
        match cli.command {
            Some(Command::Clean { dry_run, .. }) => assert!(dry_run),
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn completions_subcommand_zsh() {
        let cli = Cli::parse_from(["git-remote-tidy", "completions", "zsh"]);
        assert!(matches!(
            cli.command,
            Some(Command::Shared(
                git_tidy_core::cli::SharedCommands::Completions { .. }
            ))
        ));
    }

    #[test]
    fn match_and_exclude_flags() {
        let cli = Cli::parse_from([
            "git-remote-tidy",
            "--match",
            "upstream",
            "--exclude",
            "stale",
            "scan",
        ]);
        assert_eq!(cli.match_patterns, vec!["upstream".to_string()]);
        assert_eq!(cli.exclude_patterns, vec!["stale".to_string()]);
    }

    #[test]
    fn match_repo_and_exclude_repo_flags() {
        let cli = Cli::parse_from([
            "git-remote-tidy",
            "--match-repo",
            "myproject",
            "--exclude-repo",
            "archive",
            "scan",
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
