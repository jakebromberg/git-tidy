use std::process;

use clap::Parser;

mod classification;
mod clean;
mod cli;
mod dirty;
mod discovery;
mod error;
mod git;
mod landed;
mod output;
mod types;

fn main() {
    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    // TODO: wire up scan/clean commands in later PRs
    match &cli.command {
        None | Some(cli::Command::Scan { .. }) => {
            eprintln!("scan command not yet implemented");
            process::exit(1);
        }
        Some(cli::Command::Clean { .. }) => {
            eprintln!("clean command not yet implemented");
            process::exit(1);
        }
    }
}
