use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(name = "agent-translator", version, about)]
struct Cli {}

/// Parses command-line arguments and runs the requested command.
pub fn run() -> ExitCode {
    Cli::parse();
    ExitCode::SUCCESS
}
