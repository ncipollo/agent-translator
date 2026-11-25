//! Default agent configs and parsing rules, embedded in the binary. They load through the
//! normal loader pipeline; only the source of the rules text differs.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::feature::agent_config::{LoadError, LoadedAgent, load_with, parse_validated_rules};

macro_rules! agent {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("../../defaults/agents/", $name, ".toml")),
        )
    };
}

macro_rules! parsing {
    ($name:literal) => {
        (
            concat!("parsing/", $name, ".json"),
            include_str!(concat!("../../defaults/agents/parsing/", $name, ".json")),
        )
    };
}

const CONFIGS: &[(&str, &str)] = &[
    agent!("claude"),
    agent!("claude-opus"),
    agent!("claude-haiku"),
    agent!("claude-fable"),
    agent!("codex"),
    agent!("cortex"),
    agent!("opencode"),
    agent!("cursor"),
    agent!("pi"),
];

const PARSING: &[(&str, &str)] = &[
    parsing!("claude"),
    parsing!("codex"),
    parsing!("cortex"),
    parsing!("opencode"),
    parsing!("cursor"),
    parsing!("pi"),
];

#[derive(Debug)]
pub enum DefaultsError {
    UnknownAgent { name: String },
    Load(LoadError),
}

impl fmt::Display for DefaultsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefaultsError::UnknownAgent { name } => write!(
                f,
                "unknown default agent '{name}'; available: {}",
                names().join(", ")
            ),
            DefaultsError::Load(source) => source.fmt(f),
        }
    }
}

impl std::error::Error for DefaultsError {}

/// Names of the bundled agents.
pub fn names() -> Vec<&'static str> {
    CONFIGS.iter().map(|(name, _)| *name).collect()
}

/// Loads a bundled agent by name.
pub fn load(name: &str) -> Result<LoadedAgent, DefaultsError> {
    let (_, contents) = CONFIGS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .ok_or_else(|| DefaultsError::UnknownAgent { name: name.into() })?;
    load_with(contents, &format!("default '{name}'"), &embedded_rules).map_err(DefaultsError::Load)
}

fn embedded_rules(rules_file: &str) -> Result<Vec<crate::feature::rules::MapRule>, LoadError> {
    let Some((_, raw)) = PARSING.iter().find(|(path, _)| *path == rules_file) else {
        return Err(LoadError::InvalidRules {
            path: rules_file.into(),
            message: "no bundled rules file with that name".into(),
        });
    };
    parse_validated_rules(raw, rules_file)
}

/// Writes every bundled config and parsing file under `dir` so users can customize them.
pub fn write_all(dir: &Path) -> io::Result<()> {
    for (name, contents) in CONFIGS {
        write_file(&dir.join(format!("{name}.toml")), contents)?;
    }
    for (path, contents) in PARSING {
        write_file(&dir.join(path), contents)?;
    }
    Ok(())
}

fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::agent_config::{AgentConfig, load_from_path};
    use tempfile::TempDir;

    #[test]
    fn every_default_loads_with_rules() {
        for name in names() {
            let loaded = load(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!loaded.rules.is_empty(), "{name} has no rules");
        }
    }

    #[test]
    fn unknown_name_lists_available_agents() {
        let message = load("nope").unwrap_err().to_string();
        assert!(message.contains("nope"), "{message}");
        for name in names() {
            assert!(message.contains(name), "{message}");
        }
    }

    #[test]
    fn write_all_produces_loadable_files() {
        let dir = TempDir::new().unwrap();
        write_all(dir.path()).unwrap();
        for name in names() {
            let from_disk = load_from_path(&dir.path().join(format!("{name}.toml"))).unwrap();
            assert_eq!(from_disk, load(name).unwrap(), "{name}");
        }
    }

    #[test]
    fn serialized_configs_reload_identically() {
        for name in names() {
            let config = load(name).unwrap().config;
            let serialized = toml::to_string(&config).unwrap();
            assert_eq!(serialized, toml::to_string(&config).unwrap());
            assert_eq!(
                toml::from_str::<AgentConfig>(&serialized).unwrap(),
                config,
                "{name}"
            );
        }
    }

    #[test]
    fn cortex_keeps_prompt_flag_last() {
        let cmd = load("cortex").unwrap().config.cmd;
        assert_eq!(cmd.last().map(String::as_str), Some("-p"));
    }

    #[test]
    fn session_id_parsing_matches_the_agent_table() {
        for name in ["codex", "opencode", "cursor"] {
            assert!(load(name).unwrap().config.parse_session_id, "{name}");
        }
        for name in ["claude", "claude-opus", "cortex", "pi"] {
            assert!(!load(name).unwrap().config.parse_session_id, "{name}");
        }
    }
}
