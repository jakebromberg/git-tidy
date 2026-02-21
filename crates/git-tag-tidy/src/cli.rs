use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-tag-tidy",
    about = "Scan, classify, and remove stale Git tags"
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

    /// Skip remote tag queries (no network access)
    #[arg(long, global = true)]
    pub offline: bool,

    /// Filter repos by name substring (can be repeated, OR semantics)
    #[arg(long = "match-repo", global = true)]
    pub match_repo_patterns: Vec<String>,

    /// Exclude repos by name substring (takes precedence over --match-repo)
    #[arg(long = "exclude-repo", global = true)]
    pub exclude_repo_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of local and remote tags
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

    /// Scan and interactively remove stale tags
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Allow deleting synced tags and bypass release tag warnings
        #[arg(short = 'f', long)]
        force: bool,

        /// Only delete stale tags
        #[arg(long)]
        stale_only: bool,

        /// Only delete local-only tags
        #[arg(long)]
        local_only: bool,

        /// Also delete remote copies when cleaning
        #[arg(long)]
        include_remote: bool,

        /// Delete stale + local_only + remote_only (not synced)
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
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn default_command_is_scan() {
        let cli = Cli::parse_from(["git-tag-tidy"]);
        assert!(cli.command.is_none());
        assert!(!cli.offline);
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-tag-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "scan", "--json"]);
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
        let cli = Cli::parse_from(["git-tag-tidy", "clean", "--dry-run", "--yes", "--force"]);
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
    fn clean_stale_only_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "clean", "--stale-only"]);
        match cli.command {
            Some(Command::Clean { stale_only, .. }) => {
                assert!(stale_only);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_local_only_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "clean", "--local-only"]);
        match cli.command {
            Some(Command::Clean { local_only, .. }) => {
                assert!(local_only);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_include_remote_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "clean", "--include-remote"]);
        match cli.command {
            Some(Command::Clean { include_remote, .. }) => {
                assert!(include_remote);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_all_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn offline_flag() {
        let cli = Cli::parse_from(["git-tag-tidy", "--offline", "scan"]);
        assert!(cli.offline);
    }

    #[test]
    fn offline_flag_with_clean() {
        let cli = Cli::parse_from(["git-tag-tidy", "--offline", "clean", "--dry-run"]);
        assert!(cli.offline);
        match cli.command {
            Some(Command::Clean { dry_run, .. }) => assert!(dry_run),
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn completions_subcommand_zsh() {
        let cli = Cli::parse_from(["git-tag-tidy", "completions", "zsh"]);
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
            "git-tag-tidy",
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
