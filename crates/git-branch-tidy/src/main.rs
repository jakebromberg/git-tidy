use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::git;

mod cli;
mod discovery;
mod scan;
mod types;

fn main() {
    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let git = git::RealGit;
    let _stdout = io::stdout().lock();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            match scan::run_scan(&git, &directory, cli.behind_threshold, cli.verbose) {
                Ok(result) => {
                    // Output formatting will be added in PR 5
                    eprintln!(
                        "{} branches scanned across {} repos",
                        result.total_scanned,
                        result.repos.len()
                    );
                    for warning in &result.warnings {
                        eprintln!("warning: {warning}");
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(e.exit_code());
                }
            }
        }
        Some(cli::Command::Clean { .. }) => {
            eprintln!("clean command not yet implemented");
            process::exit(1);
        }
    }
}
