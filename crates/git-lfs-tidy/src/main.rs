use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::error;
use git_tidy_core::filter::NameFilter;
use git_tidy_core::gix_ops;
use git_tidy_core::progress::Progress;

mod clean;
mod cli;
mod output;
mod scan;
mod types;

fn main() {
    let cli = cli::Cli::parse();

    if let Some(cli::Command::Shared(shared)) = &cli.command {
        shared.run::<cli::Cli>("git-lfs-tidy");
        return;
    }

    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let git = gix_ops::GixGitOps;
    let progress = Progress::new();
    let repo_filter = NameFilter::new(
        &cli.repo_filter.match_repo_patterns,
        &cli.repo_filter.exclude_repo_patterns,
    );
    let mut stdout = io::stdout().lock();

    // Extract scan parameters from whichever subcommand is active.
    let (size_threshold_str, depth) = match &cli.command {
        Some(cli::Command::Scan {
            size_threshold,
            depth,
            ..
        })
        | Some(cli::Command::Clean {
            size_threshold,
            depth,
            ..
        }) => (size_threshold.as_str(), *depth),
        None | Some(cli::Command::Shared(_)) => ("1MB", 1000),
    };

    let size_threshold = match scan::parse_size(size_threshold_str) {
        Some(t) => t,
        None => {
            eprintln!("error: invalid size threshold: {size_threshold_str}");
            process::exit(1);
        }
    };

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(cli::Command::Scan {
                    json, porcelain, ..
                }) => (*json, *porcelain),
                _ => (false, false),
            };

            match scan::run_scan(
                &git,
                &directory,
                size_threshold,
                depth,
                cli.verbose,
                &repo_filter,
                &progress,
            ) {
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
                Err(e) => error::exit_with_error(&e),
            }
        }
        Some(cli::Command::Clean { dry_run, prune, .. }) => match scan::run_scan(
            &git,
            &directory,
            size_threshold,
            depth,
            cli.verbose,
            &repo_filter,
            &progress,
        ) {
            Ok(scan_result) => {
                let options = clean::CleanOptions {
                    dry_run: *dry_run,
                    prune: *prune,
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
        },
        Some(cli::Command::Shared(_)) => unreachable!("handled above"),
    }
}
