use std::process::ExitCode;

use agent_translator::cli::router;

fn main() -> ExitCode {
    router::run()
}
