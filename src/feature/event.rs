//! Agent-neutral event model and its NDJSON serialization.

mod agent_event;
mod coalesce;
mod envelope;
mod ndjson;
mod unknown;

pub use agent_event::AgentEvent;
pub use coalesce::push_coalescing;
pub use envelope::EventEnvelope;
pub use ndjson::{parse_events_from_str, read_events, to_ndjson_line};
pub use unknown::wrap_line_as_unknown;
