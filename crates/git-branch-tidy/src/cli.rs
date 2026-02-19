use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-branch-tidy",
    about = "Scan, classify, and interactively remove stale local Git branches"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,

    /// Commit count for diverged annotation
    #[arg(long, default_value_t = 100, global = true)]
    pub behind_threshold: usize,

    /// Show commit-matching details during landed detection
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Filter repos by name substring (can be repeated, OR semantics)
    #[arg(long = "match-repo", global = true)]
    pub match_repo_patterns: Vec<String>,

    /// Exclude repos by name substring (takes precedence over --match-repo)
    #[arg(long = "exclude-repo", global = true)]
    pub exclude_repo_patterns: Vec<String>,

    /// Filter branches by name substring (can be repeated, OR semantics)
    #[arg(long = "match", global = true)]
    pub match_patterns: Vec<String>,

    /// Exclude branches by name substring (takes precedence over --match)
    #[arg(long = "exclude", global = true)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of local branches
    Scan {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },

    /// Scan and interactively remove stale local branches
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Force-delete branches (git branch -D instead of -d)
        #[arg(short, long)]
        force: bool,

        /// Skip confirmation prompts (accept all defaults)
        #[arg(short = 'y', long)]
        yes: bool,

        /// Only target merged branches
        #[arg(long)]
        merged_only: bool,

        /// Target merged and fully landed branches (not partial)
        #[arg(long)]
        landed: bool,

        /// Include active and local branches in the interactive clean flow
        #[arg(long)]
        all: bool,

        /// Also delete remote tracking branches
        #[arg(long)]
        include_remote: bool,

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
        let cli = Cli::parse_from(["git-branch-tidy"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-branch-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn clean_with_flags() {
        let cli = Cli::parse_from([
            "git-branch-tidy",
            "clean",
            "--dry-run",
            "--force",
            "--yes",
            "--merged-only",
            "--include-remote",
        ]);
        match cli.command {
            Some(Command::Clean {
                dry_run,
                force,
                yes,
                merged_only,
                include_remote,
                ..
            }) => {
                assert!(dry_run);
                assert!(force);
                assert!(yes);
                assert!(merged_only);
                assert!(include_remote);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn behind_threshold_default() {
        let cli = Cli::parse_from(["git-branch-tidy"]);
        assert_eq!(cli.behind_threshold, 100);
    }

    #[test]
    fn behind_threshold_custom() {
        let cli = Cli::parse_from(["git-branch-tidy", "--behind-threshold", "50"]);
        assert_eq!(cli.behind_threshold, 50);
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-branch-tidy", "scan", "--json"]);
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
        let cli = Cli::parse_from(["git-branch-tidy", "scan", "--porcelain"]);
        match cli.command {
            Some(Command::Scan { json, porcelain }) => {
                assert!(!json);
                assert!(porcelain);
            }
            _ => panic!("expected Scan command"),
        }
    }

    #[test]
    fn clean_landed_flag() {
        let cli = Cli::parse_from(["git-branch-tidy", "clean", "--landed"]);
        match cli.command {
            Some(Command::Clean { landed, .. }) => {
                assert!(landed);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_all_flag() {
        let cli = Cli::parse_from(["git-branch-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn match_and_exclude_flags() {
        let cli = Cli::parse_from([
            "git-branch-tidy",
            "--match",
            "feature",
            "--exclude",
            "wip",
            "scan",
        ]);
        assert_eq!(cli.match_patterns, vec!["feature".to_string()]);
        assert_eq!(cli.exclude_patterns, vec!["wip".to_string()]);
    }

    #[test]
    fn match_repo_and_exclude_repo_flags() {
        let cli = Cli::parse_from([
            "git-branch-tidy",
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
