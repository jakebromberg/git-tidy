use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::cli::{OutputFormat, validate_directory};
use git_tidy_core::error;
use git_tidy_core::git;

mod clean;
mod cli;
mod discovery;
mod output;
mod scan;
mod types;

fn main() {
    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if let Err(e) = validate_directory(&directory) {
        error::exit_with_error(&e);
    }

    let git = git::RealGit;
    let mut stdout = io::stdout().lock();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let format = match &cli.command {
                Some(cli::Command::Scan { json, porcelain }) => {
                    OutputFormat::from_flags(*json, *porcelain)
                }
                _ => OutputFormat::Human,
            };

            match scan::run_scan(&git, &directory, cli.behind_threshold, cli.verbose) {
                Ok(result) => {
                    let write_result = match format {
                        OutputFormat::Json => output::write_json(&mut stdout, &result),
                        OutputFormat::Porcelain => output::write_porcelain(&mut stdout, &result),
                        OutputFormat::Human => output::write_human(&mut stdout, &result),
                    };
                    if let Err(e) = write_result {
                        eprintln!("error writing output: {e}");
                        process::exit(1);
                    }
                }
                Err(e) => error::exit_with_error(&e),
            }
        }
        Some(cli::Command::Clean {
            dry_run,
            force,
            yes,
            merged_only,
            landed,
            all,
            include_remote,
            ..
        }) => {
            // First, scan to get the current state
            match scan::run_scan(&git, &directory, cli.behind_threshold, cli.verbose) {
                Ok(scan_result) => {
                    let options = clean::CleanOptions {
                        dry_run: *dry_run,
                        force: *force,
                        yes: *yes,
                        merged_only: *merged_only,
                        landed: *landed,
                        all: *all,
                        include_remote: *include_remote,
                    };

                    match clean::run_clean(&git, &scan_result, &options, &mut stdout) {
                        Ok(result) => {
                            if !result.failed.is_empty() {
                                process::exit(1);
                            }
                        }
                        Err(e) => error::exit_with_error(&e),
                    }
                }
                Err(e) => error::exit_with_error(&e),
            }
        }
    }
}
