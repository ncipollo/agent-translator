use serde_json::{Map, Value};

use crate::feature::event::{AgentEvent, wrap_line_as_unknown};
use crate::feature::rules::{Extractor, MapRule, Matcher};
use crate::feature::translate::expr::evaluate;
use crate::feature::translate::path::resolve;

/// Evaluates parsing rules against native JSON lines.
#[derive(Debug, Clone)]
pub struct EventMapper {
    rules: Vec<MapRule>,
}

impl EventMapper {
    pub fn new(rules: Vec<MapRule>) -> Self {
        Self { rules }
    }

    /// Rules are evaluated in order. A matching rule without `iterate` wins immediately and
    /// emits exactly one event. Matching `iterate` rules all contribute, ordered by array
    /// element index. Anything unrecognized becomes an `Unknown` event.
    pub fn map_line(&self, line: &[u8]) -> Vec<AgentEvent> {
        let parsed = std::str::from_utf8(line)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        match parsed {
            Some(root) => self.map_value(&root),
            None => vec![wrap_line_as_unknown(line)],
        }
    }

    fn map_value(&self, root: &Value) -> Vec<AgentEvent> {
        let mut collected: Vec<(usize, AgentEvent)> = Vec::new();
        for rule in &self.rules {
            if !all_match(&rule.when, root) {
                continue;
            }
            match &rule.iterate {
                None => return vec![build_event(&rule.emit, root, root)],
                Some(path) => collected.extend(iterate_rule(rule, path, root)),
            }
        }
        if collected.is_empty() {
            return vec![AgentEvent::Unknown {
                raw: root.clone(),
                reason: None,
            }];
        }
        collected.sort_by_key(|(index, _)| *index);
        collected.into_iter().map(|(_, event)| event).collect()
    }
}

fn iterate_rule(rule: &MapRule, path: &str, root: &Value) -> Vec<(usize, AgentEvent)> {
    let Some(items) = resolve(root, path).and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| all_match(&rule.when_each, item))
        .map(|(index, item)| (index, build_event(&rule.emit, item, root)))
        .collect()
}

fn build_event(extractor: &Extractor, scope: &Value, root: &Value) -> AgentEvent {
    let mut object = Map::new();
    object.insert("type".into(), Value::String(extractor.event.clone()));
    for (name, expr) in &extractor.fields {
        if let Some(value) = evaluate(expr, scope) {
            object.insert(name.clone(), value);
        }
    }
    serde_json::from_value(Value::Object(object)).unwrap_or_else(|error| AgentEvent::Unknown {
        raw: root.clone(),
        reason: Some(format!("extractor: {error}")),
    })
}

fn all_match(matchers: &[Matcher], scope: &Value) -> bool {
    matchers.iter().all(|matcher| matches(matcher, scope))
}

/// `eq` and `in` compare string values only; `exists` is true for any non-null value.
fn matches(matcher: &Matcher, scope: &Value) -> bool {
    match matcher {
        Matcher::Eq { path, value } => string_at(scope, path) == Some(value.as_str()),
        Matcher::In { path, any_of } => {
            string_at(scope, path).is_some_and(|found| any_of.iter().any(|v| v == found))
        }
        Matcher::Exists { path } => resolve(scope, path).is_some_and(|v| !v.is_null()),
        Matcher::Not { matcher } => !matches(matcher, scope),
    }
}

