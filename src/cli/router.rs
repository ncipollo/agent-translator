use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::cli::run::{self, RunArgs};

#[derive(Parser)]
#[command(name = "agent-translator", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run an agent and translate or route its output.
    Run(RunArgs),
}

/// Parses command-line arguments and runs the requested command.
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime.block_on(route(cli)),
        Err(error) => {
            eprintln!("agent-translator: failed to start the async runtime: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn route(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Run(args) => run::run(args).await,
    }
}
