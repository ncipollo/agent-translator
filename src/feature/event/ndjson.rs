use std::fs;
use std::path::Path;

use crate::feature::event::{AgentEvent, EventEnvelope, push_coalescing};

/// Missing or unreadable files yield no events.
pub fn read_events(path: &Path) -> Vec<AgentEvent> {
    parse_events_from_str(&fs::read_to_string(path).unwrap_or_default())
}

/// A trailing line without a newline may still be mid-write, so it is ignored;
/// lines that fail to parse are dropped.
pub fn parse_events_from_str(raw: &str) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for line in raw.split_inclusive('\n') {
        let Some(line) = line.strip_suffix('\n') else {
            break;
        };
        if line.is_empty() {
            continue;
        }
        if let Ok(envelope) = serde_json::from_str::<EventEnvelope>(line) {
            push_coalescing(&mut events, envelope.event);
        }
    }
    events
}

pub fn to_ndjson_line(envelope: &EventEnvelope) -> serde_json::Result<String> {
    let mut line = serde_json::to_string(envelope)?;
    line.push('\n');
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn line(seq: u64, event: AgentEvent) -> String {
        to_ndjson_line(&EventEnvelope {
            seq,
            ts_ms: 1_000,
            event,
        })
        .unwrap()
    }

    fn text(text: &str, delta: bool) -> AgentEvent {
        AgentEvent::Text {
            text: text.into(),
            delta,
        }
    }

    fn thinking(text: &str) -> AgentEvent {
        AgentEvent::Thinking { text: text.into() }
    }

    #[test]
    fn to_ndjson_line_ends_with_single_newline() {
        let out = line(0, text("hi", false));
        assert!(out.ends_with('\n'));
        assert_eq!(out.matches('\n').count(), 1);
    }

    #[test]
    fn written_lines_parse_back() {
        let raw = format!(
            "{}{}",
            line(0, text("hello", false)),
            line(1, thinking("thoughts"))
        );
        assert_eq!(
            parse_events_from_str(&raw),
            vec![text("hello", false), thinking("thoughts")]
        );
    }

    #[test]
    fn drops_trailing_partial_line() {
        let partial = r#"{"seq":2,"ts_ms":3,"type":"text","text":"inc"#;
        let raw = format!("{}{partial}", line(0, text("hello", false)));
        assert_eq!(parse_events_from_str(&raw), vec![text("hello", false)]);
    }

    #[test]
    fn drops_invalid_lines() {
        let raw = format!("not json\n{}", line(0, text("hello", false)));
        assert_eq!(parse_events_from_str(&raw), vec![text("hello", false)]);
    }

    #[test]
    fn skips_blank_lines() {
        let raw = format!("\n{}\n\n", line(0, text("hi", false)));
        assert_eq!(parse_events_from_str(&raw), vec![text("hi", false)]);
    }

    #[test]
    fn empty_input_yields_no_events() {
        assert!(parse_events_from_str("").is_empty());
    }

    #[test]
    fn coalesces_adjacent_deltas() {
        let raw = format!(
            "{}{}{}",
            line(0, text("Blink", true)),
            line(1, text("ing", true)),
            line(2, text(" Line", true))
        );
        assert_eq!(
            parse_events_from_str(&raw),
            vec![text("Blinking Line", true)]
        );
    }

    #[test]
    fn non_text_event_breaks_delta_run() {
        let raw = format!(
            "{}{}{}",
            line(0, text("Hello", true)),
            line(1, thinking("t")),
            line(2, text("World", true))
        );
        assert_eq!(
            parse_events_from_str(&raw),
            vec![text("Hello", true), thinking("t"), text("World", true)]
        );
    }

    #[test]
    fn read_events_missing_file_is_empty() {
        let dir = TempDir::new().unwrap();
        assert!(read_events(&dir.path().join("events.ndjson")).is_empty());
    }

    #[test]
    fn read_events_parses_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.ndjson");
        let raw = format!(
            "{}{}",
            line(0, text("hello", false)),
            line(1, thinking("thoughts"))
        );
        fs::write(&path, raw).unwrap();
        assert_eq!(
            read_events(&path),
            vec![text("hello", false), thinking("thoughts")]
        );
    }
}
