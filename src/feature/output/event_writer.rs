use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs::File;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::feature::event::{AgentEvent, EventEnvelope, to_ndjson_line};
use crate::feature::output::{Destination, Format, OutputRoutes, RoutesError, Sink};

/// A destination that is open for writing. A write failure is logged once and the writer is
/// dropped, so one broken sink never stops the others.
pub struct OpenWriter {
    name: String,
    writer: Option<Box<dyn AsyncWrite + Unpin + Send>>,
}

impl OpenWriter {
    pub fn new(name: impl Into<String>, writer: impl AsyncWrite + Unpin + Send + 'static) -> Self {
        Self {
            name: name.into(),
            writer: Some(Box::new(writer)),
        }
    }

    /// Writes and flushes, so tailing processes see the data immediately.
    pub async fn write(&mut self, bytes: &[u8]) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        let result = async {
            writer.write_all(bytes).await?;
            writer.flush().await
        }
        .await;
        if let Err(error) = result {
            eprintln!(
                "warning: output sink '{}' failed and was disabled: {error}",
                self.name
            );
            self.writer = None;
        }
    }
}

enum EventTarget {
    Writer(OpenWriter),
    Channel(mpsc::Sender<AgentEvent>),
}

/// Sends each event to every event sink, numbering them with one shared sequence.
#[derive(Default)]
pub struct EventFanout {
    targets: Vec<EventTarget>,
    seq: u64,
}

impl EventFanout {
    pub fn add_writer(&mut self, writer: OpenWriter) {
        self.targets.push(EventTarget::Writer(writer));
    }

    pub fn add_channel(&mut self, sender: mpsc::Sender<AgentEvent>) {
        self.targets.push(EventTarget::Channel(sender));
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub async fn emit(&mut self, event: &AgentEvent) {
        self.seq += 1;
        let envelope = EventEnvelope {
            seq: self.seq,
            ts_ms: now_ms(),
            event: event.clone(),
        };
        let line = to_ndjson_line(&envelope).ok();
        for target in &mut self.targets {
            match (target, &line) {
                (EventTarget::Writer(writer), Some(line)) => writer.write(line.as_bytes()).await,
                (EventTarget::Channel(sender), _) => {
                    let _ = sender.send(event.clone()).await;
                }
                _ => {}
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Every open sink for one run: raw writers per stream and the fan-out for events.
#[derive(Default)]
pub struct StreamSinks {
    pub stdout_raw: Vec<OpenWriter>,
    pub stderr_raw: Vec<OpenWriter>,
    pub events: EventFanout,
}

/// Validates `routes` and opens every destination.
pub async fn open_routes(
    routes: &OutputRoutes,
) -> Result<StreamSinks, crate::feature::output::OutputError> {
    routes
        .validate()
        .map_err(crate::feature::output::OutputError::InvalidRoutes)?;
    let mut sinks = StreamSinks::default();
    for sink in &routes.stdout {
        match sink.format {
            Format::Raw => sinks.stdout_raw.push(open_writer(sink).await?),
            Format::Events => add_event_sink(&mut sinks.events, sink).await?,
        }
    }
    for sink in &routes.stderr {
        sinks.stderr_raw.push(open_writer(sink).await?);
    }
    Ok(sinks)
}

async fn add_event_sink(fanout: &mut EventFanout, sink: &Sink) -> io::Result<()> {
    match &sink.destination {
        Destination::Channel(channel) => fanout.add_channel(channel.0.clone()),
        _ => fanout.add_writer(open_writer(sink).await?),
    }
    Ok(())
}

async fn open_writer(sink: &Sink) -> io::Result<OpenWriter> {
    Ok(match &sink.destination {
        Destination::Stdout => OpenWriter::new("stdout", tokio::io::stdout()),
        Destination::Stderr => OpenWriter::new("stderr", tokio::io::stderr()),
        Destination::File(path) => {
            OpenWriter::new(path.display().to_string(), File::create(path).await?)
        }
        Destination::Channel(_) => return Err(io::Error::other(RoutesError::RawChannel)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::output::{ChannelSink, Sink};
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn fanout_shares_one_sequence_across_sinks() {
        let (w1, mut r1) = tokio::io::duplex(4096);
        let (w2, mut r2) = tokio::io::duplex(4096);
        let mut fanout = EventFanout::default();
        fanout.add_writer(OpenWriter::new("a", w1));
        fanout.add_writer(OpenWriter::new("b", w2));
        fanout
            .emit(&AgentEvent::Thinking { text: "one".into() })
            .await;
        fanout
            .emit(&AgentEvent::Thinking { text: "two".into() })
            .await;
        drop(fanout);
        let (mut a, mut b) = (String::new(), String::new());
        r1.read_to_string(&mut a).await.unwrap();
        r2.read_to_string(&mut b).await.unwrap();
        let seqs: Vec<u64> = a
            .lines()
            .map(|l| serde_json::from_str::<EventEnvelope>(l).unwrap().seq)
            .collect();
        assert_eq!(seqs, [1, 2]);
        let strip = |s: &str| {
            s.lines()
                .map(|l| serde_json::from_str::<EventEnvelope>(l).unwrap().event)
                .collect::<Vec<_>>()
        };
        assert_eq!(strip(&a), strip(&b));
    }

    #[tokio::test]
    async fn a_failed_sink_is_dropped_without_stopping_the_others() {
        let (broken, reader) = tokio::io::duplex(8);
        drop(reader);
        let (good, mut good_reader) = tokio::io::duplex(4096);
        let mut fanout = EventFanout::default();
        fanout.add_writer(OpenWriter::new("broken", broken));
        fanout.add_writer(OpenWriter::new("good", good));
        fanout
            .emit(&AgentEvent::Thinking { text: "x".into() })
            .await;
        fanout
            .emit(&AgentEvent::Thinking { text: "y".into() })
            .await;
        drop(fanout);
        let mut out = String::new();
        good_reader.read_to_string(&mut out).await.unwrap();
        assert_eq!(out.lines().count(), 2);
    }

    #[tokio::test]
    async fn channel_sink_receives_events() {
        let (tx, mut rx) = mpsc::channel(4);
        let routes = OutputRoutes {
            stdout: vec![Sink {
                destination: Destination::Channel(ChannelSink(tx)),
                format: Format::Events,
            }],
            stderr: vec![],
        };
        let mut sinks = open_routes(&routes).await.unwrap();
        sinks
            .events
            .emit(&AgentEvent::Thinking { text: "hi".into() })
            .await;
        assert_eq!(
            rx.recv().await,
            Some(AgentEvent::Thinking { text: "hi".into() })
        );
    }

    #[tokio::test]
    async fn file_sinks_truncate_on_open_and_flush_each_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.ndjson");
        std::fs::write(&path, "stale contents").unwrap();
        let routes = OutputRoutes {
            stdout: vec![Sink {
                destination: Destination::File(path.clone()),
                format: Format::Raw,
            }],
            stderr: vec![],
        };
        let mut sinks = open_routes(&routes).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        sinks.stdout_raw[0].write(b"line\n").await;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "line\n");
    }

    #[tokio::test]
    async fn invalid_routes_are_rejected_before_opening() {
        let routes = OutputRoutes {
            stdout: vec![],
            stderr: vec![Sink {
                destination: Destination::Stderr,
                format: Format::Events,
            }],
        };
        assert!(open_routes(&routes).await.is_err());
    }
}
