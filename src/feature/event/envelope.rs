use serde::{Deserialize, Serialize};

use crate::feature::event::AgentEvent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub seq: u64,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: AgentEvent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn serializes_seq_ts_ms_and_type_at_top_level() {
        let envelope = EventEnvelope {
            seq: 5,
            ts_ms: 1_700_000_000_000,
            event: AgentEvent::Thinking {
                text: "processing".into(),
            },
        };
        let json: Value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["seq"], 5);
        assert_eq!(json["ts_ms"], 1_700_000_000_000u64);
        assert_eq!(json["type"], "thinking");
        assert_eq!(json["text"], "processing");
    }

    #[test]
    fn round_trips() {
        let envelope = EventEnvelope {
            seq: 1,
            ts_ms: 2,
            event: AgentEvent::Text {
                text: "hi".into(),
                delta: true,
            },
        };
        let serialized = serde_json::to_string(&envelope).unwrap();
        let parsed: EventEnvelope = serde_json::from_str(&serialized).unwrap();
        assert_eq!(envelope, parsed);
    }
}
