use std::fmt;
use std::path::PathBuf;

use tokio::sync::mpsc;

use crate::feature::event::AgentEvent;

/// Lets embedding code observe translated events live, without a file or stdio.
#[derive(Debug, Clone)]
pub struct ChannelSink(pub mpsc::Sender<AgentEvent>);

#[derive(Debug, Clone)]
pub enum Destination {
    Stdout,
    Stderr,
    /// Truncated on open and flushed after every write so another process can tail it.
    File(PathBuf),
    /// Only valid with `Format::Events`.
    Channel(ChannelSink),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Bytes exactly as the agent emitted them.
    Raw,
    /// One `EventEnvelope` per line. `seq` is shared across all event sinks of a run.
    Events,
}

#[derive(Debug, Clone)]
pub struct Sink {
    pub destination: Destination,
    pub format: Format,
}

/// The sinks for each of the child's streams.
#[derive(Debug, Clone, Default)]
pub struct OutputRoutes {
    pub stdout: Vec<Sink>,
    pub stderr: Vec<Sink>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RoutesError {
    /// stderr is diagnostics and is only ever passed through raw.
    EventsOnStderr,
    RawChannel,
}

impl fmt::Display for RoutesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoutesError::EventsOnStderr => {
                write!(f, "the events format is not supported on stderr; use raw")
            }
            RoutesError::RawChannel => write!(f, "a channel sink only accepts the events format"),
        }
    }
}

impl std::error::Error for RoutesError {}

impl OutputRoutes {
    pub fn validate(&self) -> Result<(), RoutesError> {
        if self.stderr.iter().any(|sink| sink.format == Format::Events) {
            return Err(RoutesError::EventsOnStderr);
        }
        let raw_channel = |sink: &Sink| {
            sink.format == Format::Raw && matches!(sink.destination, Destination::Channel(_))
        };
        if self.stdout.iter().chain(&self.stderr).any(raw_channel) {
            return Err(RoutesError::RawChannel);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink(destination: Destination, format: Format) -> Sink {
        Sink {
            destination,
            format,
        }
    }

    #[test]
    fn events_on_stderr_fail_validation() {
        let routes = OutputRoutes {
            stdout: vec![],
            stderr: vec![sink(Destination::Stderr, Format::Events)],
        };
        assert_eq!(routes.validate(), Err(RoutesError::EventsOnStderr));
    }

    #[test]
    fn raw_channel_fails_validation() {
        let (tx, _rx) = mpsc::channel(1);
        let routes = OutputRoutes {
            stdout: vec![sink(Destination::Channel(ChannelSink(tx)), Format::Raw)],
            stderr: vec![],
        };
        assert_eq!(routes.validate(), Err(RoutesError::RawChannel));
    }

    #[test]
    fn sensible_routes_validate() {
        let (tx, _rx) = mpsc::channel(1);
        let routes = OutputRoutes {
            stdout: vec![
                sink(Destination::Stdout, Format::Events),
                sink(Destination::File("n.ndjson".into()), Format::Raw),
                sink(Destination::Channel(ChannelSink(tx)), Format::Events),
            ],
            stderr: vec![sink(Destination::Stderr, Format::Raw)],
        };
        assert_eq!(routes.validate(), Ok(()));
    }
}