fn string_at<'a>(scope: &'a Value, path: &str) -> Option<&'a str> {
    resolve(scope, path).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mapper(rules: Value) -> EventMapper {
        EventMapper::new(serde_json::from_value(rules).unwrap())
    }

    fn text_rule(kind: &str, event: &str) -> Value {
        json!({
            "name": kind,
            "when": [{"op": "eq", "path": "type", "value": kind}],
            "emit": {"event": event, "fields": {"text": {"path": "body"}}}
        })
    }

    fn map(mapper: &EventMapper, line: Value) -> Vec<AgentEvent> {
        mapper.map_line(line.to_string().as_bytes())
    }

    #[test]
    fn invalid_utf8_yields_unknown_with_reason() {
        let events = mapper(json!([])).map_line(b"\xff\xfe");
        assert!(matches!(
            events.as_slice(),
            [AgentEvent::Unknown { reason: Some(r), .. }] if r == "invalid_utf8"
        ));
    }

    #[test]
    fn invalid_json_yields_unknown_with_raw_text() {
        let events = mapper(json!([])).map_line(b"not json");
        assert_eq!(
            events,
            vec![AgentEvent::Unknown {
                raw: json!("not json"),
                reason: Some("invalid_json".into())
            }]
        );
    }

    #[test]
    fn unmatched_json_yields_unknown_without_reason() {
        let m = mapper(json!([text_rule("a", "text")]));
        let events = map(&m, json!({"type": "b"}));
        assert_eq!(
            events,
            vec![AgentEvent::Unknown {
                raw: json!({"type": "b"}),
                reason: None
            }]
        );
    }

    #[test]
    fn first_matching_rule_without_iterate_wins() {
        let m = mapper(json!([text_rule("a", "thinking"), text_rule("a", "text")]));
        let events = map(&m, json!({"type": "a", "body": "hi"}));
        assert_eq!(events, vec![AgentEvent::Thinking { text: "hi".into() }]);
    }

    #[test]
    fn iterate_rules_collect_events_ordered_by_element_index() {
        let m = mapper(json!([
            {
                "when": [{"op": "eq", "path": "type", "value": "assistant"}],
                "iterate": "content",
                "when_each": [{"op": "eq", "path": "kind", "value": "text"}],
                "emit": {"event": "text", "fields": {"text": {"path": "v"}}}
            },
            {
                "when": [{"op": "eq", "path": "type", "value": "assistant"}],
                "iterate": "content",
                "when_each": [{"op": "eq", "path": "kind", "value": "think"}],
                "emit": {"event": "thinking", "fields": {"text": {"path": "v"}}}
            }
        ]));
        let line = json!({"type": "assistant", "content": [
            {"kind": "text", "v": "one"},
            {"kind": "think", "v": "two"},
            {"kind": "text", "v": "three"},
            {"kind": "other", "v": "skipped"}
        ]});
        assert_eq!(
            map(&m, line),
            vec![
                AgentEvent::Text {
                    text: "one".into(),
                    delta: false
                },
                AgentEvent::Thinking { text: "two".into() },
                AgentEvent::Text {
                    text: "three".into(),
                    delta: false
                },
            ]
        );
    }

    #[test]
    fn iterate_with_no_matching_elements_is_unknown() {
        let m = mapper(json!([{
            "when": [], "iterate": "content",
            "when_each": [{"op": "eq", "path": "kind", "value": "text"}],
            "emit": {"event": "text", "fields": {"text": {"path": "v"}}}
        }]));
        let events = map(&m, json!({"content": [{"kind": "x"}]}));
        assert!(matches!(
            events.as_slice(),
            [AgentEvent::Unknown { reason: None, .. }]
        ));
    }

    #[test]
    fn extractor_failure_yields_unknown_with_serde_error() {
        let m = mapper(json!([{
            "when": [],
            "emit": {"event": "text", "fields": {"text": {"path": "missing"}}}
        }]));
        let events = map(&m, json!({"type": "a"}));
        assert!(matches!(
            events.as_slice(),
            [AgentEvent::Unknown { reason: Some(r), raw }]
                if r.starts_with("extractor: ") && raw == &json!({"type": "a"})
        ));
    }

    #[test]
    fn in_and_not_and_exists_matchers() {
        let m = mapper(json!([{
            "when": [
                {"op": "in", "path": "type", "any_of": ["a", "b"]},
                {"op": "exists", "path": "body"},
                {"op": "not", "matcher": {"op": "exists", "path": "skip"}}
            ],
            "emit": {"event": "text", "fields": {"text": {"path": "body"}}}
        }]));
        let hit = map(&m, json!({"type": "b", "body": "ok"}));
        assert_eq!(
            hit,
            vec![AgentEvent::Text {
                text: "ok".into(),
                delta: false
            }]
        );
        for miss in [
            json!({"type": "c", "body": "x"}),
            json!({"type": "a"}),
            json!({"type": "a", "body": "x", "skip": 1}),
        ] {
            assert!(matches!(
                map(&m, miss).as_slice(),
                [AgentEvent::Unknown { .. }]
            ));
        }
    }

    #[test]
    fn eq_compares_strings_only() {
        let m = mapper(json!([{
            "when": [{"op": "eq", "path": "n", "value": "1"}],
            "emit": {"event": "text", "fields": {"text": {"value": "hit"}}}
        }]));
        assert!(matches!(
            map(&m, json!({"n": 1})).as_slice(),
            [AgentEvent::Unknown { .. }]
        ));
        assert!(matches!(
            map(&m, json!({"n": "1"})).as_slice(),
            [AgentEvent::Text { .. }]
        ));
    }
}
