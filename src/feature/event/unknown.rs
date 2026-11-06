use std::str;

use serde_json::Value;

use crate::feature::event::AgentEvent;

pub fn wrap_line_as_unknown(line: &[u8]) -> AgentEvent {
    let Ok(text) = str::from_utf8(line) else {
        return AgentEvent::Unknown {
            raw: Value::String(String::from_utf8_lossy(line).into_owned()),
            reason: Some("invalid_utf8".into()),
        };
    };
    match serde_json::from_str::<Value>(text) {
        Ok(raw) => AgentEvent::Unknown { raw, reason: None },
        Err(_) => AgentEvent::Unknown {
            raw: Value::String(text.into()),
            reason: Some("invalid_json".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_json_has_no_reason() {
        let event = wrap_line_as_unknown(br#"{"type":"result","data":42}"#);
        assert_eq!(
            event,
            AgentEvent::Unknown {
                raw: json!({"type": "result", "data": 42}),
                reason: None
            }
        );
    }

    #[test]
    fn invalid_json_is_flagged() {
        let event = wrap_line_as_unknown(b"not valid json");
        assert_eq!(
            event,
            AgentEvent::Unknown {
                raw: Value::String("not valid json".into()),
                reason: Some("invalid_json".into())
            }
        );
    }

    #[test]
    fn invalid_utf8_is_flagged_with_lossy_text() {
        let event = wrap_line_as_unknown(b"\xff\xfe bytes");
        assert_eq!(
            event,
            AgentEvent::Unknown {
                raw: Value::String("\u{fffd}\u{fffd} bytes".into()),
                reason: Some("invalid_utf8".into())
            }
        );
    }
}
