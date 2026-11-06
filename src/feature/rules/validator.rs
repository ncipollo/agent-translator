use crate::feature::rules::MapRule;

/// Validates rules against the event model: unknown event names, unknown fields, and missing
/// required fields are errors that name the offending rule (by `name`, or by index).
pub fn validate_rules(rules: &[MapRule]) -> Result<(), String> {
    for (index, rule) in rules.iter().enumerate() {
        validate_rule(rule, &rule_label(rule, index))?;
    }
    Ok(())
}

fn rule_label(rule: &MapRule, index: usize) -> String {
    rule.name.clone().unwrap_or_else(|| format!("#{index}"))
}

fn validate_rule(rule: &MapRule, label: &str) -> Result<(), String> {
    let event = rule.emit.event.as_str();
    let Some(allowed) = allowed_fields(event) else {
        return Err(format!("unknown event '{event}' in rule {label}"));
    };
    for field in rule.emit.fields.keys() {
        if !allowed.contains(&field.as_str()) {
            return Err(format!(
                "unknown field '{field}' for event '{event}' in rule {label}"
            ));
        }
    }
    for field in required_fields(event).unwrap_or(&[]) {
        if !rule.emit.fields.contains_key(*field) {
            return Err(format!(
                "missing required field '{field}' for event '{event}' in rule {label}"
            ));
        }
    }
    Ok(())
}

fn allowed_fields(event: &str) -> Option<&'static [&'static str]> {
    match event {
        "text" => Some(&["text", "delta"]),
        "thinking" => Some(&["text"]),
        "tool_call" => Some(&["id", "name", "input"]),
        "tool_result" => Some(&["tool_use_id", "content", "content_json", "is_error"]),
        "system_init" => Some(&["session_id", "model", "cwd", "tools"]),
        "system_notice" => Some(&["subtype", "level", "message", "data"]),
        "session_result" => Some(&[
            "success",
            "duration_ms",
            "cost_usd",
            "result",
            "usage",
            "error",
        ]),
        "unknown" => Some(&["raw", "reason"]),
        _ => None,
    }
}

fn required_fields(event: &str) -> Option<&'static [&'static str]> {
    match event {
        "text" | "thinking" => Some(&["text"]),
        "tool_call" => Some(&["id", "name"]),
        "tool_result" => Some(&["tool_use_id", "content"]),
        "system_init" => Some(&["session_id"]),
        "system_notice" => Some(&["subtype"]),
        "session_result" => Some(&["success"]),
        "unknown" => Some(&["raw"]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::rules::{Expr, ExprBody, Extractor, rule};

    fn extractor(event: &str, fields: &[(&str, &str)]) -> Extractor {
        let mut map = std::collections::BTreeMap::new();
        for (key, path) in fields {
            map.insert(
                key.to_string(),
                Expr::Structured(ExprBody {
                    path: Some((*path).into()),
                    ..Default::default()
                }),
            );
        }
        Extractor {
            event: event.into(),
            fields: map,
        }
    }

    fn rule_emitting(name: Option<&str>, emit: Extractor) -> MapRule {
        rule(name, vec![], None, vec![], emit)
    }

    #[test]
    fn accepts_every_known_event_with_required_fields() {
        let rules = vec![
            rule_emitting(None, extractor("text", &[("text", "a")])),
            rule_emitting(None, extractor("thinking", &[("text", "a")])),
            rule_emitting(None, extractor("tool_call", &[("id", "a"), ("name", "b")])),
            rule_emitting(
                None,
                extractor("tool_result", &[("tool_use_id", "a"), ("content", "b")]),
            ),
            rule_emitting(None, extractor("system_init", &[("session_id", "a")])),
            rule_emitting(None, extractor("system_notice", &[("subtype", "a")])),
            rule_emitting(None, extractor("session_result", &[("success", "a")])),
            rule_emitting(None, extractor("unknown", &[("raw", "a")])),
        ];
        assert_eq!(validate_rules(&rules), Ok(()));
    }

    #[test]
    fn rejects_unknown_event_naming_rule_by_name() {
        let rules = vec![rule_emitting(Some("my-rule"), extractor("bogus", &[]))];
        assert_eq!(
            validate_rules(&rules),
            Err("unknown event 'bogus' in rule my-rule".into())
        );
    }

    #[test]
    fn rejects_unknown_field_naming_rule_by_index() {
        let rules = vec![rule_emitting(
            None,
            extractor("text", &[("text", "a"), ("bogus", "b")]),
        )];
        assert_eq!(
            validate_rules(&rules),
            Err("unknown field 'bogus' for event 'text' in rule #0".into())
        );
    }

    #[test]
    fn rejects_missing_required_field() {
        let rules = vec![rule_emitting(None, extractor("tool_call", &[("id", "a")]))];
        assert_eq!(
            validate_rules(&rules),
            Err("missing required field 'name' for event 'tool_call' in rule #0".into())
        );
    }
}
