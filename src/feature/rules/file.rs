use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::feature::rules::MapRule;

#[derive(Debug)]
pub struct RulesFileError {
    path: PathBuf,
    kind: RulesFileErrorKind,
}

#[derive(Debug)]
enum RulesFileErrorKind {
    Read(std::io::Error),
    UnsupportedExtension,
    Json(serde_json::Error),
    Toml(toml::de::Error),
}

impl fmt::Display for RulesFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        match &self.kind {
            RulesFileErrorKind::Read(source) => {
                write!(f, "failed to read rules file '{path}': {source}")
            }
            RulesFileErrorKind::UnsupportedExtension => {
                write!(f, "unsupported rules file extension: '{path}'")
            }
            RulesFileErrorKind::Json(source) => {
                write!(f, "failed to parse rules file '{path}' as JSON: {source}")
            }
            RulesFileErrorKind::Toml(source) => {
                write!(f, "failed to parse rules file '{path}' as TOML: {source}")
            }
        }
    }
}

impl std::error::Error for RulesFileError {}

#[derive(Serialize, Deserialize)]
struct TomlRulesFile {
    #[serde(default)]
    rules: Vec<MapRule>,
}

/// Loads rules from a `.json` file (a top-level array of `MapRule`) or a `.toml` file
/// (a `rules = [...]` key). Errors always include the file path.
pub fn parse_rules_file(path: &Path) -> Result<Vec<MapRule>, RulesFileError> {
    let error = |kind: RulesFileErrorKind| RulesFileError {
        path: path.to_path_buf(),
        kind,
    };
    let raw = fs::read_to_string(path).map_err(|source| error(RulesFileErrorKind::Read(source)))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => {
            serde_json::from_str(&raw).map_err(|source| error(RulesFileErrorKind::Json(source)))
        }
        Some("toml") => toml::from_str::<TomlRulesFile>(&raw)
            .map(|file| file.rules)
            .map_err(|source| error(RulesFileErrorKind::Toml(source))),
        _ => Err(error(RulesFileErrorKind::UnsupportedExtension)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::rules::{Expr, ExprBody, Extractor, Matcher, rule};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn sample_rules() -> Vec<MapRule> {
        let mut fields = BTreeMap::new();
        fields.insert(
            "text".to_string(),
            Expr::Structured(ExprBody {
                path: Some("message.content".into()),
                ..Default::default()
            }),
        );
        vec![rule(
            Some("text-rule"),
            vec![Matcher::Eq {
                path: "type".into(),
                value: "text".into(),
            }],
            None,
            vec![],
            Extractor {
                event: "text".into(),
                fields,
            },
        )]
    }

    #[test]
    fn parses_json_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.json");
        fs::write(&path, serde_json::to_string(&sample_rules()).unwrap()).unwrap();
        assert_eq!(parse_rules_file(&path).unwrap(), sample_rules());
    }

    #[test]
    fn parses_toml_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        let wrapper = TomlRulesFile {
            rules: sample_rules(),
        };
        fs::write(&path, toml::to_string(&wrapper).unwrap()).unwrap();
        assert_eq!(parse_rules_file(&path).unwrap(), sample_rules());
    }

    #[test]
    fn missing_file_error_includes_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("missing.json");
        let err = parse_rules_file(&path).unwrap_err();
        assert!(err.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn malformed_json_error_includes_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.json");
        fs::write(&path, "not json").unwrap();
        let err = parse_rules_file(&path).unwrap_err();
        assert!(err.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn malformed_toml_error_includes_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        fs::write(&path, "not = [valid").unwrap();
        let err = parse_rules_file(&path).unwrap_err();
        assert!(err.to_string().contains(&path.display().to_string()));
    }

    #[test]
    fn unsupported_extension_error_includes_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.yaml");
        fs::write(&path, "rules = []").unwrap();
        let err = parse_rules_file(&path).unwrap_err();
        assert!(err.to_string().contains(&path.display().to_string()));
    }
}
