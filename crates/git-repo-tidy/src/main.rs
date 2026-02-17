use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::config::{NoiseConfig, default_config_path, load_config_file};
use git_tidy_core::error;
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

    // Resolve noise patterns
    let (config_extra, config_exclude) = default_config_path()
        .map(|p| load_config_file(&p))
        .unwrap_or_default();

    let noise_config = NoiseConfig {
        config_extra,
        config_exclude,
        cli_extra: cli.noise_patterns.clone(),
        no_defaults: cli.no_default_noise,
    };
    let noise_patterns = noise_config.resolve();

    let stale_days = cli.stale_threshold_days();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(cli::Command::Scan { json, porcelain }) => (*json, *porcelain),
                _ => (false, false),
            };

            match scan::run_scan(&git, &directory, stale_days, &noise_patterns, cli.offline) {
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
        Some(cli::Command::Clean {
            dry_run,
            yes,
            force,
            stale_only,
            orphaned_only,
            all,
            ..
        }) => match scan::run_scan(&git, &directory, stale_days, &noise_patterns, cli.offline) {
            Ok(scan_result) => {
                let options = clean::CleanOptions {
                    dry_run: *dry_run,
                    yes: *yes,
                    force: *force,
                    stale_only: *stale_only,
                    orphaned_only: *orphaned_only,
                    all: *all,
                };

                let delete_fn = |path: &std::path::Path| std::fs::remove_dir_all(path);

                match clean::run_clean(&scan_result, &options, &delete_fn, &mut stdout) {
                    Ok(result) => {
                        if result.dirty_blocked {
                            process::exit(2);
                        }
                        if !result.failed.is_empty() {
                            process::exit(1);
                        }
                    }
                    Err(e) => error::exit_with_error(&e),
                }
            }
            Err(e) => error::exit_with_error(&e),
        },
    }
}
