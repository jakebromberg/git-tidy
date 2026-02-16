use std::io;
use std::process;

use clap::Parser;

mod cli;
mod discovery;
mod types;

fn main() {
    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let _stdout = io::stdout().lock();

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
