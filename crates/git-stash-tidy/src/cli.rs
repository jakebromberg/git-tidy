use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-stash-tidy",
    about = "Scan, classify, and interactively drop stale Git stashes"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,

    /// Days before a stash is considered aged
    #[arg(long, default_value_t = 90, global = true)]
    pub age_threshold: u64,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of stash entries
    Scan {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },

    /// Scan and interactively drop stale stashes
    Clean {
        /// Show what would be dropped without dropping
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Only target committed stashes
        #[arg(long)]
        committed_only: bool,

        /// Only target aged stashes
        #[arg(long)]
        aged_only: bool,

        /// Include all stashes except active
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
        self.directory.clone().unwrap_or_else(|| {
            std::env::current_dir().expect("could not determine current directory")
        })
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn default_command_is_scan() {
        let cli = Cli::parse_from(["git-stash-tidy"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-stash-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn clean_with_flags() {
        let cli = Cli::parse_from([
            "git-stash-tidy",
            "clean",
            "--dry-run",
            "--yes",
            "--committed-only",
        ]);
        match cli.command {
            Some(Command::Clean {
                dry_run,
                yes,
                committed_only,
                ..
            }) => {
                assert!(dry_run);
                assert!(yes);
                assert!(committed_only);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn age_threshold_default() {
        let cli = Cli::parse_from(["git-stash-tidy"]);
        assert_eq!(cli.age_threshold, 90);
    }

    #[test]
    fn age_threshold_custom() {
        let cli = Cli::parse_from(["git-stash-tidy", "--age-threshold", "30"]);
        assert_eq!(cli.age_threshold, 30);
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-stash-tidy", "scan", "--json"]);
        match cli.command {
            Some(Command::Scan { json, porcelain }) => {
                assert!(json);
                assert!(!porcelain);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn scan_porcelain_flag() {
        let cli = Cli::parse_from(["git-stash-tidy", "scan", "--porcelain"]);
        match cli.command {
            Some(Command::Scan { json, porcelain }) => {
                assert!(!json);
                assert!(porcelain);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn clean_all_flag() {
        let cli = Cli::parse_from(["git-stash-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_aged_only_flag() {
        let cli = Cli::parse_from(["git-stash-tidy", "clean", "--aged-only"]);
        match cli.command {
            Some(Command::Clean { aged_only, .. }) => {
                assert!(aged_only);
            }
            _ => panic!("expected Clean command"),
        }
    }
}
