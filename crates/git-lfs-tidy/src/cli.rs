use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-lfs-tidy",
    about = "Scan repos for LFS health issues and clean up orphaned LFS objects"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,

    /// Filter repos by name substring (can be repeated, OR semantics)
    #[arg(long = "match-repo", global = true)]
    pub match_repo_patterns: Vec<String>,

    /// Exclude repos by name substring (takes precedence over --match-repo)
    #[arg(long = "exclude-repo", global = true)]
    pub exclude_repo_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of LFS health
    Scan {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,

        /// Minimum blob size to flag as untracked (e.g. "1MB", "500KB")
        #[arg(long, default_value = "1MB")]
        size_threshold: String,

        /// Maximum number of commits to scan for large blobs
        #[arg(long, default_value_t = 1000)]
        depth: usize,
    },

    #[command(flatten)]
    Shared(git_tidy_core::cli::SharedCommands),

    /// Scan and clean up orphaned LFS objects
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Enable pruning of orphaned LFS objects
        #[arg(long)]
        prune: bool,

        /// Minimum blob size to flag as untracked (e.g. "1MB", "500KB")
        #[arg(long, default_value = "1MB")]
        size_threshold: String,

        /// Maximum number of commits to scan for large blobs
        #[arg(long, default_value_t = 1000)]
        depth: usize,

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
    fn default_command_is_scan() {
        let cli = Cli::parse_from(["git-lfs-tidy"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-lfs-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-lfs-tidy", "scan", "--json"]);
        match cli.command {
            Some(Command::Scan {
                json, porcelain, ..
            }) => {
                assert!(json);
                assert!(!porcelain);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn scan_size_threshold() {
        let cli = Cli::parse_from(["git-lfs-tidy", "scan", "--size-threshold", "500KB"]);
        match cli.command {
            Some(Command::Scan { size_threshold, .. }) => {
                assert_eq!(size_threshold, "500KB");
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn scan_depth() {
        let cli = Cli::parse_from(["git-lfs-tidy", "scan", "--depth", "500"]);
        match cli.command {
            Some(Command::Scan { depth, .. }) => {
                assert_eq!(depth, 500);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn clean_with_flags() {
        let cli = Cli::parse_from(["git-lfs-tidy", "clean", "--dry-run", "--yes", "--prune"]);
        match cli.command {
            Some(Command::Clean {
                dry_run,
                yes,
                prune,
                ..
            }) => {
                assert!(dry_run);
                assert!(yes);
                assert!(prune);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_default_thresholds() {
        let cli = Cli::parse_from(["git-lfs-tidy", "clean"]);
        match cli.command {
            Some(Command::Clean {
                size_threshold,
                depth,
                ..
            }) => {
                assert_eq!(size_threshold, "1MB");
                assert_eq!(depth, 1000);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn completions_subcommand_zsh() {
        let cli = Cli::parse_from(["git-lfs-tidy", "completions", "zsh"]);
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
            "git-lfs-tidy",
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
