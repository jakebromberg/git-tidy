use std::io::{self, IsTerminal};
use std::process;

use clap::Parser;

use git_worktree_tidy::{clean, git, output, scan};

fn main() {
    let cli = git_worktree_tidy::cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let git = git::RealGit;
    let mut stdout = io::stdout().lock();

    match &cli.command {
        None | Some(git_worktree_tidy::cli::Command::Scan { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(git_worktree_tidy::cli::Command::Scan { json, porcelain }) => {
                    (*json, *porcelain)
                }
                _ => (false, false),
            };

            match scan::run_scan(&git, &directory, cli.behind_threshold, cli.verbose) {
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
        Some(git_worktree_tidy::cli::Command::Clean {
            dry_run,
            force,
            yes,
            merged_only,
            landed,
            all,
            delete_branches,
            json,
            porcelain,
        }) => {
            match scan::run_scan(&git, &directory, cli.behind_threshold, cli.verbose) {
                Ok(scan_result) => {
                    // Optionally output scan results first
                    if *json {
                        if let Err(e) = output::write_json(&mut stdout, &scan_result) {
                            eprintln!("error writing output: {e}");
                            process::exit(1);
                        }
                    } else if *porcelain {
                        if let Err(e) = output::write_porcelain(&mut stdout, &scan_result) {
                            eprintln!("error writing output: {e}");
                            process::exit(1);
                        }
                    }

                    let interactive = io::stdin().is_terminal();
                    let opts = clean::CleanOptions {
                        dry_run: *dry_run,
                        force: *force,
                        yes: *yes,
                        merged_only: *merged_only,
                        landed: *landed,
                        all: *all,
                        delete_branches: *delete_branches,
                    };

                    match clean::run_clean(
                        &git,
                        &scan_result,
                        &opts,
                        &mut stdout,
                        interactive && !*yes,
                    ) {
                        Ok(result) => {
                            if result.dirty_blocked {
                                process::exit(2);
                            }
                            if result.failed > 0 {
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
            }
        }
    }
}
