use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::git;

mod clean;
mod cli;
mod output;
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
    let mut stdout = io::stdout().lock();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(cli::Command::Scan { json, porcelain }) => (*json, *porcelain),
                _ => (false, false),
            };

            match scan::run_scan(&git, &directory, cli.offline) {
                Ok(result) => {
                    let write_result = if json {
                        output::write_json(&mut stdout, &result)
                    } else if porcelain {
                        output::write_porcelain(&mut stdout, &result)
                    } else {
                        output::write_human(&mut stdout, &result)
                    };
                    if let Err(e) = write_result {
                        eprintln!("error writing output: {e}");
                        process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(e.exit_code());
                }
            }
        }
        Some(cli::Command::Clean {
            dry_run,
            yes,
            force,
            all,
            ..
        }) => match scan::run_scan(&git, &directory, cli.offline) {
            Ok(scan_result) => {
                let options = clean::CleanOptions {
                    dry_run: *dry_run,
                    yes: *yes,
                    force: *force,
                    all: *all,
                };

                match clean::run_clean(&git, &scan_result, &options, &mut stdout) {
                    Ok(result) => {
                        if !result.failed.is_empty() {
                            process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(e.exit_code());
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(e.exit_code());
            }
        },
    }
}
