use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "git-tidy",
    about = "Audit runner for git-tidy tools",
    after_help = "\
Tool commands (dispatch to individual tools):
  worktrees    git-worktree-tidy      branches     git-branch-tidy
  stashes      git-stash-tidy         remotes      git-remote-tidy
  tags         git-tag-tidy           repos        git-repo-tidy
  config       git-config-tidy        lfs          git-lfs-tidy

Singular forms also accepted (worktree, branch, stash, etc.).

Examples:
  git tidy worktrees scan --json ~/Developer
  git tidy branches clean --yes
  git tidy config lint"
)]
pub struct Cli {
    /// Directory to scan (default: current directory)
    #[arg(global = true)]
    pub directory: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run audit across all installed git-tidy tools (default)
    Audit {
        /// Output results as JSON
        #[arg(long)]
        json: bool,

        /// Machine-readable tab-delimited output
        #[arg(long)]
        porcelain: bool,

        /// Show verbose output (tool paths, timing)
        #[arg(long, short)]
        verbose: bool,

        /// Comma-separated list of tools to run (e.g., "branch,tag" or "git-branch-tidy")
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// Use subprocess mode (shell out to each tool binary instead of calling in-process)
        #[arg(long)]
        subprocess: bool,
    },
}

impl Cli {
    /// Resolve the target directory, defaulting to the current directory.
    pub fn target_directory(&self) -> PathBuf {
        self.directory
            .clone()
            .unwrap_or_else(|| std::env::current_dir().expect("cannot determine current directory"))
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn default_command_is_none() {
        let cli = Cli::parse_from(["git-tidy"]);
        assert!(cli.command.is_none());
        assert!(cli.directory.is_none());
    }

    #[test]
    fn audit_subcommand_explicit() {
        let cli = Cli::parse_from(["git-tidy", "audit"]);
        assert!(matches!(cli.command, Some(Command::Audit { .. })));
    }

    #[test]
    fn audit_with_json_flag() {
        let cli = Cli::parse_from(["git-tidy", "audit", "--json"]);
        match cli.command {
            Some(Command::Audit {
                json,
                porcelain,
                verbose,
                subprocess,
                ..
            }) => {
                assert!(json);
                assert!(!porcelain);
                assert!(!verbose);
                assert!(!subprocess);
            }
            _ => panic!("expected Audit command"),
        }
    }

    #[test]
    fn audit_with_subprocess_flag() {
        let cli = Cli::parse_from(["git-tidy", "audit", "--subprocess"]);
        match cli.command {
            Some(Command::Audit { subprocess, .. }) => assert!(subprocess),
            _ => panic!("expected Audit command"),
        }
    }

    #[test]
    fn audit_with_porcelain_flag() {
        let cli = Cli::parse_from(["git-tidy", "audit", "--porcelain"]);
        match cli.command {
            Some(Command::Audit { porcelain, .. }) => assert!(porcelain),
            _ => panic!("expected Audit command"),
        }
    }

    #[test]
    fn audit_with_verbose_flag() {
        let cli = Cli::parse_from(["git-tidy", "audit", "-v"]);
        match cli.command {
            Some(Command::Audit { verbose, .. }) => assert!(verbose),
            _ => panic!("expected Audit command"),
        }
    }

    #[test]
    fn audit_with_tools_filter() {
        let cli = Cli::parse_from(["git-tidy", "audit", "--tools", "branch,tag"]);
        match cli.command {
            Some(Command::Audit { tools, .. }) => {
                assert_eq!(tools, vec!["branch", "tag"]);
            }
            _ => panic!("expected Audit command"),
        }
    }

    #[test]
    fn directory_argument() {
        let cli = Cli::parse_from(["git-tidy", "/tmp/dev"]);
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn directory_with_audit_subcommand() {
        let cli = Cli::parse_from(["git-tidy", "audit", "--json", "/tmp/dev"]);
        assert_eq!(cli.directory, Some(PathBuf::from("/tmp/dev")));
        assert!(matches!(cli.command, Some(Command::Audit { .. })));
    }
}
