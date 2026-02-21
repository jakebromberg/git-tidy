use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-repo-tidy",
    about = "Scan, classify, and remove stale Git repositories"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,

    /// Show detailed classification reasoning
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Number of months without commits before a repo is considered stale
    #[arg(long, default_value = "6", global = true)]
    pub stale_months: u64,

    /// Skip remote reachability checks (no network access)
    #[arg(long, global = true)]
    pub offline: bool,

    /// Additional noise patterns for dirty detection
    #[arg(long = "noise-pattern", global = true)]
    pub noise_patterns: Vec<String>,

    /// Disable default noise patterns
    #[arg(long, global = true)]
    pub no_default_noise: bool,

    /// Filter repos by name substring (can be repeated, OR semantics)
    #[arg(long = "match-repo", global = true)]
    pub match_repo_patterns: Vec<String>,

    /// Exclude repos by name substring (takes precedence over --match-repo)
    #[arg(long = "exclude-repo", global = true)]
    pub exclude_repo_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of repositories
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

    /// Scan and interactively remove stale repositories
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Allow deleting dirty repos
        #[arg(short = 'f', long)]
        force: bool,

        /// Only delete stale repos
        #[arg(long)]
        stale_only: bool,

        /// Only delete orphaned repos
        #[arg(long)]
        orphaned_only: bool,

        /// Delete all non-active repos (stale + orphaned)
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
        git_tidy_core::cli::resolve_directory(self.directory.clone())
    }

    /// Convert stale_months to days.
    pub fn stale_threshold_days(&self) -> u64 {
        self.stale_months * 30
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn default_command_is_scan() {
        let cli = Cli::parse_from(["git-repo-tidy"]);
        assert!(cli.command.is_none());
        assert!(!cli.offline);
        assert_eq!(cli.stale_months, 6);
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-repo-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-repo-tidy", "scan", "--json"]);
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
        let cli = Cli::parse_from(["git-repo-tidy", "clean", "--dry-run", "--yes", "--force"]);
        match cli.command {
            Some(Command::Clean {
                dry_run,
                yes,
                force,
                ..
            }) => {
                assert!(dry_run);
                assert!(yes);
                assert!(force);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_filter_flags() {
        let cli = Cli::parse_from(["git-repo-tidy", "clean", "--stale-only"]);
        match cli.command {
            Some(Command::Clean { stale_only, .. }) => {
                assert!(stale_only);
            }
            _ => panic!("expected Clean command"),
        }

        let cli = Cli::parse_from(["git-repo-tidy", "clean", "--orphaned-only"]);
        match cli.command {
            Some(Command::Clean { orphaned_only, .. }) => {
                assert!(orphaned_only);
            }
            _ => panic!("expected Clean command"),
        }

        let cli = Cli::parse_from(["git-repo-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn stale_months_default() {
        let cli = Cli::parse_from(["git-repo-tidy"]);
        assert_eq!(cli.stale_months, 6);
        assert_eq!(cli.stale_threshold_days(), 180);
    }

    #[test]
    fn stale_months_custom() {
        let cli = Cli::parse_from(["git-repo-tidy", "--stale-months", "12"]);
        assert_eq!(cli.stale_months, 12);
        assert_eq!(cli.stale_threshold_days(), 360);
    }

    #[test]
    fn offline_flag() {
        let cli = Cli::parse_from(["git-repo-tidy", "--offline", "scan"]);
        assert!(cli.offline);
    }

    #[test]
    fn noise_pattern_flags() {
        let cli = Cli::parse_from([
            "git-repo-tidy",
            "--noise-pattern",
            "*.swp",
            "--noise-pattern",
            ".envrc",
            "scan",
        ]);
        assert_eq!(cli.noise_patterns, vec!["*.swp", ".envrc"]);
    }

    #[test]
    fn no_default_noise_flag() {
        let cli = Cli::parse_from(["git-repo-tidy", "--no-default-noise", "scan"]);
        assert!(cli.no_default_noise);
    }

    #[test]
    fn completions_subcommand_zsh() {
        let cli = Cli::parse_from(["git-repo-tidy", "completions", "zsh"]);
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
            "git-repo-tidy",
            "--match-repo",
            "myproject",
            "--exclude-repo",
            "archive",
            "scan",
        ]);
        assert_eq!(cli.match_repo_patterns, vec!["myproject".to_string()]);
        assert_eq!(cli.exclude_repo_patterns, vec!["archive".to_string()]);
    }
}
