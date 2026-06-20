use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::error;
use git_tidy_core::filter::NameFilter;
use git_tidy_core::gix_ops;
use git_tidy_core::progress::Progress;

mod cli;
mod fix;
mod lint;
mod output;
mod types;

fn main() {
    let cli = cli::Cli::parse();

    if let Some(cli::Command::Shared(shared)) = &cli.command {
        shared.run::<cli::Cli>("git-config-tidy");
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

    match &cli.command {
        None | Some(cli::Command::Lint { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(cli::Command::Lint { json, porcelain }) => (*json, *porcelain),
                _ => (false, false),
            };

            match lint::run_lint(&git, &directory, cli.verbose, &repo_filter, &progress) {
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
        Some(cli::Command::Fix {
            dry_run,
            json,
            porcelain,
        }) => match lint::run_lint(&git, &directory, cli.verbose, &repo_filter, &progress) {
            Ok(lint_result) => {
                // Show lint results first if requested
                let write_result = if *json {
                    output::write_json(&mut stdout, &lint_result)
                } else if *porcelain {
                    output::write_porcelain(&mut stdout, &lint_result)
                } else {
                    Ok(())
                };
                if let Err(e) = write_result {
                    eprintln!("error writing output: {e}");
                    process::exit(1);
                }

                let options = fix::FixOptions { dry_run: *dry_run };

                // In machine-readable modes the lint JSON / porcelain has already been written to stdout. Fix progress ("removed section X in repo") must NOT corrupt that stream; route it to stderr instead. Without this, `git-config-tidy fix --json` produced a JSON document followed by free-form text — unparseable by anything downstream.
                let fix_result = if *json || *porcelain {
                    let mut stderr = io::stderr().lock();
                    fix::run_fix(&git, &lint_result, &options, &mut stderr)
                } else {
                    fix::run_fix(&git, &lint_result, &options, &mut stdout)
                };
                match fix_result {
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
