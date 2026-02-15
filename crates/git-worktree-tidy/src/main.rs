use std::io;
use std::process;

use clap::Parser;
use git_tidy_core::classification;
use git_tidy_core::config;
use git_tidy_core::error;
use git_tidy_core::git;
use git_tidy_core::types::{RepoGroup, ScanCounts, ScanResult};

mod clean;
mod cli;
mod discovery;
mod output;

fn main() {
    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    // Load config file and resolve noise patterns
    let (config_extra, config_exclude) = config::default_config_path()
        .map(|p| config::load_config_file(&p))
        .unwrap_or_default();
    let noise_config = config::NoiseConfig {
        config_extra,
        config_exclude,
        cli_extra: cli.noise_patterns.clone(),
        no_defaults: cli.no_default_noise,
    };
    let noise_patterns = noise_config.resolve();

    let git = git::RealGit;
    let mut stdout = io::stdout().lock();

    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            let (json, porcelain) = match &cli.command {
                Some(cli::Command::Scan { json, porcelain }) => (*json, *porcelain),
                _ => (false, false),
            };

            match run_scan(
                &git,
                &directory,
                cli.behind_threshold,
                cli.verbose,
                &noise_patterns,
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

fn run_scan(
    git: &dyn git::GitOps,
    directory: &std::path::Path,
    behind_threshold: usize,
    verbose: bool,
    noise_patterns: &[String],
) -> Result<ScanResult, error::Error> {
    let groups = discovery::discover_worktrees(directory)?;

    let mut repos = Vec::new();
    let mut counts = ScanCounts::default();
    let mut warnings = Vec::new();
    let mut total_scanned = 0;

    for (repo_path, worktrees) in &groups {
        // Fetch to get current remote state
        if let Err(e) = git.fetch_prune(repo_path) {
            warnings.push(format!("fetch failed for {}: {e}", repo_path.display()));
        }

        // Detect default branch
        let default_branch = match classification::detect_default_branch(git, repo_path) {
            Ok(b) => b,
            Err(_) => {
                warnings.push(format!(
                    "could not determine default branch for {} -- skipping",
                    repo_path.display()
                ));
                continue;
            }
        };

        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| repo_path.display().to_string());

        let mut classified = Vec::new();
        for wt in worktrees {
            match classification::classify_worktree(
                git,
                &wt.path,
                repo_path,
                &default_branch,
                behind_threshold,
                verbose,
                noise_patterns,
            ) {
                Ok(info) => {
                    counts.increment(&info.classification);
                    total_scanned += 1;
                    classified.push(info);
                }
                Err(e) => {
                    warnings.push(format!("error classifying {}: {e}", wt.path.display()));
                }
            }
        }

        // Sort by classification priority
        classified.sort_by_key(|wt| wt.classification.priority());

        if !classified.is_empty() {
            repos.push(RepoGroup {
                repo_path: repo_path.clone(),
                name: repo_name,
                worktrees: classified,
            });
        }
    }

    Ok(ScanResult {
        repos,
        total_scanned,
        counts,
        warnings,
    })
}
