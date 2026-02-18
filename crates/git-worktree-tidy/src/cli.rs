use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-worktree-tidy",
    about = "Scan, classify, and interactively remove stale Git worktrees"
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

    /// Additional file patterns to treat as noise (can be repeated)
    #[arg(long = "noise-pattern", global = true)]
    pub noise_patterns: Vec<String>,

    /// Disable all default noise patterns
    #[arg(long, global = true)]
    pub no_default_noise: bool,

    /// Filter worktrees by name substring (can be repeated, OR semantics)
    #[arg(long = "match", global = true)]
    pub match_patterns: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read-only analysis of worktrees
    Scan {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,
    },

    /// Scan and interactively remove stale worktrees
    Clean {
        /// Show what would be removed without removing
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Remove worktrees with meaningful uncommitted changes
        #[arg(short, long)]
        force: bool,

        /// Skip confirmation prompts (accept all defaults)
        #[arg(short = 'y', long)]
        yes: bool,

        /// Only target merged worktrees
        #[arg(long)]
        merged_only: bool,

        /// Target merged and fully landed worktrees (not partial)
        #[arg(long)]
        landed: bool,

        /// Include active and local worktrees in the interactive clean flow
        #[arg(long)]
        all: bool,

        /// Delete local branches after removing their worktrees
        #[arg(long)]
        delete_branches: bool,

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
        let cli = Cli::parse_from(["git-worktree-tidy"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn scan_with_directory() {
        let cli = Cli::parse_from(["git-worktree-tidy", "scan", "/tmp/dev"]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
    }

    #[test]
    fn clean_with_flags() {
        let cli = Cli::parse_from([
            "git-worktree-tidy",
            "clean",
            "--dry-run",
            "--force",
            "--yes",
            "--merged-only",
            "--delete-branches",
        ]);
        match cli.command {
            Some(Command::Clean {
                dry_run,
                force,
                yes,
                merged_only,
                delete_branches,
                ..
            }) => {
                assert!(dry_run);
                assert!(force);
                assert!(yes);
                assert!(merged_only);
                assert!(delete_branches);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn behind_threshold_default() {
        let cli = Cli::parse_from(["git-worktree-tidy"]);
        assert_eq!(cli.behind_threshold, 100);
    }

    #[test]
    fn behind_threshold_custom() {
        let cli = Cli::parse_from(["git-worktree-tidy", "--behind-threshold", "50"]);
        assert_eq!(cli.behind_threshold, 50);
    }

    #[test]
    fn scan_json_flag() {
        let cli = Cli::parse_from(["git-worktree-tidy", "scan", "--json"]);
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
        let cli = Cli::parse_from(["git-worktree-tidy", "scan", "--porcelain"]);
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
        let cli = Cli::parse_from(["git-worktree-tidy", "clean", "--landed"]);
        match cli.command {
            Some(Command::Clean { landed, .. }) => {
                assert!(landed);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn clean_all_flag() {
        let cli = Cli::parse_from(["git-worktree-tidy", "clean", "--all"]);
        match cli.command {
            Some(Command::Clean { all, .. }) => {
                assert!(all);
            }
            _ => panic!("expected Clean command"),
        }
    }

    #[test]
    fn match_pattern_single() {
        let cli = Cli::parse_from(["git-worktree-tidy", "--match", "tubafrenzy"]);
        assert_eq!(cli.match_patterns, vec!["tubafrenzy".to_string()]);
    }

    #[test]
    fn match_pattern_multiple() {
        let cli = Cli::parse_from([
            "git-worktree-tidy",
            "--match",
            "tubafrenzy",
            "--match",
            "wxyc",
        ]);
        assert_eq!(
            cli.match_patterns,
            vec!["tubafrenzy".to_string(), "wxyc".to_string()]
        );
    }

    #[test]
    fn match_pattern_with_subcommand() {
        let cli = Cli::parse_from(["git-worktree-tidy", "--match", "tubafrenzy", "scan"]);
        assert_eq!(cli.match_patterns, vec!["tubafrenzy".to_string()]);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
    }

    #[test]
    fn noise_pattern_single() {
        let cli = Cli::parse_from(["git-worktree-tidy", "--noise-pattern", "*.swp"]);
        assert_eq!(cli.noise_patterns, vec!["*.swp".to_string()]);
    }

    #[test]
    fn noise_pattern_multiple() {
        let cli = Cli::parse_from([
            "git-worktree-tidy",
            "--noise-pattern",
            "*.swp",
            "--noise-pattern",
            ".envrc",
        ]);
        assert_eq!(
            cli.noise_patterns,
            vec!["*.swp".to_string(), ".envrc".to_string()]
        );
    }

    #[test]
    fn no_default_noise_flag() {
        let cli = Cli::parse_from(["git-worktree-tidy", "--no-default-noise"]);
        assert!(cli.no_default_noise);
    }

    #[test]
    fn noise_flags_with_subcommand() {
        let cli = Cli::parse_from([
            "git-worktree-tidy",
            "--noise-pattern",
            "*.swp",
            "--no-default-noise",
            "scan",
        ]);
        assert_eq!(cli.noise_patterns, vec!["*.swp".to_string()]);
        assert!(cli.no_default_noise);
        assert!(matches!(cli.command, Some(Command::Scan { .. })));
    }
}
