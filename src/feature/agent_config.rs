//! Agent configuration: the TOML schema describing how to invoke an agent and interpret its
//! output, plus the loader that resolves and validates everything up front.

mod env_config;
mod loader;

use serde::{Deserialize, Serialize};

pub use env_config::EnvConfig;
pub use loader::{LoadError, LoadedAgent, load_from_path, load_from_str};

/// How an agent's stdout is interpreted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputParsing {
    #[default]
    Raw,
    Typed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rules_file: Option<String>,
    },
}

/// One agent's TOML config. Fields holding tables (`env`, `output_parsing`) come last so the
/// `toml` crate can serialize the struct.
///
/// `cmd` is the start template (the prompt is appended as the final argument at spawn time),
/// `resume` the interactive template and `headless_resume` the streaming template for
/// follow-up turns. Templates may contain `{{variable}}` tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub cmd: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headless_resume: Option<Vec<String>>,
    /// True when the agent mints its own session id, captured from a `system_init` event
    /// rather than injected.
    #[serde(default)]
    pub parse_session_id: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<EnvConfig>,
    #[serde(default)]
    pub output_parsing: OutputParsing,
}
