use std::io;
use std::process;

use clap::Parser;

mod cli;
mod dispatch;
mod output;
mod runner;
mod types;

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    dispatch::try_dispatch_default(&args);

    let cli = cli::Cli::parse();
    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let (json, porcelain, verbose, tools) = match &cli.command {
        Some(cli::Command::Audit {
            json,
            porcelain,
            verbose,
            tools,
        }) => (*json, *porcelain, *verbose, tools.clone()),
        None => (false, false, false, vec![]),
    };

    let tool_filter = if tools.is_empty() {
        None
    } else {
        Some(tools.as_slice())
    };

    let runner = runner::RealToolRunner;
    let result = runner::run_audit(&runner, &directory, tool_filter);

    let mut stdout = io::stdout().lock();
    let write_result = if json {
        output::write_json(&mut stdout, &result)
    } else if porcelain {
        output::write_porcelain(&mut stdout, &result)
    } else {
        output::write_human(&mut stdout, &result, verbose)
    };

    if let Err(e) = write_result {
        eprintln!("error writing output: {e}");
        process::exit(1);
    }
}
