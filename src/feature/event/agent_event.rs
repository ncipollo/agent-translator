use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The tag and field names are the public wire format; downstream fixtures depend on them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Text {
        text: String,
        #[serde(default)]
        delta: bool,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_json: Option<Value>,
        #[serde(default)]
        is_error: bool,
    },
    SystemInit {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tools: Vec<String>,
    },
    SystemNotice {
        subtype: String,
        #[serde(default)]
        level: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<Value>,
    },
    SessionResult {
        success: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    Unknown {
        raw: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(event: &AgentEvent) {
        let serialized = serde_json::to_string(event).unwrap();
        let deserialized: AgentEvent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(event, &deserialized);
    }

    fn to_json(event: &AgentEvent) -> Value {
        serde_json::to_value(event).unwrap()
    }

    #[test]
    fn round_trip_text() {
        round_trip(&AgentEvent::Text {
            text: "hello".into(),
            delta: true,
        });
        round_trip(&AgentEvent::Text {
            text: "world".into(),
            delta: false,
        });
    }

    #[test]
    fn round_trip_thinking() {
        round_trip(&AgentEvent::Thinking { text: "hmm".into() });
    }

    #[test]
    fn round_trip_tool_call() {
        round_trip(&AgentEvent::ToolCall {
            id: "tc_1".into(),
            name: "read_file".into(),
            input: json!({"path": "/foo"}),
        });
    }

    #[test]
    fn round_trip_tool_result() {
        round_trip(&AgentEvent::ToolResult {
            tool_use_id: "tc_1".into(),
            content: "contents".into(),
            content_json: Some(json!({"k": "v"})),
            is_error: false,
        });
        round_trip(&AgentEvent::ToolResult {
            tool_use_id: "tc_2".into(),
            content: "boom".into(),
            content_json: None,
            is_error: true,
        });
    }

    #[test]
    fn round_trip_system_init() {
        round_trip(&AgentEvent::SystemInit {
            session_id: "s1".into(),
            model: Some("m".into()),
            cwd: Some("/w".into()),
            tools: vec!["a".into(), "b".into()],
        });
        round_trip(&AgentEvent::SystemInit {
            session_id: "s2".into(),
            model: None,
            cwd: None,
            tools: vec![],
        });
    }

    #[test]
    fn round_trip_system_notice() {
        round_trip(&AgentEvent::SystemNotice {
            subtype: "warning".into(),
            level: "warn".into(),
            message: Some("careful".into()),
            data: Some(json!({"x": 1})),
        });
    }

    #[test]
    fn round_trip_session_result() {
        round_trip(&AgentEvent::SessionResult {
            success: true,
            duration_ms: Some(1500),
            cost_usd: Some(0.05),
            result: Some("done".into()),
            usage: Some(json!({"tokens": 100})),
            error: None,
        });
        round_trip(&AgentEvent::SessionResult {
            success: false,
            duration_ms: None,
            cost_usd: None,
            result: None,
            usage: None,
            error: Some("timeout".into()),
        });
    }

    #[test]
    fn round_trip_unknown() {
        round_trip(&AgentEvent::Unknown {
            raw: json!({"foo": "bar"}),
            reason: None,
        });
        round_trip(&AgentEvent::Unknown {
            raw: Value::String("nope".into()),
            reason: Some("invalid_json".into()),
        });
    }

    #[test]
    fn tag_is_snake_case_type() {
        let event = AgentEvent::ToolCall {
            id: "1".into(),
            name: "n".into(),
            input: Value::Null,
        };
        assert_eq!(to_json(&event)["type"], "tool_call");
    }

    #[test]
    fn none_fields_are_omitted() {
        let tool_result = to_json(&AgentEvent::ToolResult {
            tool_use_id: "1".into(),
            content: "c".into(),
            content_json: None,
            is_error: false,
        });
        assert!(tool_result.get("content_json").is_none());

        let init = to_json(&AgentEvent::SystemInit {
            session_id: "s".into(),
            model: None,
            cwd: None,
            tools: vec![],
        });
        for key in ["model", "cwd", "tools"] {
            assert!(init.get(key).is_none(), "{key} should be omitted");
        }

        let notice = to_json(&AgentEvent::SystemNotice {
            subtype: "s".into(),
            level: String::new(),
            message: None,
            data: None,
        });
        assert!(notice.get("message").is_none());
        assert!(notice.get("data").is_none());

        let result = to_json(&AgentEvent::SessionResult {
            success: true,
            duration_ms: None,
            cost_usd: None,
            result: None,
            usage: None,
            error: None,
        });
        for key in ["duration_ms", "cost_usd", "result", "usage", "error"] {
            assert!(result.get(key).is_none(), "{key} should be omitted");
        }

        let unknown = to_json(&AgentEvent::Unknown {
            raw: Value::Null,
            reason: None,
        });
        assert!(unknown.get("reason").is_none());
    }

    #[test]
    fn defaults_apply_when_fields_missing() {
        let text: AgentEvent = serde_json::from_str(r#"{"type":"text","text":"hi"}"#).unwrap();
        assert_eq!(
            text,
            AgentEvent::Text {
                text: "hi".into(),
                delta: false
            }
        );

        let call: AgentEvent =
            serde_json::from_str(r#"{"type":"tool_call","id":"1","name":"n"}"#).unwrap();
        assert_eq!(
            call,
            AgentEvent::ToolCall {
                id: "1".into(),
                name: "n".into(),
                input: Value::Null
            }
        );
    }
}
