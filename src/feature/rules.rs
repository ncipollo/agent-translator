//! Data-driven rules for mapping native agent output into `AgentEvent`s.
//!
//! This module is schema, (de)serialization, file loading, and validation only; evaluating
//! rules against real JSON input lives in the mapper feature.

mod expr;
mod extractor;
mod file;
mod matcher;
mod validator;

use serde::{Deserialize, Serialize};

pub use expr::{Expr, ExprBody, MapPath};
pub use extractor::Extractor;
pub use file::{RulesFileError, parse_rules_file};
pub use matcher::Matcher;
pub use validator::validate_rules;

/// Describes when a native JSON line matches and how to build an event from it. When `iterate`
/// names an array path, each element is tested against `when_each` and emits its own event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterate: Option<String>,
    #[serde(default)]
    pub when: Vec<Matcher>,
    #[serde(default)]
    pub when_each: Vec<Matcher>,
    pub emit: Extractor,
}

/// Builder helper so later issues (the mapper, the default rule bundles) can construct rules
/// concisely in tests.
#[cfg(test)]
pub(crate) fn rule(
    name: Option<&str>,
    when: Vec<Matcher>,
    iterate: Option<&str>,
    when_each: Vec<Matcher>,
    emit: Extractor,
) -> MapRule {
    MapRule {
        name: name.map(String::from),
        iterate: iterate.map(String::from),
        when,
        when_each,
        emit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn full_rule() -> MapRule {
        let mut fields = BTreeMap::new();
        fields.insert(
            "text".to_string(),
            Expr::Structured(ExprBody {
                path: Some("message.content".into()),
                default: Some(json!("")),
                ..Default::default()
            }),
        );
        fields.insert(
            "delta".to_string(),
            Expr::Structured(ExprBody {
                concat: Some(vec![
                    Expr::Literal(json!("a")),
                    Expr::Structured(ExprBody {
                        path: Some("b".into()),
                        ..Default::default()
                    }),
                ]),
                sep: Some("-".into()),
                ..Default::default()
            }),
        );
        rule(
            Some("stream-text"),
            vec![Matcher::Eq {
                path: "type".into(),
                value: "message_delta".into(),
            }],
            Some("content"),
            vec![
                Matcher::Exists { path: ".".into() },
                Matcher::Not {
                    matcher: Box::new(Matcher::In {
                        path: "type".into(),
                        any_of: vec!["skip".into()],
                    }),
                },
            ],
            Extractor {
                event: "text".into(),
                fields,
            },
        )
    }

    #[test]
    fn round_trips_json() {
        let rule = full_rule();
        let serialized = serde_json::to_string(&rule).unwrap();
        let deserialized: MapRule = serde_json::from_str(&serialized).unwrap();
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn round_trips_toml() {
        let rule = full_rule();
        let serialized = toml::to_string(&rule).unwrap();
        let deserialized: MapRule = toml::from_str(&serialized).unwrap();
        assert_eq!(rule, deserialized);
    }

    #[test]
    fn rule_builder_matches_fields() {
        let built = rule(None, vec![], None, vec![], extractor_with_no_fields());
        assert_eq!(built.name, None);
        assert_eq!(built.iterate, None);
        assert!(built.when.is_empty());
        assert!(built.when_each.is_empty());
    }

    fn extractor_with_no_fields() -> Extractor {
        Extractor {
            event: "unknown".into(),
            fields: BTreeMap::new(),
        }
    }
}
