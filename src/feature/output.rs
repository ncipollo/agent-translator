//! Output layer: fans an agent's stdout and stderr out to any number of sinks, each with its own
//! format. Raw sinks get the bytes exactly as emitted; event sinks get translated NDJSON.

mod event_writer;
mod router;
mod sink;

use std::fmt;
use std::process::ExitStatus;

pub use event_writer::{EventFanout, OpenWriter, StreamSinks, open_routes};
pub use router::{route, route_streams};
pub use sink::{ChannelSink, Destination, Format, OutputRoutes, RoutesError, Sink};

/// What a finished run reports back to its caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub exit_status: ExitStatus,
    /// The first non-empty `SystemInit.session_id`, for callers that persist it when the agent
    /// mints its own session id.
    pub session_id: Option<String>,
}

#[derive(Debug)]
pub enum OutputError {
    InvalidRoutes(RoutesError),
    Io(std::io::Error),
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputError::InvalidRoutes(source) => source.fmt(f),
            OutputError::Io(source) => write!(f, "output error: {source}"),
        }
    }
}

impl std::error::Error for OutputError {}

impl From<std::io::Error> for OutputError {
    fn from(source: std::io::Error) -> Self {
        OutputError::Io(source)
    }
}
