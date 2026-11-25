use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::feature::agent_config::{AgentConfig, OutputParsing};
use crate::feature::rules::{MapRule, RulesFileError, parse_rules_file, validate_rules};

/// An agent config together with its resolved, validated parsing rules. Rules are empty for
/// `raw` parsing or when `rules_file` is absent.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedAgent {
    pub config: AgentConfig,
    pub rules: Vec<MapRule>,
}

#[derive(Debug)]
pub enum LoadError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        origin: String,
        source: toml::de::Error,
    },
    EmptyCmd {
        origin: String,
    },
    Rules(RulesFileError),
    InvalidRules {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read { path, source } => {
                write!(
                    f,
                    "failed to read agent config '{}': {source}",
                    path.display()
                )
            }
            LoadError::Parse { origin, source } => {
                write!(f, "failed to parse agent config {origin}: {source}")
            }
            LoadError::EmptyCmd { origin } => write!(f, "agent config {origin} has an empty cmd"),
            LoadError::Rules(source) => source.fmt(f),
            LoadError::InvalidRules { path, message } => {
                write!(f, "invalid rules in '{}': {message}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Resolves a config's `rules_file` into validated rules.
pub type RulesResolver<'a> = &'a dyn Fn(&str) -> Result<Vec<MapRule>, LoadError>;

/// Reads the TOML at `path`; `rules_file` resolves relative to the file's directory.
pub fn load_from_path(path: &Path) -> Result<LoadedAgent, LoadError> {
    let contents = fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    load_with(
        &contents,
        &format!("'{}'", path.display()),
        &disk_rules(base_dir),
    )
}

/// Loads an embedded config; `rules_file` resolves relative to `base_dir`.
pub fn load_from_str(contents: &str, base_dir: &Path) -> Result<LoadedAgent, LoadError> {
    load_with(contents, "<inline>", &disk_rules(base_dir))
}

/// Shared loading pipeline: parse the TOML, check it, then resolve rules through `resolve_rules`
/// (only called for typed parsing with a `rules_file`). `origin` labels errors.
pub fn load_with(
    contents: &str,
    origin: &str,
    resolve_rules: RulesResolver,
) -> Result<LoadedAgent, LoadError> {
    let config: AgentConfig = toml::from_str(contents).map_err(|source| LoadError::Parse {
        origin: origin.to_string(),
        source,
    })?;
    if config.cmd.is_empty() {
        return Err(LoadError::EmptyCmd {
            origin: origin.to_string(),
        });
    }
    let rules = match &config.output_parsing {
        OutputParsing::Typed {
            rules_file: Some(rules_file),
        } => resolve_rules(rules_file)?,
        _ => Vec::new(),
    };
    Ok(LoadedAgent { config, rules })
}

/// Parses and validates rules text, reporting problems against `path`.
pub fn parse_validated_rules(raw: &str, path: &str) -> Result<Vec<MapRule>, LoadError> {
    let invalid = |message: String| LoadError::InvalidRules {
        path: PathBuf::from(path),
        message,
    };
    let rules: Vec<MapRule> = serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
    validate_rules(&rules).map_err(invalid)?;
    Ok(rules)
}

fn disk_rules(base_dir: &Path) -> impl Fn(&str) -> Result<Vec<MapRule>, LoadError> {
    let base_dir = base_dir.to_path_buf();
    move |rules_file| {
        let path = base_dir.join(rules_file);
        let rules = parse_rules_file(&path).map_err(LoadError::Rules)?;
        validate_rules(&rules).map_err(|message| LoadError::InvalidRules { path, message })?;
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const RULES_JSON: &str = r#"[{"name":"t","when":[{"op":"eq","path":"type","value":"text"}],
        "emit":{"event":"text","fields":{"text":{"path":"body"}}}}]"#;

    fn write(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn minimal_cmd_loads_raw_with_no_rules() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "agent.toml", r#"cmd = ["echo", "hi"]"#);
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.config.cmd, ["echo", "hi"]);
        assert_eq!(loaded.config.output_parsing, OutputParsing::Raw);
        assert!(!loaded.config.parse_session_id);
        assert!(loaded.rules.is_empty());
    }

    #[test]
    fn typed_config_loads_and_validates_json_rules() {
        let dir = TempDir::new().unwrap();
        write(&dir, "parsing/a.json", RULES_JSON);
        let path = write(
            &dir,
            "agent.toml",
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\nrules_file = \"parsing/a.json\"\n",
        );
        assert_eq!(load_from_path(&path).unwrap().rules.len(), 1);
    }

    #[test]
    fn typed_config_loads_toml_rules() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "rules.toml",
            "[[rules]]\nname = \"t\"\n[rules.emit]\nevent = \"thinking\"\n[rules.emit.fields.text]\npath = \"x\"\n",
        );
        let path = write(
            &dir,
            "agent.toml",
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\nrules_file = \"rules.toml\"\n",
        );
        assert_eq!(load_from_path(&path).unwrap().rules.len(), 1);
    }

    #[test]
    fn typed_without_rules_file_has_no_rules() {
        let loaded = load_from_str(
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\n",
            Path::new("."),
        )
        .unwrap();
        assert!(loaded.rules.is_empty());
    }

    #[test]
    fn missing_rules_file_error_names_the_path() {
        let dir = TempDir::new().unwrap();
        let path = write(
            &dir,
            "agent.toml",
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\nrules_file = \"nope.json\"\n",
        );
        let err = load_from_path(&path).unwrap_err().to_string();
        assert!(err.contains("nope.json"), "{err}");
    }

    #[test]
    fn malformed_rules_file_error_names_the_path() {
        let dir = TempDir::new().unwrap();
        write(&dir, "bad.json", "not json");
        let path = write(
            &dir,
            "agent.toml",
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\nrules_file = \"bad.json\"\n",
        );
        let err = load_from_path(&path).unwrap_err().to_string();
        assert!(err.contains("bad.json"), "{err}");
    }

    #[test]
    fn invalid_rules_error_names_the_path_and_rule() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "bad.json",
            r#"[{"name":"oops","emit":{"event":"bogus","fields":{}}}]"#,
        );
        let path = write(
            &dir,
            "agent.toml",
            "cmd = [\"a\"]\n[output_parsing]\ntype = \"typed\"\nrules_file = \"bad.json\"\n",
        );
        let err = load_from_path(&path).unwrap_err().to_string();
        assert!(err.contains("bad.json") && err.contains("oops"), "{err}");
    }

    #[test]
    fn unreadable_or_invalid_config_names_the_path() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("missing.toml");
        assert!(
            load_from_path(&missing)
                .unwrap_err()
                .to_string()
                .contains("missing.toml")
        );
        let bad = write(&dir, "bad.toml", "cmd = [");
        assert!(
            load_from_path(&bad)
                .unwrap_err()
                .to_string()
                .contains("bad.toml")
        );
    }

    #[test]
    fn empty_cmd_is_rejected() {
        assert!(matches!(
            load_from_str("cmd = []", Path::new(".")),
            Err(LoadError::EmptyCmd { .. })
        ));
    }

    #[test]
    fn full_config_serializes_stably_and_reloads_identically() {
        let toml_in = r#"
cmd = ["claude", "-p", "--session-id", "{{session_id}}"]
resume = ["claude", "--resume", "{{session_id}}"]
headless_resume = ["claude", "-p", "--resume", "{{session_id}}"]
parse_session_id = true

[output_parsing]
type = "typed"

[env]
allow = ["HOME", "PATH"]
deny = ["SECRET"]

[env.overrides]
NO_COLOR = "1"
"#;
        let config = load_from_str(toml_in, Path::new(".")).unwrap().config;
        assert_eq!(config.env.as_ref().unwrap().overrides["NO_COLOR"], "1");
        let serialized = toml::to_string(&config).unwrap();
        assert_eq!(serialized, toml::to_string(&config).unwrap());
        let reloaded = load_from_str(&serialized, Path::new(".")).unwrap().config;
        assert_eq!(config, reloaded);
        assert_eq!(reloaded.env, config.env);
    }
}
