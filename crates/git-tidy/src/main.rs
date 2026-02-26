use std::io;
use std::process;

use clap::Parser;
use clap_complete::Shell;
use git_tidy_core::cli::SharedCommands;
use git_tidy_core::progress::Progress;

mod cli;
mod completions;
mod dispatch;
mod explain;
mod inprocess;
mod output;
mod runner;
mod types;

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    dispatch::try_dispatch_default(&args);

    let cli = cli::Cli::parse();

    if let Some(cli::Command::Shared(shared)) = &cli.command {
        match shared {
            SharedCommands::Completions { shell } if *shell == Shell::Zsh => {
                completions::generate_zsh(&mut io::stdout()).unwrap();
            }
            _ => shared.run::<cli::Cli>("git-tidy"),
        }
        return;
    }

    if let Some(cli::Command::Explain { term }) = &cli.command {
        let mut stdout = io::stdout().lock();
        if let Some(t) = term {
            explain::write_term(&mut stdout, t).unwrap();
        } else {
            explain::write_full(&mut stdout).unwrap();
        }
        return;
    }

    let directory = cli.target_directory();

    if !directory.is_dir() {
        eprintln!("error: directory not found: {}", directory.display());
        process::exit(1);
    }

    let (json, porcelain, verbose, tools, subprocess) = match &cli.command {
        Some(cli::Command::Audit {
            json,
            porcelain,
            verbose,
            tools,
            subprocess,
        }) => (*json, *porcelain, *verbose, tools.clone(), *subprocess),
        None => (false, false, false, vec![], false),
        Some(cli::Command::Shared(_)) | Some(cli::Command::Explain { .. }) => {
            unreachable!("handled above")
        }
    };

    let tool_filter = if tools.is_empty() {
        None
    } else {
        Some(tools.as_slice())
    };

    let progress = Progress::new();
    let result = if subprocess {
        let runner = runner::RealToolRunner;
        runner::run_audit(&runner, &directory, tool_filter, verbose, &progress)
    } else {
        inprocess::run_audit_inprocess(&directory, tool_filter, verbose, &progress)
    };

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
