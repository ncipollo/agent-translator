use std::collections::HashMap;

use uuid::Uuid;

use crate::feature::agent_config::AgentConfig;
use crate::feature::process::{ProcessError, render_cmd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandMode {
    /// First turn: the `cmd` template with the caller's session id.
    Start,
    /// Interactive hand-off: the `resume` template. Returned as argv for the caller to display
    /// or exec; never spawned by this layer.
    Resume,
    /// Follow-up turn with streaming output: `headless_resume`, or a fresh `cmd` run when the
    /// agent has none.
    HeadlessResume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCommand {
    /// The rendered command, without the prompt (the spawner appends it as the final argument).
    pub argv: Vec<String>,
    /// The session id the command was rendered with. It differs from the requested one when a
    /// headless resume falls back to `cmd`.
    pub session_id: String,
}

/// Picks and renders the template for `mode`. `vars` supplies extra `{{variable}}` values;
/// `session_id` is always provided.
pub fn resolve_command(
    config: &AgentConfig,
    mode: CommandMode,
    session_id: &str,
    vars: &HashMap<String, String>,
) -> Result<ResolvedCommand, ProcessError> {
    resolve_with(config, mode, session_id, vars, || {
        Uuid::new_v4().to_string()
    })
}

fn resolve_with(
    config: &AgentConfig,
    mode: CommandMode,
    session_id: &str,
    vars: &HashMap<String, String>,
    new_session_id: impl FnOnce() -> String,
) -> Result<ResolvedCommand, ProcessError> {
    let (template, session_id) = match mode {
        CommandMode::Start => (&config.cmd, session_id.to_string()),
        CommandMode::Resume => {
            let template = config
                .resume
                .as_ref()
                .ok_or(ProcessError::MissingResumeTemplate)?;
            (template, session_id.to_string())
        }
        CommandMode::HeadlessResume => match &config.headless_resume {
            Some(template) => (template, session_id.to_string()),
            None => (&config.cmd, new_session_id()),
        },
    };
    let mut vars = vars.clone();
    vars.insert("session_id".into(), session_id.clone());
    Ok(ResolvedCommand {
        argv: render_cmd(template, &vars),
        session_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(headless: bool) -> AgentConfig {
        let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        AgentConfig {
            cmd: strings(&[
                "agent",
                "--session-id",
                "{{session_id}}",
                "--tag",
                "{{tag}}",
            ]),
            resume: Some(strings(&["agent", "--resume", "{{session_id}}"])),
            headless_resume: headless
                .then(|| strings(&["agent", "-p", "--resume", "{{session_id}}"])),
            parse_session_id: false,
            env: None,
            output_parsing: Default::default(),
        }
    }

    fn vars() -> HashMap<String, String> {
        HashMap::from([("tag".to_string(), "t1".to_string())])
    }

    #[test]
    fn start_uses_cmd_with_the_callers_session_id_and_vars() {
        let got = resolve_command(&config(true), CommandMode::Start, "s1", &vars()).unwrap();
        assert_eq!(got.argv, ["agent", "--session-id", "s1", "--tag", "t1"]);
        assert_eq!(got.session_id, "s1");
    }

    #[test]
    fn headless_resume_uses_its_template_with_the_existing_session() {
        let got =
            resolve_command(&config(true), CommandMode::HeadlessResume, "s1", &vars()).unwrap();
        assert_eq!(got.argv, ["agent", "-p", "--resume", "s1"]);
        assert_eq!(got.session_id, "s1");
    }

    #[test]
    fn headless_resume_without_a_template_falls_back_to_cmd_with_a_new_session_id() {
        let got = resolve_with(
            &config(false),
            CommandMode::HeadlessResume,
            "old",
            &vars(),
            || "fresh".to_string(),
        )
        .unwrap();
        assert_eq!(got.argv, ["agent", "--session-id", "fresh", "--tag", "t1"]);
        assert_eq!(got.session_id, "fresh");
    }

    #[test]
    fn fallback_generates_a_real_uuid() {
        let got =
            resolve_command(&config(false), CommandMode::HeadlessResume, "old", &vars()).unwrap();
        assert_ne!(got.session_id, "old");
        assert!(Uuid::parse_str(&got.session_id).is_ok());
    }

    #[test]
    fn resume_renders_the_interactive_template() {
        let got = resolve_command(&config(false), CommandMode::Resume, "s1", &vars()).unwrap();
        assert_eq!(got.argv, ["agent", "--resume", "s1"]);
    }

    #[test]
    fn resume_without_a_template_is_an_error() {
        let mut cfg = config(false);
        cfg.resume = None;
        let err = resolve_command(&cfg, CommandMode::Resume, "s1", &vars()).unwrap_err();
        assert!(matches!(err, ProcessError::MissingResumeTemplate));
    }
}
