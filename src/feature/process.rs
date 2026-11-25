//! Process layer: renders command templates, applies environment filtering, and spawns agent
//! CLIs with their output piped back for routing.

mod command;
mod env;
mod spawn;
mod template;

use std::fmt;

pub use command::{CommandMode, ResolvedCommand, resolve_command};
pub use env::resolve_env;
pub use spawn::{SpawnOptions, spawn};
#[cfg(unix)]
pub use spawn::{process_exists, terminate};
pub use template::{render, render_cmd};

#[derive(Debug)]
pub enum ProcessError {
    EmptyCmd,
    MissingResumeTemplate,
    Spawn(std::io::Error),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessError::EmptyCmd => write!(f, "agent command is empty"),
            ProcessError::MissingResumeTemplate => {
                write!(f, "agent config has no `resume` command template")
            }
            ProcessError::Spawn(source) => write!(f, "failed to spawn agent process: {source}"),
        }
    }
}

impl std::error::Error for ProcessError {}
