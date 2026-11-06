use crate::feature::event::AgentEvent;

/// Appends `event`, merging it into the last element when both are `Text { delta: true }`,
/// so streamed token fragments never appear as a run of adjacent events.
pub fn push_coalescing(events: &mut Vec<AgentEvent>, event: AgentEvent) {
    if let AgentEvent::Text {
        text: new_text,
        delta: true,
    } = &event
        && let Some(AgentEvent::Text {
            text: last_text,
            delta: true,
        }) = events.last_mut()
    {
        last_text.push_str(new_text);
        return;
    }
    events.push(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(text: &str) -> AgentEvent {
        AgentEvent::Text {
            text: text.into(),
            delta: true,
        }
    }

    fn non_delta(text: &str) -> AgentEvent {
        AgentEvent::Text {
            text: text.into(),
            delta: false,
        }
    }

    #[test]
    fn merges_adjacent_deltas() {
        let mut events = Vec::new();
        push_coalescing(&mut events, delta("Blink"));
        push_coalescing(&mut events, delta("ing"));
        push_coalescing(&mut events, delta(" Line"));
        assert_eq!(events, vec![delta("Blinking Line")]);
    }

    #[test]
    fn keeps_non_delta_text_separate() {
        let mut events = Vec::new();
        push_coalescing(&mut events, non_delta("Hello"));
        push_coalescing(&mut events, non_delta("World"));
        assert_eq!(events, vec![non_delta("Hello"), non_delta("World")]);
    }

    #[test]
    fn does_not_merge_delta_into_non_delta() {
        let mut events = Vec::new();
        push_coalescing(&mut events, non_delta("a"));
        push_coalescing(&mut events, delta("b"));
        assert_eq!(events, vec![non_delta("a"), delta("b")]);
    }

    #[test]
    fn non_text_event_resets_the_run() {
        let thinking = AgentEvent::Thinking { text: "t".into() };
        let mut events = Vec::new();
        push_coalescing(&mut events, delta("Hello"));
        push_coalescing(&mut events, thinking.clone());
        push_coalescing(&mut events, delta("World"));
        assert_eq!(events, vec![delta("Hello"), thinking, delta("World")]);
    }

    #[test]
    fn single_delta_is_stored_unchanged() {
        let mut events = Vec::new();
        push_coalescing(&mut events, delta("only"));
        assert_eq!(events, vec![delta("only")]);
    }
}
