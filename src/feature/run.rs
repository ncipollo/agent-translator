//! Runs one agent end to end: load its config, render the command, spawn it, and route its
//! output. The `run` command is a thin wrapper over [`execute`].

use std::collections::HashMap;
use std::fmt;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};
use std::process::ExitStatus;

use uuid::Uuid;

use crate::feature::agent_config::{LoadError, LoadedAgent, load_from_path};
use crate::feature::defaults::{self, DefaultsError};
use crate::feature::output::{OutputError, OutputRoutes, RoutesError, route};
use crate::feature::process::{CommandMode, ProcessError, SpawnOptions, resolve_command, spawn};

pub struct RunRequest {
    /// A bundled agent name or the path to an agent TOML file.
    pub agent: String,
    pub cwd: Option<PathBuf>,
    pub vars: HashMap<String, String>,
    /// Generated (UUID v4) when absent.
    pub session_id: Option<String>,
    /// Use the headless resume template with `session_id`.
    pub resume: bool,
    pub routes: OutputRoutes,
    pub prompt: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RunOutcome {
    /// The child's exit code; `128 + signal` when it was killed by a signal.
    pub exit_code: i32,
    /// The session id captured from the agent's output, only when its config sets
    /// `parse_session_id`.
    pub captured_session_id: Option<String>,
}

#[derive(Debug)]
pub enum RunError {
    Defaults(DefaultsError),
    Load(LoadError),
    Routes(RoutesError),
    Process(ProcessError),
    Output(OutputError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Defaults(source) => source.fmt(f),
            RunError::Load(source) => source.fmt(f),
            RunError::Routes(source) => source.fmt(f),
            RunError::Process(source) => source.fmt(f),
            RunError::Output(source) => source.fmt(f),
        }
    }
}

impl std::error::Error for RunError {}

pub async fn execute(request: RunRequest) -> Result<RunOutcome, RunError> {
    request.routes.validate().map_err(RunError::Routes)?;
    let agent = load_agent(&request.agent)?;
    let session_id = request
        .session_id
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let mode = if request.resume {
        CommandMode::HeadlessResume
    } else {
        CommandMode::Start
    };
    let command = resolve_command(&agent.config, mode, &session_id, &request.vars)
        .map_err(RunError::Process)?;
    let none = HashMap::new();
    let mut child = spawn(&SpawnOptions {
        cmd: &command.argv,
        prompt: &request.prompt,
        working_dir: request.cwd.as_deref(),
        env_config: agent.config.env.as_ref(),
        extra_env: &none,
        forced_env: &none,
    })
    .map_err(RunError::Process)?;
    let summary = route(
        &mut child,
        &request.routes,
        &agent.config.output_parsing,
        agent.rules,
    )
    .await
    .map_err(RunError::Output)?;
    Ok(RunOutcome {
        exit_code: exit_code(summary.exit_status),
        captured_session_id: summary.session_id.filter(|_| agent.config.parse_session_id),
    })
}

/// A bundled name unless the argument looks like a path (has a separator or ends in `.toml`).
fn load_agent(agent: &str) -> Result<LoadedAgent, RunError> {
    if looks_like_path(agent) {
        load_from_path(Path::new(agent)).map_err(RunError::Load)
    } else {
        defaults::load(agent).map_err(RunError::Defaults)
    }
}

fn looks_like_path(agent: &str) -> bool {
    agent.contains(MAIN_SEPARATOR) || agent.ends_with(".toml")
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| signal_exit_code(status))
}

#[cfg(unix)]
fn signal_exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    128 + status.signal().unwrap_or(0)
}

#[cfg(not(unix))]
fn signal_exit_code(_status: ExitStatus) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_toml_files_are_not_bundled_names() {
        assert!(looks_like_path("agents/mine.toml"));
        assert!(looks_like_path("mine.toml"));
        assert!(looks_like_path("/abs/agent"));
        assert!(!looks_like_path("claude"));
    }

    #[cfg(unix)]
    #[test]
    fn signal_deaths_map_to_128_plus_signal() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(exit_code(ExitStatus::from_raw(9)), 137);
        assert_eq!(exit_code(ExitStatus::from_raw(3 << 8)), 3);
    }

    #[tokio::test]
    async fn unknown_bundled_name_lists_the_available_agents() {
        let request = RunRequest {
            agent: "nope".into(),
            cwd: None,
            vars: HashMap::new(),
            session_id: None,
            resume: false,
            routes: OutputRoutes::default(),
            prompt: "hi".into(),
        };
        let message = execute(request).await.unwrap_err().to_string();
        assert!(
            message.contains("nope") && message.contains("claude"),
            "{message}"
        );
    }

    #[tokio::test]
    async fn missing_config_file_names_the_path() {
        let request = RunRequest {
            agent: "/no/such/agent.toml".into(),
            cwd: None,
            vars: HashMap::new(),
            session_id: None,
            resume: false,
            routes: OutputRoutes::default(),
            prompt: "hi".into(),
        };
        let message = execute(request).await.unwrap_err().to_string();
        assert!(message.contains("/no/such/agent.toml"), "{message}");
    }
}
