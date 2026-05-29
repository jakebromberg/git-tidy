use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::cli::{OutputFormat, validate_directory};
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
        shared.run::<cli::Cli>("git-stash-tidy");
        return;
    }

    let directory = cli.target_directory();

    if let Err(e) = validate_directory(&directory) {
        error::exit_with_error(&e);
    }

    let git = gix_ops::GixGitOps;
    let progress = Progress::new();
    let repo_filter = NameFilter::new(&cli.match_repo_patterns, &cli.exclude_repo_patterns);
    let entity_filter = NameFilter::new(&cli.match_patterns, &cli.exclude_patterns);
    let mut stdout = io::stdout().lock();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let format = match &cli.command {
                Some(cli::Command::Scan { json, porcelain }) => {
                    OutputFormat::from_flags(*json, *porcelain)
                }
                _ => OutputFormat::Human,
            };

            match scan::run_scan(
                &git,
                &directory,
                cli.age_threshold,
                cli.verbose,
                &repo_filter,
                &entity_filter,
                &progress,
            ) {
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
            committed_only,
            aged_only,
            all,
            ..
        }) => match scan::run_scan(
            &git,
            &directory,
            cli.age_threshold,
            cli.verbose,
            &repo_filter,
            &entity_filter,
            &progress,
        ) {
            Ok(scan_result) => {
                let options = clean::CleanOptions {
                    dry_run: *dry_run,
                    committed_only: *committed_only,
                    aged_only: *aged_only,
                    all: *all,
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
